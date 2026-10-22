//! Forgeup's private registry and immutable Forge Revision installations.

#![allow(clippy::missing_errors_doc)]

use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsString,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const REGISTRY_SCHEMA_VERSION: u32 = 1;
const SUPPORTED_MANAGEMENT_PROTOCOL_REVISION: u32 = 1;
const REGISTRY_FILE: &str = "registry.toml";
const LOCK_FILE: &str = "registry.lock";
const STATE_LOG_LIMIT: u64 = 64 * 1024;
const STATE_LOG_RETAINED_FILES: usize = 3;
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// All user-owned locations used by Forgeup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForgeupPaths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub runtime: PathBuf,
    pub bin: PathBuf,
}

impl ForgeupPaths {
    /// Resolves the Forgeup layout from `FORGEUP_HOME` or platform conventions.
    pub fn from_environment() -> Result<Self, ForgeupError> {
        if let Some(home) = env::var_os("FORGEUP_HOME") {
            let home = PathBuf::from(home);
            if home.as_os_str().is_empty() {
                return Err(ForgeupError::InvalidLocation("FORGEUP_HOME is empty"));
            }
            return Ok(Self {
                config: home.join("config"),
                data: home.join("data"),
                runtime: home.join("runtime"),
                bin: home.join("bin"),
            });
        }
        let home = home_dir()?;
        #[cfg(target_os = "windows")]
        {
            let roaming = variable_path("APPDATA")?;
            let local = variable_path("LOCALAPPDATA")?;
            return Ok(Self {
                config: roaming.join("Forgeup"),
                data: local.join("Forgeup"),
                runtime: local.join("Forgeup/runtime"),
                bin: local.join("Forgeup/bin"),
            });
        }
        #[cfg(target_os = "macos")]
        {
            return Ok(Self {
                config: home.join("Library/Application Support/Forgeup"),
                data: home.join("Library/Application Support/Forgeup"),
                runtime: home.join("Library/Caches/Forgeup/runtime"),
                bin: home.join("Library/Application Support/Forgeup/bin"),
            });
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let config = env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
            let data = env_path("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
            let runtime =
                env_path("XDG_RUNTIME_DIR").unwrap_or_else(|| data.join("forgeup/runtime"));
            Ok(Self {
                config: config.join("forgeup"),
                data: data.join("forgeup"),
                runtime: runtime.join("forgeup"),
                bin: data.join("forgeup/bin"),
            })
        }
    }

    pub fn ensure_private(&self) -> Result<(), ForgeupError> {
        for path in [&self.config, &self.data, &self.runtime, &self.bin] {
            fs::create_dir_all(path).map_err(|source| ForgeupError::Io {
                path: path.clone(),
                source,
            })?;
            set_private_permissions(path)?;
            verify_private_permissions(path)?;
        }
        Ok(())
    }
    #[must_use]
    pub fn registry(&self) -> PathBuf {
        self.config.join(REGISTRY_FILE)
    }
    #[must_use]
    pub fn installations(&self) -> PathBuf {
        self.data.join("installations")
    }
    #[must_use]
    pub fn state(&self, selector: &str) -> RevisionState {
        RevisionState {
            database: self.data.join("state").join(selector).join("forge.db"),
            runtime: self.runtime.join("revisions").join(selector),
            logs: self.data.join("state").join(selector).join("logs"),
        }
    }
    fn lock(&self) -> PathBuf {
        self.config.join(LOCK_FILE)
    }
}

/// Immutable facts reported by a Forge executable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionIdentity {
    pub commit: String,
    pub dirty: bool,
    pub management_protocol_revision: u32,
    pub version: String,
}

/// One immutable snapshot in the registry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installation {
    pub source_label: String,
    pub selector: String,
    pub local_build_alias: Option<String>,
    pub identity: RevisionIdentity,
    pub cli_path: PathBuf,
    pub daemon_path: PathBuf,
    pub cli_sha256: String,
    pub daemon_sha256: String,
    pub endpoint: String,
}

/// Isolated persistent locations for one Forge Revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionState {
    pub database: PathBuf,
    pub runtime: PathBuf,
    pub logs: PathBuf,
}

/// A strictly validated persisted Forgeup registry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub schema_version: u32,
    #[serde(default)]
    pub installations: Vec<Installation>,
    /// Kept here for lifecycle operations; activation is managed by a later Forgeup command.
    #[serde(default)]
    pub active_selector: Option<String>,
}
impl Registry {
    fn empty() -> Self {
        Self {
            schema_version: REGISTRY_SCHEMA_VERSION,
            installations: Vec::new(),
            active_selector: None,
        }
    }
}

