//! Read-only retained input fingerprint. No temporary index, object writes or file copies.
use forge_app::{RegistrationError, retained_work::RetainedState};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path},
    process::Command,
};

fn unknown() -> RegistrationError {
    RegistrationError::new(
        "recovery-state-unknown",
        "Cannot safely observe retained state (unreadable files, links, submodules or invalid Git state); preserve evidence and inspect before recovery",
    )
}
fn git_bytes(path: &Path, args: &[&str]) -> Result<Vec<u8>, RegistrationError> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|_| unknown())?;
    if !output.status.success() {
        return Err(unknown());
    }
    Ok(output.stdout)
}
fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update(bytes.len().to_le_bytes());
    hash.update(bytes);
}

pub(super) fn observe(path: &Path) -> Result<RetainedState, RegistrationError> {
    let mut hash = Sha256::new();
    field(&mut hash, b"forge-retained-state-v1");
    field(&mut hash, &git_bytes(path, &["rev-parse", "HEAD"])?);
    // Includes staged blobs, modes, merge stages and index flags, distinct from working bytes.
    field(
        &mut hash,
        &git_bytes(path, &["ls-files", "--stage", "-v", "-z"])?,
    );
    let files = git_bytes(path, &["ls-files", "--cached", "--others", "-z"])?;
    let files = files
        .split(|b| *b == 0)
        .filter(|b| !b.is_empty())
        .collect::<BTreeSet<_>>();
    for name in files {
        let relative = Path::new(std::str::from_utf8(name).map_err(|_| unknown())?);
        let mut full = path.to_path_buf();
        for component in relative.components() {
            let Component::Normal(component) = component else {
                return Err(unknown());
            };
            full.push(component);
            match fs::symlink_metadata(&full) {
                Ok(metadata) if metadata.file_type().is_symlink() => return Err(unknown()),
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => return Err(unknown()),
            }
        }
        field(&mut hash, name);
        match fs::symlink_metadata(&full) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => field(&mut hash, b"missing"),
            Err(_) => return Err(unknown()),
            Ok(metadata) => {
                if !metadata.is_file() {
                    return Err(unknown());
                }
                field(&mut hash, b"file");
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    field(&mut hash, &metadata.permissions().mode().to_le_bytes());
                }
                let mut file = fs::File::open(&full).map_err(|_| unknown())?;
                let mut content = Sha256::new();
                let mut buffer = [0_u8; 8192];
                loop {
                    let count = file.read(&mut buffer).map_err(|_| unknown())?;
                    if count == 0 {
                        break;
                    }
                    content.update(&buffer[..count]);
                }
                field(&mut hash, &content.finalize());
            }
        }
    }
    Ok(RetainedState {
        sha256: format!("{:x}", hash.finalize()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observation_is_read_only_and_binds_index_working_bytes_and_file_names() {
        let root = std::env::temp_dir().join(format!(
            "forge-retained-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        super::super::git(&root, &["init"]).unwrap();
        fs::write(root.join("tracked"), b"original").unwrap();
        super::super::git(&root, &["add", "tracked"]).unwrap();
        super::super::git(
            &root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                "base",
            ],
        )
        .unwrap();
        let base = observe(&root).unwrap();
        fs::write(root.join("tracked"), b"staged").unwrap();
        super::super::git(&root, &["add", "tracked"]).unwrap();
        fs::write(root.join("tracked"), b"original").unwrap();
        let staged = observe(&root).unwrap();
        assert_ne!(staged, base, "working bytes alone miss staged-only changes");
        let index = fs::read(root.join(".git/index")).unwrap();
        fs::write(root.join("new file\nwith newline"), b"untracked\0\xff").unwrap();
        let untracked = observe(&root).unwrap();
        assert_ne!(untracked, staged);
        assert_eq!(observe(&root).unwrap(), untracked);
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
        assert_eq!(fs::read(root.join("tracked")).unwrap(), b"original");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/passwd", root.join("link")).unwrap();
            assert_eq!(observe(&root).unwrap_err().code, "recovery-state-unknown");
            fs::remove_file(root.join("link")).unwrap();
            fs::remove_file(root.join("tracked")).unwrap();
            std::os::unix::fs::symlink("/etc/passwd", root.join("tracked")).unwrap();
            assert!(observe(&root).is_err());
        }
        assert!(observe(&root.join("missing")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
