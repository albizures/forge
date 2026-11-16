//! Revision-isolated copy-and-upgrade orchestration. No application SQL lives here.

#[cfg(all(test, unix))]
#[path = "upgrade_tests.rs"]
mod tests;
use super::{
    ForgeupError, ForgeupPaths, Installation, Ownership, Registry, RegistryStore, atomic_write,
    daemon_record_path, offline_output, proved_owned, read_daemon_record, reconcile_active_path,
    resolve_in_registry, secret, set_private_permissions, validate_identity, validate_installation,
    validate_label,
};
use forge_app::storage_upgrade::{StoreInspection, UPGRADE_INTERFACE_VERSION, UpgradeCapabilities};
use fs4::fs_std::FileExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    process::Command,
};

const RECEIPT: &str = "upgrade.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradePlan {
    pub source: String,
    pub target: String,
    pub source_store: StoreInspection,
    pub destination_store: StoreInspection,
    pub migrations: Vec<(u32, u32)>,
    pub blockers: Vec<String>,
    pub requirements: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpgradePhase {
    Preparing,
    ReadyToPublish,
    Prepared,
    Activating,
    Activated,
    Discarding,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeReceipt {
    pub interface_version: u32,
    pub id: String,
    pub source: Installation,
    pub target: Installation,
    pub phase: UpgradePhase,
    pub source_store: StoreInspection,
    pub original_destination: StoreInspection,
    pub upgraded_store: Option<StoreInspection>,
    pub staging_file: Option<String>,
    pub migrations: Vec<(u32, u32)>,
    pub accepted_stale_source: bool,
    pub source_restart_acknowledged: bool,
}

fn blocked(message: impl Into<String>) -> ForgeupError {
    ForgeupError::UpgradeBlocked(message.into())
}
fn io_error(path: &Path, source: io::Error) -> ForgeupError {
    ForgeupError::Io {
        path: path.into(),
        source,
    }
}
fn receipt_path(paths: &ForgeupPaths, target: &str) -> PathBuf {
    paths.data.join("state").join(target).join(RECEIPT)
}
fn artifacts(paths: &ForgeupPaths, receipt: &UpgradeReceipt) -> PathBuf {
    paths
        .data
        .join("state")
        .join(&receipt.target.selector)
        .join("upgrades")
        .join(&receipt.id)
}
fn regular(path: &Path) -> Result<(), ForgeupError> {
    if !fs::symlink_metadata(path)
        .map_err(|e| io_error(path, e))?
        .is_file()
    {
        return Err(blocked("upgrade paths must be regular files, not symlinks"));
    }
    Ok(())
}
fn contained(paths: &ForgeupPaths, path: &Path) -> Result<(), ForgeupError> {
    let root = paths
        .data
        .canonicalize()
        .map_err(|e| io_error(&paths.data, e))?;
    let parent = path.parent().ok_or_else(|| blocked("missing parent"))?;
    let canonical = parent.canonicalize().map_err(|e| io_error(parent, e))?;
    if !canonical.starts_with(root) || canonical != parent {
        return Err(blocked(
            "upgrade path redirects outside its canonical Forgeup location",
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) =>
        {
            return Err(blocked("upgrade path must not redirect"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(path, error)),
    }
    Ok(())
}
fn sync_directory(path: &Path) -> Result<(), ForgeupError> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| io_error(path, e))?;
    Ok(())
}
fn save(paths: &ForgeupPaths, receipt: &UpgradeReceipt) -> Result<(), ForgeupError> {
    let path = receipt_path(paths, &receipt.target.selector);
    contained(paths, &path)?;
    atomic_write(
        &path,
        &serde_json::to_vec_pretty(receipt).map_err(|e| blocked(e.to_string()))?,
    )?;
    sync_directory(
        path.parent()
            .ok_or_else(|| blocked("missing receipt parent"))?,
    )
}
fn load(paths: &ForgeupPaths, target: &str) -> Result<Option<UpgradeReceipt>, ForgeupError> {
    let path = receipt_path(paths, target);
    if !path.exists() {
        return Ok(None);
    }
    contained(paths, &path)?;
    let receipt: UpgradeReceipt =
        serde_json::from_slice(&fs::read(&path).map_err(|e| io_error(&path, e))?)
            .map_err(|_| blocked("invalid upgrade receipt; retain it for recovery"))?;
    validate_label(&receipt.id)?;
    if let Some(name) = &receipt.staging_file {
        validate_label(name)?;
        let nonce = name
            .strip_prefix("staging-")
            .and_then(|v| v.strip_suffix(".db"));
        if nonce.is_none_or(|v| v.len() != 64 || !v.bytes().all(|b| b.is_ascii_hexdigit())) {
            return Err(blocked("invalid staging identity"));
        }
    }
    if receipt.interface_version != UPGRADE_INTERFACE_VERSION || receipt.target.selector != target {
        return Err(blocked("unsupported or misplaced upgrade receipt"));
    }
    // Selectors are also path components; never trust a receipt to supply arbitrary paths.
    for item in [&receipt.source, &receipt.target] {
        validate_label(&item.source_label)?;
        validate_identity(&item.identity)?;
        if item.selector != format!("{}@{}", item.source_label, item.identity.commit) {
            return Err(blocked("invalid receipt selector"));
        }
    }
    Ok(Some(receipt))
}
fn receipts(paths: &ForgeupPaths) -> Result<Vec<UpgradeReceipt>, ForgeupError> {
    let root = paths.data.join("state");
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| io_error(&root, e))? {
        let entry = entry.map_err(|e| io_error(&root, e))?;
        if !entry
            .file_type()
            .map_err(|e| io_error(&entry.path(), e))?
            .is_dir()
        {
            return Err(blocked("unexpected entry in Revision State root"));
        }
        let selector = entry
            .file_name()
            .into_string()
            .map_err(|_| blocked("invalid state directory"))?;
        if let Some(receipt) = load(paths, &selector)? {
            result.push(receipt);
        }
    }
    Ok(result)
}
fn invoke<T: DeserializeOwned>(
    target: &Installation,
    operation: &str,
    paths: &[&Path],
) -> Result<T, ForgeupError> {
    let mut command = Command::new(&target.daemon_path);
    command.args(["--state-upgrade", operation]).args(paths);
    for (key, _) in env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("FORGE_")) {
        command.env_remove(key);
    }
    let output = offline_output(&mut command).map_err(|e| io_error(&target.daemon_path, e))?;
    if !output.status.success() {
        return Err(blocked(format!(
            "target {operation} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(|_| {
        blocked(format!(
            "target does not implement upgrade interface {UPGRADE_INTERFACE_VERSION}: {operation}"
        ))
    })
}
fn inspect(target: &Installation, path: &Path) -> Result<StoreInspection, ForgeupError> {
    regular(path)?;
    invoke(target, "inspect", &[path])
}
fn handshake(target: &Installation) -> Result<UpgradeCapabilities, ForgeupError> {
    validate_installation(target)?;
    let caps: UpgradeCapabilities = invoke(target, "capabilities", &[])?;
    if caps.interface_version != UPGRADE_INTERFACE_VERSION {
        return Err(blocked("unsupported target upgrade interface"));
    }
    Ok(caps)
}
fn daemon_blocker(
    paths: &ForgeupPaths,
    item: &Installation,
) -> Result<Option<String>, ForgeupError> {
    let path = daemon_record_path(paths, &item.selector);
    if path.exists() {
        let record = read_daemon_record(&path)?;
        if !matches!(proved_owned(&record, item), Ownership::Stale) {
            return Ok(Some(format!(
                "{} daemon is running or ambiguous",
                item.selector
            )));
        }
    }
    Ok(None)
}

/// Daemons and upgrades use precisely the same nonblocking sidecar lock.
struct OwnershipLocks {
    files: Vec<File>,
}
impl Drop for OwnershipLocks {
    fn drop(&mut self) {
        // Explicit unlock also releases descriptors transiently inherited by a
        // concurrently spawned child before exec closes its CLOEXEC descriptors.
        for file in &self.files {
            if let Err(error) = FileExt::unlock(file) {
                eprintln!("forgeup: ownership unlock failed: {error}");
            }
        }
    }
}
impl OwnershipLocks {
    fn acquire(
        paths: &ForgeupPaths,
        source: &Installation,
        target: &Installation,
    ) -> Result<Self, ForgeupError> {
        let mut databases = [
            paths.state(&source.selector).database,
            paths.state(&target.selector).database,
        ];
        databases.sort();
        let mut locks = Self { files: Vec::new() };
        for database in databases {
            contained(paths, &database)?;
            regular(&database)?;
            let mut lock = database.into_os_string();
            lock.push(".lock");
            let lock = PathBuf::from(lock);
            contained(paths, &lock)?;
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock)
                .map_err(|e| io_error(&lock, e))?;
            set_private_permissions(&lock)?;
            if !file.try_lock_exclusive().map_err(|e| io_error(&lock, e))? {
                return Err(blocked(format!(
                    "database ownership unavailable: {}",
                    lock.display()
                )));
            }
            locks.files.push(file);
        }
        for item in [source, target] {
            if let Some(reason) = daemon_blocker(paths, item)? {
                return Err(blocked(reason));
            }
        }
        Ok(locks)
    }
}

fn build_plan(
    paths: &ForgeupPaths,
    registry: &Registry,
    from: &str,
    to: &str,
) -> Result<(UpgradePlan, Installation, Installation), ForgeupError> {
    let source = resolve_in_registry(registry, from)?;
    let target = resolve_in_registry(registry, to)?;
    if source.selector == target.selector {
        return Err(blocked("source and target must be distinct revisions"));
    }
    validate_installation(&source)?;
    let caps = handshake(&target)?;
    for item in [&source, &target] {
        contained(paths, &paths.state(&item.selector).database)?;
    }
    let source_store = inspect(&target, &paths.state(&source.selector).database)?;
    let destination_store = inspect(&target, &paths.state(&target.selector).database)?;
    let schema = source_store
        .schema
        .ok_or_else(|| blocked("source is not an initialized Forge database"))?;
    if !caps.supported_sources.contains(&schema) || schema > caps.target_schema {
        return Err(blocked("no supported forward migration chain"));
    }
    let mut blockers = source_store.blockers.clone();
    blockers.extend(destination_store.blockers.clone());
    if !destination_store.pristine {
        blockers.push(
            "destination contains user state; implicit overwrite and merge are forbidden".into(),
        );
    }
    for item in [&source, &target] {
        if let Some(reason) = daemon_blocker(paths, item)? {
            blockers.push(reason);
        }
    }
    for receipt in receipts(paths)? {
        if [&source.selector, &target.selector].contains(&&receipt.target.selector)
            || [&source.selector, &target.selector].contains(&&receipt.source.selector)
        {
            // A completed target may itself be carried forward. Retired sources
            // and incomplete upgrades require explicit recovery/acknowledgement.
            if receipt.phase != UpgradePhase::Activated
                || (receipt.source.selector == source.selector
                    && !receipt.source_restart_acknowledged)
                || receipt.target.selector == target.selector
            {
                blockers.push(format!(
                    "existing upgrade {} requires inspection or recovery",
                    receipt.id
                ));
            }
        }
    }
    let migrations = (schema..caps.target_schema).map(|v| (v, v + 1)).collect();
    Ok((
        UpgradePlan {
            source: source.selector.clone(),
            target: target.selector.clone(),
            source_store,
            destination_store,
            migrations,
            blockers,
            requirements: vec![
                "Verify agents cannot act; stopping a daemon alone is insufficient".into(),
                "Preparation requires registry schema 2; older Forgeup and older clients with embedded managed resolution will refuse it".into(),
                "Rollback keeps the old database; use updated Forgeup for lifecycle and an explicit endpoint with older clients".into(),
                "Registered paths are preserved; runtime records, logs, and external resources are not transferred".into(),
            ],
        },
        source,
        target,
    ))
}

/// Read-only preview; does not create a registry, state, snapshots, or ownership locks.
pub fn plan(from: &str, to: &str) -> Result<UpgradePlan, ForgeupError> {
    plan_at(&ForgeupPaths::from_environment()?, from, to)
}
fn plan_at(paths: &ForgeupPaths, from: &str, to: &str) -> Result<UpgradePlan, ForgeupError> {
    let store = RegistryStore::open_read_only(paths.clone())?
        .ok_or_else(|| blocked("no registered revisions"))?;
    let registry = store.load_read_only()?;
    Ok(build_plan(paths, &registry, from, to)?.0)
}

/// Prepare state only. This never selects or starts a revision.
pub fn prepare(
    from: &str,
    to: &str,
    confirm_agents_stopped: bool,
) -> Result<UpgradeReceipt, ForgeupError> {
    prepare_at(
        &ForgeupPaths::from_environment()?,
        from,
        to,
        confirm_agents_stopped,
    )
}
fn prepare_at(
    paths: &ForgeupPaths,
    from: &str,
    to: &str,
    confirmed: bool,
) -> Result<UpgradeReceipt, ForgeupError> {
    if !confirmed {
        return Err(blocked(
            "confirm agents cannot act with --confirm-agents-stopped after operator verification",
        ));
    }
    let store = RegistryStore::open(paths.clone())?;
    let mut registry = store.load()?;
    let source = resolve_in_registry(&registry, from)?;
    let target = resolve_in_registry(&registry, to)?;
    if source.selector == target.selector {
        return Err(blocked("source and target must differ"));
    }
    let _locks = OwnershipLocks::acquire(paths, &source, &target)?;
    if let Some(mut receipt) = load(paths, &target.selector)? {
        if receipt.source != source || receipt.target != target {
            return Err(blocked("conflicting upgrade retry or changed installation"));
        }
        return finish_preparation(paths, &mut receipt);
    }
    let (plan, source, target) = build_plan(paths, &registry, from, to)?;
    if !plan.blockers.is_empty() {
        return Err(blocked(plan.blockers.join("; ")));
    }
    let mut receipt = UpgradeReceipt {
        interface_version: UPGRADE_INTERFACE_VERSION,
        id: secret()?,
        source,
        target,
        phase: UpgradePhase::Preparing,
        source_store: plan.source_store,
        original_destination: plan.destination_store,
        upgraded_store: None,
        staging_file: None,
        migrations: plan.migrations,
        accepted_stale_source: false,
        source_restart_acknowledged: false,
    };
    let directory = artifacts(paths, &receipt);
    let parent = directory
        .parent()
        .ok_or_else(|| blocked("missing artifact parent"))?;
    for dir in [parent, directory.as_path()] {
        contained(paths, dir)?;
        if !dir.exists() {
            fs::create_dir(dir).map_err(|e| io_error(dir, e))?;
        }
        if !fs::symlink_metadata(dir)
            .map_err(|e| io_error(dir, e))?
            .is_dir()
        {
            return Err(blocked("artifact directory redirects"));
        }
        set_private_permissions(dir)?;
    }
    contained(paths, &directory.join("source.db"))?;
    // Old clients embed an older Forgeup library. Fence them out before any
    // upgrade intent: otherwise they could bypass the new lineage/start guards.
    registry.schema_version = 2;
    store.save(&registry)?;
    sync_directory(&paths.config)?;
    save(paths, &receipt)?;
    finish_preparation(paths, &mut receipt)
}
fn current_source(
    paths: &ForgeupPaths,
    receipt: &UpgradeReceipt,
) -> Result<StoreInspection, ForgeupError> {
    inspect(
        &receipt.target,
        &paths.state(&receipt.source.selector).database,
    )
}
fn current_destination(
    paths: &ForgeupPaths,
    receipt: &UpgradeReceipt,
) -> Result<StoreInspection, ForgeupError> {
    inspect(
        &receipt.target,
        &paths.state(&receipt.target.selector).database,
    )
}
fn sync_file(path: &Path) -> Result<(), ForgeupError> {
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| io_error(path, e))
}
fn remove_owned(path: &Path) -> Result<(), ForgeupError> {
    if path.exists() {
        regular(path)?;
        fs::remove_file(path).map_err(|e| io_error(path, e))?;
    }
    Ok(())
}
fn finish_preparation(
    paths: &ForgeupPaths,
    receipt: &mut UpgradeReceipt,
) -> Result<UpgradeReceipt, ForgeupError> {
    handshake(&receipt.target)?;
    if receipt.phase == UpgradePhase::Discarding {
        return Err(blocked(
            "discard is in progress; use upgrade recover/discard",
        ));
    }
    if matches!(
        receipt.phase,
        UpgradePhase::Prepared | UpgradePhase::Activating | UpgradePhase::Activated
    ) {
        if receipt.phase != UpgradePhase::Activated
            && Some(current_destination(paths, receipt)?) != receipt.upgraded_store
        {
            return Err(blocked("prepared destination changed"));
        }
        return Ok(receipt.clone());
    }
    if current_source(paths, receipt)? != receipt.source_store {
        return Err(blocked(
            "source changed; recover the incomplete upgrade and prepare again",
        ));
    }
    let directory = artifacts(paths, receipt);
    let backup = directory.join("source.db");
    contained(paths, &backup)?;
    if receipt.phase == UpgradePhase::Preparing {
        if current_destination(paths, receipt)? != receipt.original_destination {
            return Err(blocked("destination changed during preparation"));
        }
        if backup.exists() {
            if inspect(&receipt.target, &backup)? != receipt.source_store {
                return Err(blocked("incomplete source snapshot; run upgrade recover"));
            }
        } else {
            let snapshot: StoreInspection = invoke(
                &receipt.target,
                "snapshot",
                &[&paths.state(&receipt.source.selector).database, &backup],
            )?;
            if snapshot != receipt.source_store {
                return Err(blocked("source snapshot differs from planned state"));
            }
        }
        set_private_permissions(&backup)?;
        sync_file(&backup)?;
        // A worker may outlive a killed manager. Never reuse or unlink its
        // staging pathname: each attempt gets a distinct durable identity.
        receipt.staging_file = Some(format!("staging-{}.db", secret()?));
        save(paths, receipt)?;
        let staging = staging_path(paths, receipt)?;
        drop(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staging)
                .map_err(|e| io_error(&staging, e))?,
        );
        fs::copy(&backup, &staging).map_err(|e| io_error(&staging, e))?;
        set_private_permissions(&staging)?;
        let upgraded: StoreInspection = invoke(&receipt.target, "migrate", &[&staging])?;
        if upgraded.schema != Some(handshake(&receipt.target)?.target_schema)
            || !upgraded.blockers.is_empty()
        {
            return Err(blocked("target returned invalid migrated state"));
        }
        if inspect(&receipt.target, &staging)? != upgraded {
            return Err(blocked("migrated state failed independent validation"));
        }
        sync_file(&staging)?;
        receipt.upgraded_store = Some(upgraded);
        receipt.phase = UpgradePhase::ReadyToPublish;
        save(paths, receipt)?;
    }
    let staging = staging_path(paths, receipt)?;
    let destination = paths.state(&receipt.target.selector).database;
    let current = current_destination(paths, receipt)?;
    if Some(&current) != receipt.upgraded_store.as_ref() {
        if current != receipt.original_destination {
            return Err(blocked(
                "publication outcome conflicts with journal; retain state for recovery",
            ));
        }
        if Some(inspect(&receipt.target, &staging)?) != receipt.upgraded_store {
            return Err(blocked("staging validation failed before publication"));
        }
        // Checkpoint-free snapshotting does not justify deleting a destination WAL.
        // A pristine destination with sidecars must be explicitly checkpointed by its owner.
        for suffix in ["-wal", "-shm", "-journal"] {
            let sidecar = PathBuf::from(format!("{}{suffix}", destination.display()));
            if sidecar.exists() {
                return Err(blocked(
                    "destination has SQLite sidecars; checkpoint/close its daemon before retrying",
                ));
            }
        }
        atomic_replace(&staging, &destination)?;
    }
    receipt.phase = UpgradePhase::Prepared;
    save(paths, receipt)?;
    Ok(receipt.clone())
}
fn staging_path(paths: &ForgeupPaths, receipt: &UpgradeReceipt) -> Result<PathBuf, ForgeupError> {
    let name = receipt
        .staging_file
        .as_ref()
        .ok_or_else(|| blocked("missing staging identity"))?;
    let path = artifacts(paths, receipt).join(name);
    contained(paths, &path)?;
    Ok(path)
}
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), ForgeupError> {
    // std::fs::rename atomically replaces files on supported Unix platforms.
    // On Windows replacement may fail; that leaves the old destination intact.
    fs::rename(source, destination).map_err(|e| io_error(destination, e))?;
    sync_directory(
        destination
            .parent()
            .ok_or_else(|| blocked("missing destination parent"))?,
    )
}

