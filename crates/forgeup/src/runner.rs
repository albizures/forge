//! Matched durable-runner bundle attestation for a Forge Revision Installation.
//!
//! A bundle is Revision-relative: the pinned Node executable, the runner sources and
//! the locked JavaScript dependency closure are copied into the immutable
//! Installation and attested by a content tree hash. Nothing here consults `PATH`,
//! global npm state or the checkout, and nothing here authorizes execution: it only
//! proves which exact components belong to which Revision.

use super::{ForgeupError, RevisionIdentity, offline_output, sha256};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Bundle manifest file name at the bundle root.
pub const BUNDLE_MANIFEST: &str = "forge-runner-bundle.json";
/// Forge-owned runner wire protocol implemented by this Forgeup.
pub const SUPPORTED_RUNNER_PROTOCOL: u32 = 1;
/// Fixed, not manifest-selected, component locations inside a bundle.
pub const NODE_RELATIVE: &str = "node/node";
/// The shipped production Check runner entry (`runCheckRunner`, no substitution).
/// The synthetic T04 runner is a test fixture under `runner/test/`, never bundled.
pub const ENTRY_RELATIVE: &str = "runner/src/check-entry.ts";
pub const INSPECTOR_RELATIVE: &str = "runner/src/inspect-effects.ts";
pub const LOCK_RELATIVE: &str = "runner/package-lock.json";
const SUPPORTED_SCHEMA: u32 = 1;
const MAX_FILES: usize = 200_000;

/// Facts declared by the packager and independently recomputed by Forgeup.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    pub schema: u32,
    pub runner_protocol: u32,
    pub node_version: String,
    pub forge_commit: String,
    pub package_lock_sha256: String,
    pub tree_sha256: String,
}

/// The attested components of one Installation, recorded in its manifest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledRunner {
    pub root: PathBuf,
    pub node_path: PathBuf,
    pub entry_path: PathBuf,
    pub inspector_path: PathBuf,
    pub protocol: u32,
    pub node_version: String,
    pub package_lock_sha256: String,
    pub tree_sha256: String,
}

fn invalid(message: impl Into<String>) -> ForgeupError {
    ForgeupError::RunnerBundle(message.into())
}