/// Holds the cross-process registry lock while reading or writing the registry.
pub struct RegistryStore {
    paths: ForgeupPaths,
    _lock: File,
}
impl RegistryStore {
    pub fn open(paths: ForgeupPaths) -> Result<Self, ForgeupError> {
        paths.ensure_private()?;
        let lock_path = paths.lock();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| ForgeupError::Io {
                path: lock_path.clone(),
                source,
            })?;
        set_private_permissions(&lock_path)?;
        verify_private_permissions(&lock_path)?;
        lock.lock_exclusive().map_err(|source| ForgeupError::Io {
            path: lock_path,
            source,
        })?;
        Ok(Self { paths, _lock: lock })
    }
    pub fn load(&self) -> Result<Registry, ForgeupError> {
        let path = self.paths.registry();
        if !path.exists() {
            return Ok(Registry::empty());
        }
        verify_private_permissions(&path)?;
        let contents = fs::read_to_string(&path).map_err(|source| ForgeupError::Io {
            path: path.clone(),
            source,
        })?;
        let registry = toml::from_str::<Registry>(&contents).map_err(|source| {
            ForgeupError::InvalidRegistry {
                path: path.clone(),
                message: source.to_string(),
            }
        })?;
        match registry.schema_version {
            REGISTRY_SCHEMA_VERSION => Ok(registry),
            version if version > REGISTRY_SCHEMA_VERSION => {
                Err(ForgeupError::FutureSchema(version))
            }
            version => self.migrate(version, &contents),
        }
    }
    pub fn save(&self, registry: &Registry) -> Result<(), ForgeupError> {
        if registry.schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(ForgeupError::UnsupportedSchema(registry.schema_version));
        }
        let contents =
            toml::to_string_pretty(registry).map_err(|source| ForgeupError::InvalidRegistry {
                path: self.paths.registry(),
                message: source.to_string(),
            })?;
        atomic_write(&self.paths.registry(), contents.as_bytes())
    }
    fn migrate(&self, version: u32, old: &str) -> Result<Registry, ForgeupError> {
        if version != 0 {
            return Err(ForgeupError::UnsupportedSchema(version));
        }
        atomic_write(
            &self.paths.registry().with_extension("toml.bak"),
            old.as_bytes(),
        )?;
        let registry = Registry::empty();
        self.save(&registry)?;
        Ok(registry)
    }
}

/// Options which make registration's destructive or state-sharing behavior explicit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegistrationOptions<'a> {
    pub replace: Option<&'a str>,
    pub reuse_state: Option<&'a str>,
    pub purge_state: bool,
}

/// Registers a verified executable pair as an immutable Revision Installation.
pub fn register(
    source_label: &str,
    cli: &Path,
    daemon: &Path,
    local_build_alias: Option<&str>,
) -> Result<Installation, ForgeupError> {
    register_with_options(
        source_label,
        cli,
        daemon,
        local_build_alias,
        RegistrationOptions::default(),
    )
}

/// Registers a Revision Installation with explicit replacement and state options.
pub fn register_with_options(
    source_label: &str,
    cli: &Path,
    daemon: &Path,
    local_build_alias: Option<&str>,
    options: RegistrationOptions<'_>,
) -> Result<Installation, ForgeupError> {
    register_at(
        &ForgeupPaths::from_environment()?,
        source_label,
        cli,
        daemon,
        local_build_alias,
        options,
    )
}