/// Inspect durable progress without starting either daemon.
pub fn status(to: &str) -> Result<UpgradeReceipt, ForgeupError> {
    let paths = ForgeupPaths::from_environment()?;
    let store =
        RegistryStore::open_read_only(paths.clone())?.ok_or_else(|| blocked("no registry"))?;
    let item = resolve_in_registry(&store.load_read_only()?, to)?;
    load(&paths, &item.selector)?.ok_or_else(|| blocked("no upgrade receipt for target"))
}

/// Explicit activation, separate from preparation and daemon startup.
pub fn activate(
    to: &str,
    accept_stale_source: bool,
    confirm_agents_stopped: bool,
) -> Result<UpgradeReceipt, ForgeupError> {
    activate_at(
        &ForgeupPaths::from_environment()?,
        to,
        accept_stale_source,
        confirm_agents_stopped,
    )
}
fn activate_at(
    paths: &ForgeupPaths,
    to: &str,
    accept_stale: bool,
    confirmed: bool,
) -> Result<UpgradeReceipt, ForgeupError> {
    if !confirmed {
        return Err(blocked("activation requires --confirm-agents-stopped"));
    }
    let store = RegistryStore::open(paths.clone())?;
    let mut registry = store.load()?;
    let target = resolve_in_registry(&registry, to)?;
    let mut receipt =
        load(paths, &target.selector)?.ok_or_else(|| blocked("prepare an upgrade first"))?;
    let source = resolve_in_registry(&registry, &receipt.source.selector)?;
    if source != receipt.source || target != receipt.target {
        return Err(blocked("upgrade installations changed"));
    }
    validate_installation(&source)?;
    handshake(&target)?;
    let _locks = OwnershipLocks::acquire(paths, &source, &target)?;
    if !matches!(
        receipt.phase,
        UpgradePhase::Prepared | UpgradePhase::Activating | UpgradePhase::Activated
    ) {
        return Err(blocked("complete preparation or recover before activation"));
    }
    if receipt.phase != UpgradePhase::Activated {
        let source_state = current_source(paths, &receipt)?;
        if !source_state.blockers.is_empty() {
            return Err(blocked(source_state.blockers.join("; ")));
        }
        if source_state.fingerprint != receipt.source_store.fingerprint && !accept_stale {
            return Err(blocked(
                "source changed after preparation; prepare afresh or explicitly --accept-stale-source",
            ));
        }
        if Some(current_destination(paths, &receipt)?) != receipt.upgraded_store {
            return Err(blocked("prepared target changed"));
        }
        receipt.accepted_stale_source =
            source_state.fingerprint != receipt.source_store.fingerprint;
        receipt.phase = UpgradePhase::Activating;
        save(paths, &receipt)?;
    }
    // The persisted Activating phase blocks both daemon starts until selection
    // and the receipt are reconciled. Retrying this operation is safe.
    registry.active_selector = Some(target.selector.clone());
    store.save(&registry)?;
    reconcile_active_path(paths, &registry)?;
    receipt.phase = UpgradePhase::Activated;
    save(paths, &receipt)?;
    Ok(receipt)
}

