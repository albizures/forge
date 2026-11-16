//! Orchestration tests use a deterministic offline process adapter, not a daemon.
use super::*;
use crate::{RegistrationOptions, register_at, use_revision_at};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture {
    root: PathBuf,
    paths: ForgeupPaths,
    source: Installation,
    target: Installation,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "forgeup-upgrade-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let paths = ForgeupPaths {
            config: root.join("config"),
            data: root.join("data"),
            runtime: root.join("runtime"),
            bin: root.join("bin"),
        };
        let executable =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/offline-upgrade");
        let source = register_at(
            &paths,
            "old",
            &executable,
            &executable,
            Some("old"),
            RegistrationOptions::default(),
        )
        .unwrap();
        let target = register_at(
            &paths,
            "new",
            &executable,
            &executable,
            Some("new"),
            RegistrationOptions::default(),
        )
        .unwrap();
        fs::write(paths.state(&source.selector).database, "7history").unwrap();
        Self {
            root,
            paths,
            source,
            target,
        }
    }
    fn prepare(&self) -> UpgradeReceipt {
        prepare_at(&self.paths, "old", "new", true).unwrap()
    }
    fn load(&self) -> UpgradeReceipt {
        load(&self.paths, &self.target.selector).unwrap().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn plan_is_read_only_and_prepare_preserves_source_without_activation() {
    let f = Fixture::new();
    let registry_before = fs::read(f.paths.registry()).unwrap();
    let plan = plan_at(&f.paths, "old", "new").unwrap();
    assert!(plan.blockers.is_empty());
    assert_eq!(plan.migrations, [(7, 8)]);
    assert_eq!(fs::read(f.paths.registry()).unwrap(), registry_before);
    assert!(!receipt_path(&f.paths, &f.target.selector).exists());
    assert!(prepare_at(&f.paths, "old", "new", false).is_err());
    let receipt = f.prepare();
    assert_eq!(receipt.phase, UpgradePhase::Prepared);
    assert_eq!(
        fs::read_to_string(f.paths.state(&f.source.selector).database).unwrap(),
        "7history"
    );
    assert_eq!(
        fs::read_to_string(f.paths.state(&f.target.selector).database).unwrap(),
        "8history"
    );
    assert_eq!(
        fs::read_to_string(artifacts(&f.paths, &receipt).join("source.db")).unwrap(),
        "7history"
    );
    let registry = RegistryStore::open(f.paths.clone())
        .unwrap()
        .load()
        .unwrap();
    assert!(registry.active_selector.is_none());
    assert_eq!(
        registry.schema_version, 2,
        "older managers must not bypass lineage guards"
    );
    assert!(use_revision_at(&f.paths, "new").is_err());
    assert!(check_start(&f.paths, &f.target.selector).is_err());
    assert_eq!(
        f.prepare().id,
        receipt.id,
        "exact retry must retain original receipt"
    );
}

#[test]
fn activation_checks_freshness_and_requires_explicit_rollback_acknowledgement() {
    let f = Fixture::new();
    f.prepare();
    fs::write(f.paths.state(&f.source.selector).database, "7later").unwrap();
    assert!(activate_at(&f.paths, "new", false, true).is_err());
    assert!(activate_at(&f.paths, "new", true, false).is_err());
    let receipt = activate_at(&f.paths, "new", true, true).unwrap();
    assert!(receipt.accepted_stale_source);
    assert_eq!(receipt.phase, UpgradePhase::Activated);
    assert!(check_start(&f.paths, &f.source.selector).is_err());
    assert!(use_revision_at(&f.paths, "old").is_err());
    assert!(check_start(&f.paths, &f.target.selector).is_ok());
    crate::select_revision_at(&f.paths, "old", true).unwrap();
    assert!(check_start(&f.paths, &f.source.selector).is_ok());
    assert!(f.load().source_restart_acknowledged);
    assert_eq!(
        fs::read_to_string(f.paths.state(&f.source.selector).database).unwrap(),
        "7later"
    );
    assert_eq!(
        fs::read_to_string(f.paths.state(&f.target.selector).database).unwrap(),
        "8history"
    );
    assert!(check_mutation(&f.paths, &f.source.selector).is_err());
    assert!(check_mutation(&f.paths, &f.target.selector).is_err());
}

#[test]
fn populated_destinations_conflicting_retries_and_concurrent_owners_fail_closed() {
    let f = Fixture::new();
    fs::write(f.paths.state(&f.target.selector).database, "8valuable").unwrap();
    assert!(!plan_at(&f.paths, "old", "new").unwrap().blockers.is_empty());
    assert!(prepare_at(&f.paths, "old", "new", true).is_err());
    assert_eq!(
        fs::read_to_string(f.paths.state(&f.target.selector).database).unwrap(),
        "8valuable"
    );
    fs::write(f.paths.state(&f.target.selector).database, "").unwrap();
    let locks = OwnershipLocks::acquire(&f.paths, &f.source, &f.target).unwrap();
    assert!(prepare_at(&f.paths, "old", "new", true).is_err());
    drop(locks);
    let receipt = f.prepare();
    let mut conflicting = receipt.clone();
    conflicting.target.daemon_sha256 = "changed".into();
    save(&f.paths, &conflicting).unwrap();
    assert!(prepare_at(&f.paths, "old", "new", true).is_err());
    assert!(activate_at(&f.paths, "new", false, true).is_err());
}

#[test]
fn interrupted_publication_and_activation_recover_without_replaying_migrations() {
    let f = Fixture::new();
    let mut receipt = f.prepare();
    // Simulate a crash after atomic publication but before journal advancement.
    receipt.phase = UpgradePhase::ReadyToPublish;
    save(&f.paths, &receipt).unwrap();
    assert!(check_start(&f.paths, &f.source.selector).is_err());
    assert!(
        recover_at(&f.paths, "new")
            .unwrap()
            .contains("publication recovered")
    );
    assert_eq!(f.load().phase, UpgradePhase::Prepared);
    receipt.phase = UpgradePhase::Activating;
    save(&f.paths, &receipt).unwrap();
    assert!(check_start(&f.paths, &f.source.selector).is_err());
    assert!(check_start(&f.paths, &f.target.selector).is_err());
    assert!(recover_at(&f.paths, "new").is_err());
    activate_at(&f.paths, "new", false, true).unwrap();
    assert_eq!(f.load().phase, UpgradePhase::Activated);
    assert_eq!(
        fs::read_to_string(f.paths.state(&f.target.selector).database).unwrap(),
        "8history"
    );
}

#[test]
fn incomplete_unpublished_attempt_is_abandoned_without_deleting_artifacts() {
    let f = Fixture::new();
    let mut receipt = f.prepare();
    // Reconstruct an interrupted pre-publication attempt with a partial staging file.
    fs::write(f.paths.state(&f.target.selector).database, "").unwrap();
    receipt.phase = UpgradePhase::Preparing;
    save(&f.paths, &receipt).unwrap();
    let stage = artifacts(&f.paths, &receipt).join(receipt.staging_file.as_ref().unwrap());
    fs::write(&stage, "partial").unwrap();
    fs::write(f.paths.state(&f.source.selector).database, "7newer").unwrap();
    assert!(prepare_at(&f.paths, "old", "new", true).is_err());
    assert!(recover_at(&f.paths, "new").unwrap().contains("abandoned"));
    assert!(stage.exists());
    assert!(
        artifacts(&f.paths, &receipt)
            .join("abandoned.json")
            .exists()
    );
    assert!(check_start(&f.paths, &f.source.selector).is_ok());
    let next = f.prepare();
    assert_ne!(next.id, receipt.id);
}

#[test]
fn ready_publication_retries_and_prepared_discard_preserve_recovery_evidence() {
    let f = Fixture::new();
    let mut receipt = f.prepare();
    let directory = artifacts(&f.paths, &receipt);
    let staging = directory.join(receipt.staging_file.as_ref().unwrap());
    let destination = f.paths.state(&f.target.selector).database;
    fs::copy(&destination, &staging).unwrap();
    fs::write(&destination, "").unwrap();
    receipt.phase = UpgradePhase::ReadyToPublish;
    save(&f.paths, &receipt).unwrap();
    assert_eq!(f.prepare().phase, UpgradePhase::Prepared);
    assert!(discard_at(&f.paths, "new").unwrap().contains("archived"));
    assert_eq!(
        fs::read_to_string(directory.join("prepared.db")).unwrap(),
        "8history"
    );
    assert!(directory.join("abandoned.json").exists());
    assert_eq!(fs::read_to_string(destination).unwrap(), "");
    assert!(load(&f.paths, &f.target.selector).unwrap().is_none());
    fs::write(f.paths.state(&f.source.selector).database, "7more-work").unwrap();
    let next = f.prepare();
    assert_ne!(next.id, receipt.id);
    assert_eq!(next.source_store.fingerprint, "7more-work");
}

#[test]
fn changed_publication_is_not_discarded_and_corrupt_backup_requires_recovery() {
    let f = Fixture::new();
    let mut receipt = f.prepare();
    let destination = f.paths.state(&f.target.selector).database;
    fs::write(&destination, "8user-work").unwrap();
    assert!(discard_at(&f.paths, "new").is_err());
    assert_eq!(fs::read_to_string(&destination).unwrap(), "8user-work");
    fs::write(&destination, "").unwrap();
    receipt.phase = UpgradePhase::Preparing;
    save(&f.paths, &receipt).unwrap();
    fs::write(artifacts(&f.paths, &receipt).join("source.db"), "7partial").unwrap();
    assert!(prepare_at(&f.paths, "old", "new", true).is_err());
    recover_at(&f.paths, "new").unwrap();
    assert!(artifacts(&f.paths, &receipt).join("source.db").exists());
    assert_eq!(f.prepare().phase, UpgradePhase::Prepared);
}

#[test]
fn preparation_retry_never_reuses_a_surviving_workers_staging_path() {
    let f = Fixture::new();
    let mut receipt = f.prepare();
    let old_stage = staging_path(&f.paths, &receipt).unwrap();
    fs::write(&old_stage, "7slow-worker").unwrap();
    fs::write(f.paths.state(&f.target.selector).database, "").unwrap();
    receipt.phase = UpgradePhase::Preparing;
    save(&f.paths, &receipt).unwrap();
    let retried = f.prepare();
    assert_ne!(retried.staging_file, receipt.staging_file);
    assert_eq!(fs::read_to_string(&old_stage).unwrap(), "7slow-worker");
    // A worker orphaned by a killed manager finishes late, but only touches
    // its own retained inode/path, never the publication chosen by the retry.
    fs::write(&old_stage, "8late-result").unwrap();
    assert_eq!(
        fs::read_to_string(f.paths.state(&f.target.selector).database).unwrap(),
        "8history"
    );
}

#[test]
fn interrupted_discard_recovers_without_losing_the_prepared_archive() {
    let f = Fixture::new();
    let mut receipt = f.prepare();
    let directory = artifacts(&f.paths, &receipt);
    let destination = f.paths.state(&f.target.selector).database;
    fs::copy(&destination, directory.join("prepared.db")).unwrap();
    fs::write(&destination, "").unwrap();
    receipt.phase = UpgradePhase::Discarding;
    save(&f.paths, &receipt).unwrap();
    assert!(check_start(&f.paths, &f.source.selector).is_err());
    assert!(recover_at(&f.paths, "new").unwrap().contains("archived"));
    assert_eq!(
        fs::read_to_string(directory.join("prepared.db")).unwrap(),
        "8history"
    );
    assert!(check_start(&f.paths, &f.source.selector).is_ok());
}

#[test]
fn symlinked_database_or_artifact_parent_is_never_followed() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let source = f.paths.state(&f.source.selector).database;
    let real = f.root.join("outside.db");
    fs::rename(&source, &real).unwrap();
    symlink(&real, &source).unwrap();
    assert!(plan_at(&f.paths, "old", "new").is_err());
    assert!(prepare_at(&f.paths, "old", "new", true).is_err());
    fs::remove_file(&source).unwrap();
    fs::rename(&real, &source).unwrap();
    let outside = f.root.join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(
        &outside,
        f.paths
            .state(&f.target.selector)
            .database
            .parent()
            .unwrap()
            .join("upgrades"),
    )
    .unwrap();
    assert!(prepare_at(&f.paths, "old", "new", true).is_err());
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}
