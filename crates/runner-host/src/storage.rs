//! Protected deterministic storage placement. No runtime DB open or deletion.
use crate::directory;
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
};

fn denied() -> io::Error {
    io::Error::other("unverified runtime storage boundary")
}

fn ancestor(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.uid() != rustix::process::getuid().as_raw()
        || meta.mode() & 0o022 != 0
        || fs::canonicalize(path)? != path
    {
        return Err(denied());
    }
    Ok(())
}

/// Map an immutable store digest under the registered canonical Workspace, never
/// under a worktree or caller-selected cleanup path. Opaque IDs must be hashed by
/// the trusted server rather than interpreted as filesystem components.
/// # Errors
/// Rejects redirected, foreign-owned, writable-by-others or unprotected paths.
/// Creating missing directories does not adopt existing runtime history.
pub fn runtime_directory(root: &Path, store_digest: &str, create: bool) -> io::Result<PathBuf> {
    if store_digest.len() != 64
        || !store_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(denied());
    }
    ancestor(root)?;
    let mut path = root.to_path_buf();
    for (index, component) in [".forge", "assignment-runtimes", store_digest]
        .iter()
        .enumerate()
    {
        path.push(component);
        // A no-follow precheck also distinguishes a dangling symlink from an
        // absent path. Concurrent trusted creators may race; revalidate after.
        match fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound && create => {
                match fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => {
                        fs::File::open(path.parent().ok_or_else(denied)?)?.sync_all()?;
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
        if index == 0 {
            ancestor(&path)?;
        } else {
            directory(&path)?;
        }
    }
    Ok(path)
}

/// Locate an EXISTING runtime directory without creating anything. `None` means the
/// mapped path is absent; redirected or unsafe paths remain errors.
/// # Errors
/// Rejects redirected, foreign-owned or unprotected paths.
pub fn existing_runtime_directory(root: &Path, store_digest: &str) -> io::Result<Option<PathBuf>> {
    match runtime_directory(root, store_digest, false) {
        Ok(path) => Ok(Some(path)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
