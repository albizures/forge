//! Proof that no host ever owned a runtime directory, for a durable launch that
//! failed before or at its spawn (T26).
//!
//! A host takes `owner.lock` and then records `owner.json` before it starts its
//! worker. A directory whose lock is free (or absent) and that has no owner
//! record and no pending owner write was therefore never owned. Sealing keeps
//! that true for good: it takes the exclusive lock (creating the lock file only
//! when it is absent), re-checks under it, and writes a protected
//! `launch-sealed.json`. A host refuses a sealed directory before it starts any
//! worker. Sealing signals, adopts and repairs nothing.
use crate::{directory, inspection::ExclusiveInspection, invalid};
use std::{fs, io, path::Path};

/// The tombstone a host refuses to start under.
pub const SEALED: &str = "launch-sealed.json";

fn owner_evidence(directory: &Path) -> io::Result<bool> {
    for entry in fs::read_dir(directory)? {
        let name = entry?.file_name();
        let name = name.to_str().ok_or_else(invalid)?;
        if name == "owner.json" || (name.starts_with("owner.") && name.ends_with(".pending")) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Seal a never-owned runtime directory and return the exclusive inspection
/// guard. Keep it alive through the dependent Forge commit.
/// # Errors
/// An unsafe directory, a held lock (`WouldBlock`), owner evidence, or a
/// tombstone that cannot be written. None of these is proof.
pub fn seal(path: &Path) -> io::Result<ExclusiveInspection> {
    directory(path)?;
    // Create the lock only when it is absent; never replace or repair one.
    match rustix::fs::open(
        path.join("owner.lock"),
        rustix::fs::OFlags::CREATE
            | rustix::fs::OFlags::EXCL
            | rustix::fs::OFlags::RDWR
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    ) {
        Ok(fd) => drop(fs::File::from(fd)),
        Err(rustix::io::Errno::EXIST) => {}
        Err(error) => return Err(error.into()),
    }
    let guard = ExclusiveInspection::acquire(path)?;
    if owner_evidence(path)? {
        return Err(invalid());
    }
    crate::retirement::persist(
        path,
        SEALED,
        &serde_json::json!({ "version": 1, "never_owned": true }),
    )?;
    guard.verify()?;
    if owner_evidence(path)? {
        return Err(invalid());
    }
    Ok(guard)
}

/// Whether the directory carries the never-owned tombstone.
/// # Errors
/// Unreadable directory metadata is uncertainty.
pub fn sealed(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path.join(SEALED)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs2::FileExt;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn runtime() -> (tempfile::TempDir, std::path::PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let path = fs::canonicalize(root.path()).unwrap().join("runtime");
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        (root, path)
    }

    #[test]
    fn seals_a_never_owned_directory_idempotently() {
        let (_root, path) = runtime();
        drop(seal(&path).unwrap());
        assert!(sealed(&path).unwrap());
        // An exact retry is the same result; the lock file is never replaced.
        let inode = fs::metadata(path.join("owner.lock")).unwrap();
        drop(seal(&path).unwrap());
        assert_eq!(
            fs::metadata(path.join("owner.lock")).unwrap().ino(),
            inode.ino()
        );
    }

    #[test]
    fn refuses_owner_evidence_or_a_held_lock() {
        let (_root, path) = runtime();
        fs::write(path.join("owner.7.pending"), b"{}").unwrap();
        assert!(seal(&path).is_err());
        assert!(!sealed(&path).unwrap());

        let (_root, path) = runtime();
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.join("owner.lock"))
            .unwrap();
        fs::set_permissions(path.join("owner.lock"), fs::Permissions::from_mode(0o600)).unwrap();
        lock.try_lock_exclusive().unwrap();
        assert!(seal(&path).is_err());
        assert!(!sealed(&path).unwrap());
    }
}