fn register_at(
    paths: &ForgeupPaths,
    source_label: &str,
    cli: &Path,
    daemon: &Path,
    local_build_alias: Option<&str>,
    options: RegistrationOptions<'_>,
) -> Result<Installation, ForgeupError> {
    let (cli_identity, selector, cli_hash, daemon_hash) =
        attest_registration(source_label, cli, daemon, local_build_alias)?;
    let store = RegistryStore::open(paths.clone())?;
    let mut registry = store.load()?;
    let existing_index = registry
        .installations
        .iter()
        .position(|item| item.selector == selector);
    let replacing = options.replace.is_some();
    if let Some(replace) = options.replace {
        let target = resolve_in_registry(&registry, replace)?;
        if target.selector != selector {
            return Err(ForgeupError::ReplacementSelectorMismatch {
                replace: replace.to_owned(),
                candidate: selector,
            });
        }
    }
    if options.purge_state && !replacing {
        return Err(ForgeupError::PurgeRequiresReplacement);
    }
    if let Some(reuse) = options.reuse_state
        && reuse != selector
    {
        return Err(ForgeupError::StateReuseMismatch {
            requested: reuse.to_owned(),
            selector,
        });
    }
    if let Some(index) = existing_index {
        let existing = &registry.installations[index];
        if existing.cli_sha256 == cli_hash && existing.daemon_sha256 == daemon_hash && !replacing {
            return Ok(existing.clone());
        }
        if !replacing {
            return Err(ForgeupError::RegistrationConflict(selector));
        }
    } else if replacing {
        return Err(ForgeupError::UnknownSelector(
            options.replace.unwrap_or_default().to_owned(),
        ));
    }
    if let Some(alias) = local_build_alias
        && registry
            .installations
            .iter()
            .enumerate()
            .any(|(index, item)| {
                Some(index) != existing_index && item.local_build_alias.as_deref() == Some(alias)
            })
    {
        return Err(ForgeupError::AliasConflict(alias.to_owned()));
    }
    let state = paths.state(&selector);
    if options.purge_state {
        purge_state(&state)?;
    }
    if options.reuse_state.is_some() && !state.database.parent().is_some_and(Path::exists) {
        return Err(ForgeupError::StateUnavailable(selector));
    }
    ensure_state(&state)?;
    let installation = snapshot_installation(
        paths,
        source_label,
        selector.clone(),
        local_build_alias,
        cli_identity,
        (cli_hash, daemon_hash),
        (cli, daemon),
    )?;
    append_diagnostic(&state, "registered Revision Installation")?;
    if let Some(index) = existing_index {
        registry.installations[index] = installation.clone();
    } else {
        registry.installations.push(installation.clone());
    }
    store.save(&registry)?;
    Ok(installation)
}

fn snapshot_installation(
    paths: &ForgeupPaths,
    source_label: &str,
    selector: String,
    local_build_alias: Option<&str>,
    identity: RevisionIdentity,
    hashes: (String, String),
    executables: (&Path, &Path),
) -> Result<Installation, ForgeupError> {
    let (cli_sha256, daemon_sha256) = hashes;
    let (cli, daemon) = executables;
    let destination = paths.installations().join(&selector);
    if destination.exists() {
        fs::remove_dir_all(&destination).map_err(|source| ForgeupError::Io {
            path: destination.clone(),
            source,
        })?;
    }
    fs::create_dir_all(&destination).map_err(|source| ForgeupError::Io {
        path: destination.clone(),
        source,
    })?;
    set_private_permissions(&destination)?;
    let cli_path = destination.join("forge");
    let daemon_path = destination.join("forge-server");
    copy_executable(cli, &cli_path)?;
    copy_executable(daemon, &daemon_path)?;
    let installation = Installation {
        source_label: source_label.to_owned(),
        selector,
        local_build_alias: local_build_alias.map(str::to_owned),
        identity,
        cli_path,
        daemon_path,
        cli_sha256,
        daemon_sha256,
        endpoint: allocate_endpoint()?,
    };
    let manifest =
        toml::to_string_pretty(&installation).map_err(|source| ForgeupError::InvalidRegistry {
            path: destination.join("manifest.toml"),
            message: source.to_string(),
        })?;
    atomic_write(&destination.join("manifest.toml"), manifest.as_bytes())?;
    Ok(installation)
}

/// Returns registered Revision Installations.
pub fn list() -> Result<Vec<Installation>, ForgeupError> {
    let paths = ForgeupPaths::from_environment()?;
    Ok(RegistryStore::open(paths)?.load()?.installations)
}
/// Resolves the only accepted canonical selector or a unique local-build alias.
pub fn current(selector: &str) -> Result<Installation, ForgeupError> {
    let paths = ForgeupPaths::from_environment()?;
    let registry = RegistryStore::open(paths)?.load()?;
    resolve_in_registry(&registry, selector)
}

/// Removes an Installation but preserves its Revision State unless explicitly purged.
pub fn remove(selector: &str, purge: bool) -> Result<(), ForgeupError> {
    let paths = ForgeupPaths::from_environment()?;
    remove_at(&paths, selector, purge)
}

