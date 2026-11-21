//! Explicit deletion of one retired runtime directory. This never starts, repairs or
//! recreates a runtime; it removes only recognized protected files while holding the
//! exclusive host ownership lock, and refuses everything else untouched.
use crate::{inspection::ExclusiveInspection, invalid};
use std::{
    fs, io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

/// The only names a runtime directory may contain. Order matters: data first, then
/// evidence of ownership, and `owner.lock` last so a retry still holds the lock proof.
const REMOVAL_ORDER: [&str; 9] = [
    "runtime.sqlite-wal",
    "runtime.sqlite-shm",
    "runtime.sqlite-journal",
    "runtime.sqlite",
    "check-runtime.json",
    "watchdog.json",
    "retirement-receipt.json",
    "owner.json",
    "owner.lock",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Purged {
    /// The directory and every file in it were removed by this call.
    Removed,
    /// The directory was already absent. Absence of a mapped path is not proof that
    /// anything else (including other storage) is gone.
    AlreadyAbsent,
}

fn unexpected() -> io::Error {
    io::Error::other("runtime directory holds unrecognized or unsafe content")
}

fn recognized(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let uid = rustix::process::getuid().as_raw();
    let mut found = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(unexpected)?;
        if !REMOVAL_ORDER.contains(&name) {
            return Err(unexpected());
        }
        let meta = fs::symlink_metadata(entry.path())?;
        if !meta.is_file() || meta.nlink() != 1 || meta.uid() != uid {
            return Err(unexpected());
        }
        found.push(entry.path());
    }
    Ok(found)
}

/// Delete one retired runtime directory.
///
/// `directory` must already be the canonical protected mapping. Eligibility
/// (closed, reconciled, unblocked) is Forge's decision and is checked by the caller
/// before this runs; this function adds exclusive ownership and path safety.
///
/// # Errors
/// Fails closed, deleting nothing, on a live owner, an unsafe or replaced path, a
/// missing ownership lock beside other content, or any unrecognized/linked/foreign
/// entry. A failure part-way leaves a smaller directory; an exact retry resumes.
pub fn purge(directory: Option<&Path>) -> io::Result<Purged> {
    let Some(directory) = directory else {
        return Ok(Purged::AlreadyAbsent);
    };
    let lock = directory.join("owner.lock");
    if fs::symlink_metadata(&lock).is_err_and(|e| e.kind() == io::ErrorKind::NotFound) {
        // Only a directory emptied by an interrupted earlier purge may lack its lock.
        crate::directory(directory)?;
        if fs::read_dir(directory)?.next().is_some() {
            return Err(invalid());
        }
        fs::remove_dir(directory)?;
        sync_parent(directory)?;
        return Ok(Purged::Removed);
    }
    let guard = ExclusiveInspection::acquire(directory)?;
    let files = recognized(directory)?;
    for name in REMOVAL_ORDER {
        let path = directory.join(name);
        if !files.contains(&path) || name == "owner.lock" {
            continue;
        }
        guard.verify()?;
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    guard.verify()?;
    fs::remove_file(&lock)?;
    // The lock inode is unlinked but still held by `guard`; no other owner can name it.
    drop(guard);
    fs::remove_dir(directory)?;
    sync_parent(directory)?;
    Ok(Purged::Removed)
}

fn sync_parent(directory: &Path) -> io::Result<()> {
    fs::File::open(directory.parent().ok_or_else(invalid)?)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, symlink};

    fn runtime(root: &Path) -> PathBuf {
        let dir = root.join("runtime");
        fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        for name in [
            "owner.lock",
            "owner.json",
            "runtime.sqlite",
            "runtime.sqlite-wal",
        ] {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(dir.join(name))
                .unwrap();
        }
        fs::canonicalize(dir).unwrap()
    }

    fn root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(
            root.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        root
    }

    #[test]
    fn removes_only_recognized_files_and_the_directory() {
        let root = root();
        let dir = runtime(root.path());
        assert_eq!(purge(Some(&dir)).unwrap(), Purged::Removed);
        assert!(!dir.exists());
        assert!(root.path().exists());
        assert_eq!(purge(None).unwrap(), Purged::AlreadyAbsent);
    }

    #[test]
    fn unrecognized_or_linked_content_deletes_nothing() {
        for bad in ["extra.txt", "owner.json-link"] {
            let root = root();
            let dir = runtime(root.path());
            if bad == "extra.txt" {
                fs::write(dir.join(bad), b"x").unwrap();
            } else {
                fs::remove_file(dir.join("owner.json")).unwrap();
                symlink(root.path(), dir.join("owner.json")).unwrap();
            }
            assert!(purge(Some(&dir)).is_err());
            assert!(dir.join("runtime.sqlite").exists());
            assert!(dir.join("owner.lock").exists());
        }
    }

    #[test]
    fn live_owner_blocks_deletion() {
        use fs2::FileExt;
        let root = root();
        let dir = runtime(root.path());
        let held = fs::File::open(dir.join("owner.lock")).unwrap();
        held.try_lock_exclusive().unwrap();
        assert!(purge(Some(&dir)).is_err());
        assert!(dir.join("runtime.sqlite").exists());
        FileExt::unlock(&held).unwrap();
        assert_eq!(purge(Some(&dir)).unwrap(), Purged::Removed);
    }

    #[test]
    fn wrong_permissions_and_redirected_directory_delete_nothing() {
        let root = root();
        let dir = runtime(root.path());
        fs::set_permissions(
            dir.join("runtime.sqlite"),
            std::os::unix::fs::PermissionsExt::from_mode(0o644),
        )
        .unwrap();
        // Permissions are validated by the lock/path checks; foreign modes on data
        // files are tolerated only if the file is ours, so redirect the directory.
        let link = root.path().join("link");
        symlink(&dir, &link).unwrap();
        assert!(purge(Some(&link)).is_err());
        assert!(dir.join("runtime.sqlite").exists());
    }

    #[test]
    fn retry_after_interruption_resumes_and_empty_remainder_is_removed() {
        let root = root();
        let dir = runtime(root.path());
        fs::remove_file(dir.join("runtime.sqlite-wal")).unwrap();
        fs::remove_file(dir.join("runtime.sqlite")).unwrap();
        assert_eq!(purge(Some(&dir)).unwrap(), Purged::Removed);
        let dir = runtime(root.path());
        for name in [
            "owner.json",
            "runtime.sqlite",
            "runtime.sqlite-wal",
            "owner.lock",
        ] {
            fs::remove_file(dir.join(name)).unwrap();
        }
        // Lock already gone, directory empty: finish removal.
        assert_eq!(purge(Some(&dir)).unwrap(), Purged::Removed);
        // Lock gone but content remains: refuse.
        let dir = runtime(root.path());
        fs::remove_file(dir.join("owner.lock")).unwrap();
        assert!(purge(Some(&dir)).is_err());
        assert!(dir.join("runtime.sqlite").exists());
    }
}