/// Recover publication if verifiable, or abandon only an unpublished attempt.
/// All snapshots and diagnostics are retained; no user database is deleted.
pub fn recover(to: &str) -> Result<String, ForgeupError> {
    recover_at(&ForgeupPaths::from_environment()?, to)
}
fn recover_at(paths: &ForgeupPaths, to: &str) -> Result<String, ForgeupError> {
    let store = RegistryStore::open(paths.clone())?;
    let registry = store.load()?;
    let target = resolve_in_registry(&registry, to)?;
    let mut receipt =
        load(paths, &target.selector)?.ok_or_else(|| blocked("no upgrade to recover"))?;
    let source = resolve_in_registry(&registry, &receipt.source.selector)?;
    if receipt.source != source || receipt.target != target {
        return Err(blocked("upgrade installations changed"));
    }
    let _locks = OwnershipLocks::acquire(paths, &source, &target)?;
    if receipt.phase == UpgradePhase::Discarding {
        return finish_discard(paths, &receipt);
    }
    if receipt.phase == UpgradePhase::ReadyToPublish
        && Some(current_destination(paths, &receipt)?) == receipt.upgraded_store
    {
        receipt.phase = UpgradePhase::Prepared;
        save(paths, &receipt)?;
        return Ok("publication recovered; explicit activation still required".into());
    }
    if !matches!(
        receipt.phase,
        UpgradePhase::Preparing | UpgradePhase::ReadyToPublish | UpgradePhase::Prepared
    ) {
        return Err(blocked("activation began; retry upgrade activate instead"));
    }
    if current_destination(paths, &receipt)? != receipt.original_destination {
        return Err(blocked(
            "destination was published or changed; do not discard its provenance",
        ));
    }
    let archive = artifacts(paths, &receipt).join("abandoned.json");
    contained(paths, &archive)?;
    atomic_replace(&receipt_path(paths, &target.selector), &archive)?;
    sync_directory(
        receipt_path(paths, &target.selector)
            .parent()
            .ok_or_else(|| blocked("missing receipt parent"))?,
    )?;
    Ok(
        "unpublished attempt abandoned; snapshots and staging retained in upgrades directory"
            .into(),
    )
}