fn remove_at(paths: &ForgeupPaths, selector: &str, purge: bool) -> Result<(), ForgeupError> {
    let store = RegistryStore::open(paths.clone())?;
    let mut registry = store.load()?;
    let installation = resolve_in_registry(&registry, selector)?;
    if registry.active_selector.as_deref() == Some(&installation.selector) {
        return Err(ForgeupError::ActiveRemoval(installation.selector));
    }
    let destination = paths.installations().join(&installation.selector);
    if destination.exists() {
        fs::remove_dir_all(&destination).map_err(|source| ForgeupError::Io {
            path: destination,
            source,
        })?;
    }
    let state = paths.state(&installation.selector);
    if purge {
        purge_state(&state)?;
    } else {
        append_diagnostic(&state, "removed Revision Installation; state retained")?;
    }
    registry
        .installations
        .retain(|item| item.selector != installation.selector);
    store.save(&registry)
}

fn resolve_in_registry(registry: &Registry, selector: &str) -> Result<Installation, ForgeupError> {
    let matches = registry
        .installations
        .iter()
        .filter(|item| {
            item.selector == selector || item.local_build_alias.as_deref() == Some(selector)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [item] => Ok((*item).clone()),
        [] => Err(ForgeupError::UnknownSelector(selector.to_owned())),
        _ => Err(ForgeupError::AmbiguousSelector(selector.to_owned())),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DoctorReport {
    pub paths: ForgeupPaths,
    pub schema_version: u32,
}
pub fn doctor() -> Result<DoctorReport, ForgeupError> {
    let paths = ForgeupPaths::from_environment()?;
    let registry = RegistryStore::open(paths.clone())?.load()?;
    Ok(DoctorReport {
        paths,
        schema_version: registry.schema_version,
    })
}

#[derive(Debug)]
pub enum ForgeupError {
    InvalidLocation(&'static str),
    Io { path: PathBuf, source: io::Error },
    UnsafePermissions(PathBuf),
    InvalidRegistry { path: PathBuf, message: String },
    FutureSchema(u32),
    UnsupportedSchema(u32),
    InvalidLabel(String),
    InvalidIdentity(String),
    UnsupportedManagementProtocol(u32),
    IdentityMismatch,
    CommandFailed(PathBuf, String),
    RegistrationConflict(String),
    AliasConflict(String),
    UnknownSelector(String),
    AmbiguousSelector(String),
    ReplacementSelectorMismatch { replace: String, candidate: String },
    PurgeRequiresReplacement,
    StateReuseMismatch { requested: String, selector: String },
    StateUnavailable(String),
    ActiveRemoval(String),
}
impl fmt::Display for ForgeupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLocation(m) => write!(f, "invalid Forgeup location: {m}"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::UnsafePermissions(path) => {
                write!(f, "{} must be private to its owner", path.display())
            }
            Self::InvalidRegistry { path, message } => {
                write!(f, "invalid registry {}: {message}", path.display())
            }
            Self::FutureSchema(v) => write!(f, "registry schema {v} is newer than this Forgeup"),
            Self::UnsupportedSchema(v) => write!(f, "registry schema {v} is unsupported"),
            Self::InvalidLabel(v) => write!(f, "invalid source label or alias `{v}`"),
            Self::InvalidIdentity(v) => write!(f, "invalid Revision Identity: {v}"),
            Self::UnsupportedManagementProtocol(v) => {
                write!(f, "Management Protocol revision {v} is unsupported")
            }
            Self::IdentityMismatch => write!(f, "CLI and daemon Revision Identities do not match"),
            Self::CommandFailed(path, message) => {
                write!(f, "could not inspect {}: {message}", path.display())
            }
            Self::RegistrationConflict(s) => write!(
                f,
                "Revision Installation `{s}` already has different artifacts"
            ),
            Self::AliasConflict(a) => write!(f, "local-build alias `{a}` is already in use"),
            Self::UnknownSelector(s) => write!(f, "unknown Revision Selector `{s}`"),
            Self::AmbiguousSelector(s) => write!(f, "ambiguous Revision Selector `{s}`"),
            Self::ReplacementSelectorMismatch { replace, candidate } => write!(
                f,
                "replacement `{replace}` does not identify candidate Revision `{candidate}`"
            ),
            Self::PurgeRequiresReplacement => write!(f, "--purge-state requires --replace"),
            Self::StateReuseMismatch {
                requested,
                selector,
            } => write!(
                f,
                "state `{requested}` cannot be reused by Revision `{selector}`"
            ),
            Self::StateUnavailable(selector) => write!(
                f,
                "Revision State for `{selector}` is not available for reuse"
            ),
            Self::ActiveRemoval(selector) => {
                write!(f, "cannot remove active Forge Revision `{selector}`")
            }
        }
    }
}
impl std::error::Error for ForgeupError {}

fn attest_registration(
    source_label: &str,
    cli: &Path,
    daemon: &Path,
    local_build_alias: Option<&str>,
) -> Result<(RevisionIdentity, String, String, String), ForgeupError> {
    validate_label(source_label)?;
    if let Some(alias) = local_build_alias {
        validate_label(alias)?;
    }
    let cli_identity = inspect_identity(cli)?;
    if cli_identity != inspect_identity(daemon)? {
        return Err(ForgeupError::IdentityMismatch);
    }
    validate_identity(&cli_identity)?;
    Ok((
        cli_identity.clone(),
        format!("{source_label}@{}", cli_identity.commit),
        sha256(cli)?,
        sha256(daemon)?,
    ))
}

fn inspect_identity(path: &Path) -> Result<RevisionIdentity, ForgeupError> {
    let output = Command::new(path)
        .args(["--build-info", "--json"])
        .output()
        .map_err(|source| ForgeupError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if !output.status.success() {
        return Err(ForgeupError::CommandFailed(
            path.to_path_buf(),
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| ForgeupError::CommandFailed(path.to_path_buf(), error.to_string()))
}
fn validate_identity(identity: &RevisionIdentity) -> Result<(), ForgeupError> {
    if identity.commit.len() != 40 || !identity.commit.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ForgeupError::InvalidIdentity(
            "commit must be a full 40-character hexadecimal Git ID".into(),
        ));
    }
    if identity.version.trim().is_empty() {
        return Err(ForgeupError::InvalidIdentity("version is blank".into()));
    }
    if identity.management_protocol_revision != SUPPORTED_MANAGEMENT_PROTOCOL_REVISION {
        return Err(ForgeupError::UnsupportedManagementProtocol(
            identity.management_protocol_revision,
        ));
    }
    Ok(())
}
fn validate_label(label: &str) -> Result<(), ForgeupError> {
    if label.is_empty()
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        Err(ForgeupError::InvalidLabel(label.to_owned()))
    } else {
        Ok(())
    }
}
fn ensure_state(state: &RevisionState) -> Result<(), ForgeupError> {
    for path in [
        state.database.parent().expect("database has parent"),
        &state.runtime,
        &state.logs,
    ] {
        fs::create_dir_all(path).map_err(|source| ForgeupError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        set_private_permissions(path)?;
    }
    if !state.database.exists() {
        atomic_write(&state.database, b"")?;
    }
    Ok(())
}

fn purge_state(state: &RevisionState) -> Result<(), ForgeupError> {
    let root = state.database.parent().expect("database has parent");
    if root.exists() {
        fs::remove_dir_all(root).map_err(|source| ForgeupError::Io {
            path: root.to_path_buf(),
            source,
        })?;
    }
    if state.runtime.exists() {
        fs::remove_dir_all(&state.runtime).map_err(|source| ForgeupError::Io {
            path: state.runtime.clone(),
            source,
        })?;
    }
    Ok(())
}

fn append_diagnostic(state: &RevisionState, message: &str) -> Result<(), ForgeupError> {
    fs::create_dir_all(&state.logs).map_err(|source| ForgeupError::Io {
        path: state.logs.clone(),
        source,
    })?;
    let log = state.logs.join("forgeup.log");
    let line = format!("{message}\n");
    let size = fs::metadata(&log).map_or(0, |metadata| metadata.len());
    if size + u64::try_from(line.len()).expect("line length fits u64") > STATE_LOG_LIMIT {
        let oldest = state
            .logs
            .join(format!("forgeup.log.{}", STATE_LOG_RETAINED_FILES - 1));
        if oldest.exists() {
            fs::remove_file(&oldest).map_err(|source| ForgeupError::Io {
                path: oldest,
                source,
            })?;
        }
        for number in (1..STATE_LOG_RETAINED_FILES - 1).rev() {
            let from = state.logs.join(format!("forgeup.log.{number}"));
            let to = state.logs.join(format!("forgeup.log.{}", number + 1));
            if from.exists() {
                fs::rename(&from, &to).map_err(|source| ForgeupError::Io { path: to, source })?;
            }
        }
        if log.exists() {
            fs::rename(&log, state.logs.join("forgeup.log.1")).map_err(|source| {
                ForgeupError::Io {
                    path: log.clone(),
                    source,
                }
            })?;
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map_err(|source| ForgeupError::Io {
            path: log.clone(),
            source,
        })?;
    set_private_permissions(&log)?;
    file.write_all(line.as_bytes())
        .map_err(|source| ForgeupError::Io { path: log, source })
}

fn sha256(path: &Path) -> Result<String, ForgeupError> {
    let mut file = File::open(path).map_err(|source| ForgeupError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let count = file.read(&mut buffer).map_err(|source| ForgeupError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn copy_executable(source: &Path, destination: &Path) -> Result<(), ForgeupError> {
    fs::copy(source, destination).map_err(|source_error| ForgeupError::Io {
        path: destination.to_path_buf(),
        source: source_error,
    })?;
    set_private_permissions(destination)
}
fn allocate_endpoint() -> Result<String, ForgeupError> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|source| ForgeupError::Io {
        path: PathBuf::from("loopback endpoint"),
        source,
    })?;
    let endpoint = listener
        .local_addr()
        .map_err(|source| ForgeupError::Io {
            path: PathBuf::from("loopback endpoint"),
            source,
        })?
        .to_string();
    Ok(endpoint)
}
fn home_dir() -> Result<PathBuf, ForgeupError> {
    env_path("HOME").ok_or(ForgeupError::InvalidLocation("HOME is not set"))
}
#[cfg(target_os = "windows")]
fn variable_path(name: &'static str) -> Result<PathBuf, ForgeupError> {
    env_path(name).ok_or(ForgeupError::InvalidLocation(name))
}
fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), ForgeupError> {
    let parent = path
        .parent()
        .ok_or(ForgeupError::InvalidLocation("registry has no parent"))?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        file_name(path).to_string_lossy(),
        std::process::id(),
        TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| ForgeupError::Io {
            path: temporary.clone(),
            source,
        })?;
    set_private_permissions(&temporary)?;
    file.write_all(contents)
        .map_err(|source| ForgeupError::Io {
            path: temporary.clone(),
            source,
        })?;
    file.sync_all().map_err(|source| ForgeupError::Io {
        path: temporary.clone(),
        source,
    })?;
    fs::rename(&temporary, path).map_err(|source| ForgeupError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| ForgeupError::Io {
            path: parent.to_path_buf(),
            source,
        })
}
fn file_name(path: &Path) -> OsString {
    path.file_name().unwrap_or_default().to_os_string()
}
#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<(), ForgeupError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        ForgeupError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}
#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<(), ForgeupError> {
    Ok(())
}
#[cfg(unix)]
fn verify_private_permissions(path: &Path) -> Result<(), ForgeupError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .map_err(|source| ForgeupError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        Err(ForgeupError::UnsafePermissions(path.to_path_buf()))
    } else {
        Ok(())
    }
}
#[cfg(not(unix))]
fn verify_private_permissions(_path: &Path) -> Result<(), ForgeupError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    fn executable(path: &Path, identity: &str, payload: &str) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(
            path,
            format!("#!/bin/sh\nif [ \"$1\" = \"--build-info\" ]; then echo '{identity}'; exit 0; fi\necho {payload}\n"),
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn registration_snapshots_matching_artifacts_and_resolves_aliases() {
        let root = env::temp_dir().join(format!("forgeup-registration-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let cli = root.join("candidate-forge");
        let daemon = root.join("candidate-forge-server");
        fs::create_dir_all(&root).unwrap();
        let identity = r#"{"commit":"0123456789012345678901234567890123456789","dirty":false,"management_protocol_revision":1,"version":"test"}"#;
        executable(&cli, identity, "original-cli");
        executable(&daemon, identity, "original-daemon");
        let paths = ForgeupPaths {
            config: root.join("home/config"),
            data: root.join("home/data"),
            runtime: root.join("home/runtime"),
            bin: root.join("home/bin"),
        };
        let installation = register_at(
            &paths,
            "local",
            &cli,
            &daemon,
            Some("dev"),
            RegistrationOptions::default(),
        )
        .unwrap();
        assert!(installation.cli_path.exists());
        let registry = RegistryStore::open(paths.clone()).unwrap().load().unwrap();
        assert_eq!(registry.installations, vec![installation.clone()]);
        assert_eq!(
            registry.installations[0].local_build_alias.as_deref(),
            Some("dev")
        );
        executable(&cli, identity, "changed-cli");
        assert!(matches!(
            register_at(
                &paths,
                "local",
                &cli,
                &daemon,
                Some("dev"),
                RegistrationOptions::default(),
            ),
            Err(ForgeupError::RegistrationConflict(_))
        ));
        assert_ne!(
            fs::read(&installation.cli_path).unwrap(),
            fs::read(&cli).unwrap()
        );
        let mismatched = r#"{"commit":"0123456789012345678901234567890123456789","dirty":false,"management_protocol_revision":1,"version":"other"}"#;
        executable(&cli, mismatched, "mismatched-cli");
        assert!(matches!(
            register_at(
                &paths,
                "other",
                &cli,
                &daemon,
                None,
                RegistrationOptions::default(),
            ),
            Err(ForgeupError::IdentityMismatch)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replacement_and_removal_make_state_lifecycle_explicit() {
        let root = env::temp_dir().join(format!("forgeup-state-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let paths = ForgeupPaths {
            config: root.join("config"),
            data: root.join("data"),
            runtime: root.join("runtime"),
            bin: root.join("bin"),
        };
        let cli = root.join("forge");
        let daemon = root.join("forge-server");
        let identity = r#"{"commit":"0123456789012345678901234567890123456789","dirty":false,"management_protocol_revision":1,"version":"test"}"#;
        executable(&cli, identity, "first");
        executable(&daemon, identity, "first");
        let installation = register_at(
            &paths,
            "local",
            &cli,
            &daemon,
            None,
            RegistrationOptions::default(),
        )
        .unwrap();
        let state = paths.state(&installation.selector);
        assert!(state.database.exists());
        assert!(state.runtime.exists());
        assert!(state.logs.exists());
        fs::write(&state.database, "preserved").unwrap();
        executable(&cli, identity, "replacement");
        let replacement = register_at(
            &paths,
            "local",
            &cli,
            &daemon,
            None,
            RegistrationOptions {
                replace: Some(&installation.selector),
                ..RegistrationOptions::default()
            },
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&state.database).unwrap(), "preserved");
        remove_at(&paths, &replacement.selector, false).unwrap();
        assert!(state.database.exists());
        assert!(matches!(
            register_at(
                &paths,
                "local",
                &cli,
                &daemon,
                None,
                RegistrationOptions {
                    reuse_state: Some(&replacement.selector),
                    ..RegistrationOptions::default()
                }
            ),
            Ok(_)
        ));
        {
            let store = RegistryStore::open(paths.clone()).unwrap();
            let mut registry = store.load().unwrap();
            registry.active_selector = Some(replacement.selector.clone());
            store.save(&registry).unwrap();
        }
        assert!(matches!(
            remove_at(&paths, &replacement.selector, false),
            Err(ForgeupError::ActiveRemoval(_))
        ));
        {
            let store = RegistryStore::open(paths.clone()).unwrap();
            let mut registry = store.load().unwrap();
            registry.active_selector = None;
            store.save(&registry).unwrap();
        }
        remove_at(&paths, &replacement.selector, true).unwrap();
        assert!(!state.database.parent().unwrap().exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn diagnostic_logs_are_bounded() {
        let root = env::temp_dir().join(format!("forgeup-logs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let state = RevisionState {
            database: root.join("state/forge.db"),
            runtime: root.join("runtime"),
            logs: root.join("state/logs"),
        };
        let message = "x".repeat(usize::try_from(STATE_LOG_LIMIT).unwrap());
        for _ in 0..5 {
            append_diagnostic(&state, &message).unwrap();
        }
        let logs = fs::read_dir(&state.logs).unwrap().count();
        assert_eq!(logs, STATE_LOG_RETAINED_FILES);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_incomplete_and_unsupported_identities() {
        let mut identity = RevisionIdentity {
            commit: "bad".into(),
            dirty: false,
            management_protocol_revision: 1,
            version: "test".into(),
        };
        assert!(matches!(
            validate_identity(&identity),
            Err(ForgeupError::InvalidIdentity(_))
        ));
        identity.commit = "0123456789012345678901234567890123456789".into();
        identity.management_protocol_revision = 2;
        assert!(matches!(
            validate_identity(&identity),
            Err(ForgeupError::UnsupportedManagementProtocol(2))
        ));
    }
}