fn io(path: &Path, source: std::io::Error) -> ForgeupError {
    ForgeupError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Deterministic content identity: sorted relative path, executable bit and file
/// hash. Symlinks, special files and the manifest itself never participate.
/// # Errors
/// Rejects any symlink/special file, unreadable entry or implausibly large tree.
pub fn tree_hash(root: &Path) -> Result<String, ForgeupError> {
    let mut entries = Vec::new();
    collect(root, root, &mut entries)?;
    entries.sort();
    let mut hash = Sha256::new();
    for relative in &entries {
        let path = root.join(relative);
        let metadata = fs::symlink_metadata(&path).map_err(|e| io(&path, e))?;
        #[cfg(unix)]
        let executable = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let executable = false;
        hash.update(relative.as_bytes());
        hash.update([0, u8::from(executable), 0]);
        hash.update(sha256(&path)?.as_bytes());
        hash.update(b"\n");
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn collect(root: &Path, directory: &Path, out: &mut Vec<String>) -> Result<(), ForgeupError> {
    for entry in fs::read_dir(directory).map_err(|e| io(directory, e))? {
        let entry = entry.map_err(|e| io(directory, e))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| io(&path, e))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| invalid("entry escaped the bundle root"))?
            .to_str()
            .ok_or_else(|| invalid("bundle path is not UTF-8"))?
            .to_owned();
        if relative == BUNDLE_MANIFEST {
            continue;
        }
        if metadata.is_dir() {
            collect(root, &path, out)?;
        } else if metadata.is_file() {
            out.push(relative);
            if out.len() > MAX_FILES {
                return Err(invalid("bundle has too many files"));
            }
        } else {
            return Err(invalid(format!(
                "bundle contains a symlink or special file at `{relative}`"
            )));
        }
    }
    Ok(())
}

fn read_manifest(root: &Path) -> Result<BundleManifest, ForgeupError> {
    let path = root.join(BUNDLE_MANIFEST);
    let bytes = fs::read(&path).map_err(|e| io(&path, e))?;
    let manifest: BundleManifest = serde_json::from_slice(&bytes)
        .map_err(|e| invalid(format!("malformed {BUNDLE_MANIFEST}: {e}")))?;
    if manifest.schema != SUPPORTED_SCHEMA {
        return Err(invalid(format!(
            "bundle schema {} is unsupported",
            manifest.schema
        )));
    }
    if manifest.runner_protocol != SUPPORTED_RUNNER_PROTOCOL {
        return Err(invalid(format!(
            "runner protocol {} is incompatible with this Forgeup (requires {SUPPORTED_RUNNER_PROTOCOL})",
            manifest.runner_protocol
        )));
    }
    if !hex(&manifest.tree_sha256) || !hex(&manifest.package_lock_sha256) {
        return Err(invalid("bundle hashes must be lowercase SHA-256"));
    }
    if manifest.node_version.is_empty()
        || !manifest
            .node_version
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.')
    {
        return Err(invalid("bundle Node version is malformed"));
    }
    Ok(manifest)
}

/// Verifies a bundle directory (source or installed) against its own manifest, the
/// expected Revision commit, fixed component locations and the real Node version.
/// # Errors
/// Any mismatch, missing component, tampering or version/protocol drift.
pub fn verify_bundle(
    root: &Path,
    identity: &RevisionIdentity,
) -> Result<BundleManifest, ForgeupError> {
    let manifest = read_manifest(root)?;
    if manifest.forge_commit != identity.commit {
        return Err(invalid(
            "bundle was packaged for a different Forge commit than this Revision",
        ));
    }
    for relative in [
        NODE_RELATIVE,
        ENTRY_RELATIVE,
        INSPECTOR_RELATIVE,
        LOCK_RELATIVE,
    ] {
        let path = root.join(relative);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| invalid(format!("bundle is missing required component `{relative}`")))?;
        if !metadata.is_file() {
            return Err(invalid(format!(
                "component `{relative}` is not a regular file"
            )));
        }
    }
    if sha256(&root.join(LOCK_RELATIVE))? != manifest.package_lock_sha256 {
        return Err(invalid(
            "package-lock.json does not match the attested lock identity",
        ));
    }
    let actual = tree_hash(root)?;
    if actual != manifest.tree_sha256 {
        return Err(invalid(
            "bundle contents do not match the attested tree hash (tampered or incomplete)",
        ));
    }
    let node = root.join(NODE_RELATIVE);
    let output = offline_output(Command::new(&node).arg("--version").env_clear())
        .map_err(|e| io(&node, e))?;
    let reported = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || reported != format!("v{}", manifest.node_version) {
        return Err(invalid(format!(
            "bundled Node reports `{reported}`, expected v{}",
            manifest.node_version
        )));
    }
    Ok(manifest)
}

/// Copies a verified source bundle into `destination`, then verifies the *copy*, so
/// the installed bytes (not the source) are what is attested.
/// # Errors
/// Rejects an invalid source, copy failure or an installed copy that does not verify.
pub fn install(
    source: &Path,
    destination: &Path,
    identity: &RevisionIdentity,
) -> Result<InstalledRunner, ForgeupError> {
    verify_bundle(source, identity)?;
    copy_tree(source, destination)?;
    let manifest = verify_bundle(destination, identity)?;
    Ok(describe(destination, &manifest))
}

fn describe(root: &Path, manifest: &BundleManifest) -> InstalledRunner {
    InstalledRunner {
        root: root.to_path_buf(),
        node_path: root.join(NODE_RELATIVE),
        entry_path: root.join(ENTRY_RELATIVE),
        inspector_path: root.join(INSPECTOR_RELATIVE),
        protocol: manifest.runner_protocol,
        node_version: manifest.node_version.clone(),
        package_lock_sha256: manifest.package_lock_sha256.clone(),
        tree_sha256: manifest.tree_sha256.clone(),
    }
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), ForgeupError> {
    fs::create_dir_all(destination).map_err(|e| io(destination, e))?;
    for entry in fs::read_dir(source).map_err(|e| io(source, e))? {
        let entry = entry.map_err(|e| io(source, e))?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from).map_err(|e| io(&from, e))?;
        if metadata.is_dir() {
            copy_tree(&from, &to)?;
        } else if metadata.is_file() {
            fs::copy(&from, &to).map_err(|e| io(&to, e))?;
        } else {
            return Err(invalid("bundle contains a symlink or special file"));
        }
    }
    Ok(())
}

