use super::ExclusiveInspection;
use fs2::FileExt;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "forge-exclusive-inspection-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn lock(&self) {
        fs::write(self.0.join("owner.lock"), b"retained-lock-bytes").unwrap();
        fs::set_permissions(self.0.join("owner.lock"), fs::Permissions::from_mode(0o600)).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn exclusive_inspection_requires_existing_evidence_and_releases_only_on_drop() {
    let f = Fixture::new();
    assert!(ExclusiveInspection::acquire(&f.0).is_err());
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 0);
    f.lock();
    let guard = ExclusiveInspection::acquire(&f.0).unwrap();
    guard.verify().unwrap();
    assert!(ExclusiveInspection::acquire(&f.0).is_err());
    let contender = fs::File::open(f.0.join("owner.lock")).unwrap();
    assert!(contender.try_lock_exclusive().is_err());
    assert_eq!(
        fs::read(f.0.join("owner.lock")).unwrap(),
        b"retained-lock-bytes"
    );
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
    drop(guard);
    contender.try_lock_exclusive().unwrap();
}

#[test]
fn exclusive_inspection_rejects_unsafe_paths_without_repair() {
    let f = Fixture::new();
    f.lock();
    let path = f.0.join("owner.lock");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(ExclusiveInspection::acquire(&f.0).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let linked = f.0.join("linked");
    fs::hard_link(&path, &linked).unwrap();
    assert!(ExclusiveInspection::acquire(&f.0).is_err());
    fs::remove_file(&linked).unwrap();
    fs::rename(&path, &linked).unwrap();
    symlink(&linked, &path).unwrap();
    assert!(ExclusiveInspection::acquire(&f.0).is_err());
    fs::remove_file(&path).unwrap();
    fs::rename(&linked, &path).unwrap();
    symlink(&f.0, &linked).unwrap();
    assert!(ExclusiveInspection::acquire(&linked).is_err());
    fs::set_permissions(&f.0, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(ExclusiveInspection::acquire(&f.0).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"retained-lock-bytes");
}

#[test]
fn exclusive_inspection_detects_replaced_lock_and_keeps_original_locked() {
    let f = Fixture::new();
    f.lock();
    let guard = ExclusiveInspection::acquire(&f.0).unwrap();
    let old = f.0.join("old.lock");
    fs::rename(f.0.join("owner.lock"), &old).unwrap();
    f.lock();
    assert!(guard.verify().is_err());
    let old = fs::File::open(old).unwrap();
    assert!(old.try_lock_exclusive().is_err());
    drop(guard);
    old.try_lock_exclusive().unwrap();
}

#[test]
fn exclusive_inspection_detects_replaced_directory_and_permission_changes() {
    let f = Fixture::new();
    f.lock();
    let guard = ExclusiveInspection::acquire(&f.0).unwrap();
    fs::set_permissions(f.0.join("owner.lock"), fs::Permissions::from_mode(0o640)).unwrap();
    assert!(guard.verify().is_err());
    fs::set_permissions(f.0.join("owner.lock"), fs::Permissions::from_mode(0o600)).unwrap();
    guard.verify().unwrap();
    let moved = f.0.with_extension("moved");
    fs::rename(&f.0, &moved).unwrap();
    fs::create_dir(&f.0).unwrap();
    fs::set_permissions(&f.0, fs::Permissions::from_mode(0o700)).unwrap();
    f.lock();
    assert!(guard.verify().is_err());
    drop(guard);
    fs::remove_dir_all(moved).unwrap();
}