/// Archive an unactivated prepared copy so changed source data can be prepared afresh.
/// The target must still match the exact validated publication; user writes are never reset.
pub fn discard(to: &str) -> Result<String, ForgeupError> {
    discard_at(&ForgeupPaths::from_environment()?, to)
}
fn discard_at(paths: &ForgeupPaths, to: &str) -> Result<String, ForgeupError> {
    let store = RegistryStore::open(paths.clone())?;
    let registry = store.load()?;
    let target = resolve_in_registry(&registry, to)?;
    let mut receipt =
        load(paths, &target.selector)?.ok_or_else(|| blocked("no prepared upgrade"))?;
    let source = resolve_in_registry(&registry, &receipt.source.selector)?;
    if receipt.source != source || receipt.target != target {
        return Err(blocked("upgrade installations changed"));
    }
    let _locks = OwnershipLocks::acquire(paths, &source, &target)?;
    if receipt.phase == UpgradePhase::Prepared {
        if Some(current_destination(paths, &receipt)?) != receipt.upgraded_store {
            return Err(blocked("target changed; refusing to discard user state"));
        }
        receipt.phase = UpgradePhase::Discarding;
        save(paths, &receipt)?;
    }
    if receipt.phase != UpgradePhase::Discarding {
        return Err(blocked(
            "only an unactivated prepared upgrade can be discarded",
        ));
    }
    finish_discard(paths, &receipt)
}
fn finish_discard(paths: &ForgeupPaths, receipt: &UpgradeReceipt) -> Result<String, ForgeupError> {
    let destination = paths.state(&receipt.target.selector).database;
    let directory = artifacts(paths, receipt);
    let archive = directory.join("prepared.db");
    let reset = directory.join("reset.db");
    for path in [&archive, &reset] {
        contained(paths, path)?;
    }
    let current = current_destination(paths, receipt)?;
    if Some(&current) == receipt.upgraded_store.as_ref() {
        // Interrupted archive copying is safe to retry while the unchanged
        // publication still exists. The destination remains present at all times.
        fs::copy(&destination, &archive).map_err(|e| io_error(&archive, e))?;
        set_private_permissions(&archive)?;
        sync_file(&archive)?;
    } else if !current.pristine || current.schema.is_some() {
        return Err(blocked("discard outcome is ambiguous; retain all evidence"));
    }
    if Some(inspect(&receipt.target, &archive)?) != receipt.upgraded_store {
        return Err(blocked("prepared archive validation failed"));
    }
    fs::write(&reset, []).map_err(|e| io_error(&reset, e))?;
    set_private_permissions(&reset)?;
    sync_file(&reset)?;
    atomic_replace(&reset, &destination)?;
    let abandoned = directory.join("abandoned.json");
    atomic_replace(&receipt_path(paths, &receipt.target.selector), &abandoned)?;
    sync_directory(
        destination
            .parent()
            .ok_or_else(|| blocked("missing state parent"))?,
    )?;
    Ok("unactivated copy archived; destination reset to empty; prepare again from the current source".into())
}