/// Cheap structural check used on ordinary selection; no hashing.
#[must_use]
pub fn present(runner: &InstalledRunner) -> bool {
    [
        &runner.node_path,
        &runner.entry_path,
        &runner.inspector_path,
    ]
    .iter()
    .all(|path| path.is_file())
}

/// Full re-attestation of an installed runner against its Revision and the identity
/// recorded at registration. Used before launch, daemon start and component resolution.
/// # Errors
/// Missing, modified or mismatched components, with an actionable redacted message.
pub fn verify_installed(
    runner: &InstalledRunner,
    identity: &RevisionIdentity,
) -> Result<(), ForgeupError> {
    let manifest = verify_bundle(&runner.root, identity)?;
    if manifest.tree_sha256 != runner.tree_sha256
        || manifest.package_lock_sha256 != runner.package_lock_sha256
        || manifest.node_version != runner.node_version
        || manifest.runner_protocol != runner.protocol
    {
        return Err(invalid(
            "installed bundle differs from the identity recorded at registration",
        ));
    }
    Ok(())
}

/// Reads and verifies a bundle directory without installing it.
/// # Errors
/// Same as [`verify_bundle`].
pub fn attest(source: &Path, identity: &RevisionIdentity) -> Result<BundleManifest, ForgeupError> {
    verify_bundle(source, identity)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{
        ForgeupPaths, RegistrationOptions, register_at, remove_at, runner_for_component_at,
        verify_runner,
    };
    use std::{
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU32, Ordering},
    };

    const COMMIT: &str = "0123456789012345678901234567890123456789";
    const IDENTITY: &str = r#"{"commit":"0123456789012345678901234567890123456789","dirty":false,"management_protocol_revision":1,"version":"test"}"#;
    static NEXT: AtomicU32 = AtomicU32::new(0);

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "forgeup-runner-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn identity() -> RevisionIdentity {
        serde_json::from_str(IDENTITY).unwrap()
    }

    fn write(path: &Path, contents: &str, mode: u32) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    /// A synthetic bundle whose "Node" is a shell script reporting the pinned version.
    fn bundle(root: &Path, mutate: impl FnOnce(&mut BundleManifest)) -> PathBuf {
        let bundle = root.join("bundle");
        write(
            &bundle.join(NODE_RELATIVE),
            "#!/bin/sh\necho v24.18.0\n",
            0o755,
        );
        write(&bundle.join(ENTRY_RELATIVE), "// entry", 0o644);
        write(&bundle.join(INSPECTOR_RELATIVE), "// inspector", 0o644);
        write(
            &bundle.join("runner/node_modules/dep/index.js"),
            "// dep",
            0o644,
        );
        write(&bundle.join(LOCK_RELATIVE), "{\"lock\":1}", 0o644);
        let mut manifest = BundleManifest {
            schema: 1,
            runner_protocol: 1,
            node_version: "24.18.0".into(),
            forge_commit: COMMIT.into(),
            package_lock_sha256: sha256(&bundle.join(LOCK_RELATIVE)).unwrap(),
            tree_sha256: tree_hash(&bundle).unwrap(),
        };
        mutate(&mut manifest);
        fs::write(
            bundle.join(BUNDLE_MANIFEST),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        bundle
    }

    fn message(result: Result<impl std::fmt::Debug, ForgeupError>) -> String {
        result.unwrap_err().to_string()
    }

    #[test]
    fn valid_bundle_installs_and_reattests_from_the_installed_copy() {
        let root = scratch("valid");
        let source = bundle(&root, |_| {});
        let installed = install(&source, &root.join("installed"), &identity()).unwrap();
        assert!(present(&installed));
        assert_eq!(installed.node_path, root.join("installed/node/node"));
        verify_installed(&installed, &identity()).unwrap();
        // The installed copy, not the source, is what runs: deleting the source is harmless.
        fs::remove_dir_all(&source).unwrap();
        verify_installed(&installed, &identity()).unwrap();
    }

    #[test]
    fn missing_tampered_and_mismatched_components_block_with_actionable_messages() {
        let root = scratch("blocked");
        let ok = bundle(&root, |_| {});
        let installed = install(&ok, &root.join("installed"), &identity()).unwrap();

        fs::write(installed.entry_path.clone(), "// tampered").unwrap();
        assert!(
            message(verify_installed(&installed, &identity())).contains("tampered or incomplete")
        );
        fs::write(installed.entry_path.clone(), "// entry").unwrap();
        verify_installed(&installed, &identity()).unwrap();

        fs::write(installed.root.join("runner/node_modules/dep/extra.js"), "x").unwrap();
        assert!(message(verify_installed(&installed, &identity())).contains("tampered"));
        fs::remove_file(installed.root.join("runner/node_modules/dep/extra.js")).unwrap();

        fs::remove_file(&installed.inspector_path).unwrap();
        assert!(!present(&installed));
        assert!(
            message(verify_installed(&installed, &identity()))
                .contains("missing required component")
        );

        let other = RevisionIdentity {
            commit: "f".repeat(40),
            ..identity()
        };
        let again = scratch("blocked-commit");
        let ok = bundle(&again, |_| {});
        assert!(message(install(&ok, &again.join("i"), &other)).contains("different Forge commit"));

        let again = scratch("blocked-protocol");
        let ok = bundle(&again, |m| m.runner_protocol = 2);
        assert!(message(install(&ok, &again.join("i"), &identity())).contains("incompatible"));

        let again = scratch("blocked-lock");
        let ok = bundle(&again, |m| m.package_lock_sha256 = "0".repeat(64));
        assert!(message(install(&ok, &again.join("i"), &identity())).contains("package-lock"));

        let again = scratch("blocked-node");
        let ok = bundle(&again, |m| m.node_version = "22.19.0".into());
        assert!(message(install(&ok, &again.join("i"), &identity())).contains("expected v22.19.0"));

        let again = scratch("blocked-tree");
        let ok = bundle(&again, |m| m.tree_sha256 = "1".repeat(64));
        assert!(
            message(install(&ok, &again.join("i"), &identity())).contains("attested tree hash")
        );
    }

    #[test]
    fn symlinks_and_unknown_manifest_fields_are_rejected() {
        let root = scratch("links");
        let source = bundle(&root, |_| {});
        std::os::unix::fs::symlink("/bin/sh", source.join("runner/node_modules/dep/link")).unwrap();
        assert!(message(install(&source, &root.join("i"), &identity())).contains("symlink"));
        fs::remove_file(source.join("runner/node_modules/dep/link")).unwrap();
        let mut text = fs::read_to_string(source.join(BUNDLE_MANIFEST)).unwrap();
        text.insert_str(text.len() - 1, ",\"node_path\":\"/usr/bin/node\"");
        fs::write(source.join(BUNDLE_MANIFEST), text).unwrap();
        assert!(message(install(&source, &root.join("i2"), &identity())).contains("malformed"));
    }

    fn registered(
        root: &Path,
        bundle: Option<&Path>,
    ) -> (ForgeupPaths, Result<crate::Installation, ForgeupError>) {
        let cli = root.join("cli");
        let daemon = root.join("daemon");
        for path in [&cli, &daemon] {
            write(
                path,
                &format!(
                    "#!/bin/sh\nif [ \"$1\" = \"--build-info\" ]; then echo '{IDENTITY}'; exit 0; fi\n"
                ),
                0o700,
            );
        }
        let paths = ForgeupPaths {
            config: root.join("home/config"),
            data: root.join("home/data"),
            runtime: root.join("home/runtime"),
            bin: root.join("home/bin"),
        };
        let result = register_at(
            &paths,
            "local",
            &cli,
            &daemon,
            None,
            RegistrationOptions {
                runner_bundle: bundle,
                ..RegistrationOptions::default()
            },
        );
        (paths, result)
    }

    #[test]
    fn registration_attests_runner_idempotently_and_retains_the_original_component() {
        let root = scratch("register");
        let source = bundle(&root, |_| {});
        let (paths, installation) = registered(&root, Some(&source));
        let installation = installation.unwrap();
        let runner = installation.runner.clone().unwrap();
        assert!(
            runner
                .root
                .starts_with(paths.installations().join(&installation.selector))
        );
        verify_runner(&installation).unwrap();
        // Idempotent for the identical bundle; a different bundle conflicts without replacement.
        let (_, again) = registered(&root, Some(&source));
        assert_eq!(again.unwrap(), installation);
        write(&source.join(ENTRY_RELATIVE), "// changed", 0o644);
        let mut manifest: BundleManifest =
            serde_json::from_slice(&fs::read(source.join(BUNDLE_MANIFEST)).unwrap()).unwrap();
        manifest.tree_sha256 = tree_hash(&source).unwrap();
        fs::write(
            source.join(BUNDLE_MANIFEST),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let (_, conflict) = registered(&root, Some(&source));
        assert!(matches!(
            conflict,
            Err(ForgeupError::RegistrationConflict(_))
        ));
        // Original component remains resolvable by its hash; unknown hashes fail closed.
        let found = runner_for_component_at(&paths, &runner.tree_sha256).unwrap();
        assert_eq!(found.selector, installation.selector);
        let unknown = runner_for_component_at(&paths, &"9".repeat(64))
            .unwrap_err()
            .to_string();
        assert!(unknown.contains("not redirected"));
        // Removing it while its state exists is refused; explicit purge is required.
        let removal = remove_at(&paths, &installation.selector, false).unwrap_err();
        assert!(matches!(removal, ForgeupError::RunnerRetained(_)));
        assert!(runner.root.exists());
        remove_at(&paths, &installation.selector, true).unwrap();
        assert!(runner_for_component_at(&paths, &runner.tree_sha256).is_err());
    }

    #[test]
    fn invalid_bundle_blocks_registration_and_leaves_no_installation() {
        let root = scratch("invalid-register");
        let source = bundle(&root, |m| m.forge_commit = "f".repeat(40));
        let (paths, result) = registered(&root, Some(&source));
        assert!(message(result).contains("different Forge commit"));
        let registry = crate::RegistryStore::open(paths.clone())
            .unwrap()
            .load()
            .unwrap();
        assert!(registry.installations.is_empty());
        // A Revision without a bundle has no runner and cannot be verified as matched.
        let (_, plain) = registered(&scratch("plain"), None);
        let plain = plain.unwrap();
        assert!(plain.runner.is_none());
        assert!(message(verify_runner(&plain)).contains("no matched runner bundle"));
    }

    #[test]
    fn global_path_and_environment_do_not_select_components() {
        let root = scratch("path");
        let source = bundle(&root, |_| {});
        let (_, installation) = registered(&root, Some(&source));
        let installation = installation.unwrap();
        // Selection is stored absolute paths inside the Installation; nothing reads PATH.
        let runner = verify_runner(&installation).unwrap();
        for path in [
            &runner.node_path,
            &runner.entry_path,
            &runner.inspector_path,
        ] {
            assert!(path.is_absolute() && path.starts_with(&runner.root));
        }
        assert_eq!(
            std::fs::read_to_string(runner.root.join(NODE_RELATIVE)).unwrap(),
            "#!/bin/sh\necho v24.18.0\n"
        );
    }

    #[test]
    fn packaged_entry_is_the_production_check_runner_not_the_synthetic_fixture() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let entry = fs::read_to_string(repository.join(ENTRY_RELATIVE)).unwrap();
        assert!(entry.contains("runCheckRunner(directory, worktree)"));
        assert!(!entry.contains("fauxProvider"));
        // Only `runner/src` is bundled; the synthetic runner lives outside it.
        assert!(!repository.join("runner/src/main.ts").exists());
        assert!(repository.join("runner/test/synthetic-runner.ts").exists());
    }
}