/// Explicitly remove backup/staging files only after successful activation.
/// The receipt and divergence guard remain durable.
pub fn cleanup(to: &str) -> Result<(), ForgeupError> {
    let paths = ForgeupPaths::from_environment()?;
    let store = RegistryStore::open(paths.clone())?;
    let registry = store.load()?;
    let target = resolve_in_registry(&registry, to)?;
    let receipt = load(&paths, &target.selector)?.ok_or_else(|| blocked("no upgrade receipt"))?;
    if receipt.phase != UpgradePhase::Activated {
        return Err(blocked("cleanup requires a completed activation"));
    }
    let mut names = vec!["source.db".to_owned()];
    if let Some(name) = &receipt.staging_file {
        names.push(name.clone());
    }
    for name in names {
        let path = artifacts(&paths, &receipt).join(name);
        contained(&paths, &path)?;
        remove_owned(&path)?;
    }
    sync_directory(&artifacts(&paths, &receipt))
}

/// Called under the registry lock before every managed start or selection.
pub(super) fn check_start(paths: &ForgeupPaths, selector: &str) -> Result<(), ForgeupError> {
    for receipt in receipts(paths)? {
        if receipt.target.selector == selector && receipt.phase != UpgradePhase::Activated {
            return Err(blocked(
                "target upgrade is not activated; use upgrade status/recover/activate",
            ));
        }
        if receipt.source.selector == selector {
            match receipt.phase {
                UpgradePhase::Preparing
                | UpgradePhase::ReadyToPublish
                | UpgradePhase::Activating
                | UpgradePhase::Discarding => {
                    return Err(blocked("source is reserved by an incomplete upgrade"));
                }
                UpgradePhase::Activated if !receipt.source_restart_acknowledged => {
                    return Err(blocked(
                        "source was carried forward; use forgeup use --acknowledge-divergence for explicit rollback/fork; newer work will not be reconciled",
                    ));
                }
                UpgradePhase::Prepared | UpgradePhase::Activated => {}
            }
        }
    }
    Ok(())
}
pub(super) fn acknowledge(paths: &ForgeupPaths, selector: &str) -> Result<(), ForgeupError> {
    for mut receipt in receipts(paths)? {
        if receipt.source.selector == selector && receipt.phase == UpgradePhase::Activated {
            receipt.source_restart_acknowledged = true;
            save(paths, &receipt)?;
        }
    }
    Ok(())
}
pub(super) fn check_mutation(paths: &ForgeupPaths, selector: &str) -> Result<(), ForgeupError> {
    if receipts(paths)?
        .iter()
        .any(|r| r.source.selector == selector || r.target.selector == selector)
    {
        return Err(blocked(
            "revision participates in a retained upgrade; replacement/removal would invalidate recovery provenance",
        ));
    }
    Ok(())
}
