//! Explicit deletion of one retired runtime directory. This never starts, repairs or
//! recreates a runtime; it removes only recognized protected files while holding the
//! exclusive host ownership lock, and refuses everything else untouched.
use crate::{inspection::ExclusiveInspection, invalid};
use std::{
    fs, io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

/// Exact names runner-host and the pinned runner write in a runtime directory, in
/// removal order: data first, then evidence, and the ownership lock last so a retry
/// still holds the lock proof. Bounded patterns slot in at their [`Rank`].
const EXACT: [(&str, Rank); 12] = [
    ("runtime.sqlite-wal", Rank::Data),
    ("runtime.sqlite-shm", Rank::Data),
    ("runtime.sqlite-journal", Rank::Data),
    ("runtime.sqlite", Rank::Data),
    ("check-runtime.json", Rank::Configuration),
    ("recovery-quiescence-intent.json", Rank::Recovery),
    ("recovery-quiescence-receipt.json", Rank::Recovery),
    ("watchdog.json", Rank::Marker),
    ("retirement-intent.json", Rank::Marker),
    ("retirement-receipt.json", Rank::Receipt),
    ("owner.json", Rank::Owner),
    ("owner.lock", Rank::Lock),
];

/// Removal phases. Within a phase files are removed in name order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Rank {
    /// Conversation/runtime storage.
    Data,
    /// Runner configuration written beside the data.
    Configuration,
    /// Per-event diagnostics (`watchdog-<pid>-<start>-<signal>-<result>.json`).
    Diagnostic,
    /// Recovery evidence: crash receipts, verification proofs and quiescence.
    Recovery,
    /// Fencing markers that block new execution while any data might remain.
    Marker,
    /// Retirement receipt (proof of revoked cleanup).
    Receipt,
    /// Ownership evidence, including an interrupted `owner.<pid>.pending` write.
    Owner,
    /// `owner.lock`, always last.
    Lock,
}

/// A canonical decimal: no sign, no leading zero, and in range for `T`.
fn canonical<T: std::str::FromStr + ToString>(digits: &str) -> Option<T> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: T = digits.parse().ok()?;
    (value.to_string() == digits).then_some(value)
}

/// Classify one directory entry name. Anything not written by runner-host or the
/// pinned runner is unrecognized. Patterns accept only bounded, canonical parts.
fn classify(name: &str) -> Option<Rank> {
    if let Some((_, rank)) = EXACT.iter().find(|(n, _)| *n == name) {
        return Some(*rank);
    }
    // `recovery-crash-<lease sequence>.json`; sequences start at 1 (crash.rs).
    if let Some(rest) = name
        .strip_prefix(crate::crash::RECEIPT_PREFIX)
        .and_then(|r| r.strip_suffix(".json"))
    {
        return canonical::<u32>(rest)
            .filter(|s| *s >= 1)
            .map(|_| Rank::Recovery);
    }
    // `recovery-proof-<64 lowercase hex>.json` (recovery_proof.rs).
    if let Some(key) = name
        .strip_prefix("recovery-proof-")
        .and_then(|r| r.strip_suffix(".json"))
    {
        let hex = key.len() == 64
            && key
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        return hex.then_some(Rank::Recovery);
    }
    // `owner.<host pid>.pending`, left only by an interrupted evidence write.
    if let Some(pid) = name
        .strip_prefix("owner.")
        .and_then(|r| r.strip_suffix(".pending"))
    {
        return canonical::<u32>(pid)
            .filter(|p| *p >= 1)
            .map(|_| Rank::Owner);
    }
    // Post-fence evidence (fence.rs): episode markers block like `watchdog.json`;
    // `watchdog-receipt-<lease sequence>.json` is the host's quiescence receipt.
    if crate::fence::is_marker(name) {
        return Some(Rank::Marker);
    }
    if crate::fence::is_receipt(name) {
        return Some(Rank::Receipt);
    }
    // `watchdog-<host pid>-<host start ticks>-<term|kill>-<sent|unverified>.json`,
    // or `...-quiescence-unproven.json` when post-fence proof is unavailable.
    if let Some(rest) = name
        .strip_prefix("watchdog-")
        .and_then(|r| r.strip_suffix(".json"))
    {
        let mut parts = rest.split('-');
        let (Some(pid), Some(start), Some(signal), Some(result), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return None;
        };
        let valid = canonical::<u32>(pid).is_some_and(|p| p >= 1)
            && canonical::<u64>(start).is_some()
            && matches!(
                (signal, result),
                ("term" | "kill", "sent" | "unverified") | ("quiescence", "unproven")
            );
        return valid.then_some(Rank::Diagnostic);
    }
    None
}

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

/// Every entry, validated before anything is removed, in removal order.
fn recognized(directory: &Path) -> io::Result<Vec<(Rank, PathBuf)>> {
    let uid = rustix::process::getuid().as_raw();
    let mut found = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let rank = name.to_str().and_then(classify).ok_or_else(unexpected)?;
        let meta = fs::symlink_metadata(entry.path())?;
        if !meta.is_file() || meta.nlink() != 1 || meta.uid() != uid {
            return Err(unexpected());
        }
        found.push((rank, entry.path()));
    }
    found.sort();
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
    for (rank, path) in recognized(directory)? {
        if rank == Rank::Lock {
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

    const PROOF: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// Every artifact real lifecycle code leaves behind (retirement, crash proof,
    /// quiescence, recovery verification, watchdog and interrupted owner writes).
    fn lifecycle_names() -> Vec<String> {
        vec![
            "check-runtime.json".into(),
            "runtime.sqlite-shm".into(),
            "runtime.sqlite-journal".into(),
            "watchdog.json".into(),
            "retirement-intent.json".into(),
            "retirement-receipt.json".into(),
            "recovery-crash-1.json".into(),
            "recovery-crash-4294967295.json".into(),
            "recovery-quiescence-intent.json".into(),
            "recovery-quiescence-receipt.json".into(),
            format!("recovery-proof-{PROOF}.json"),
            "owner.4242.pending".into(),
            "watchdog-17-123456-term-sent.json".into(),
            "watchdog-17-0-kill-unverified.json".into(),
            "watchdog-4294967295-18446744073709551615-term-unverified.json".into(),
            "watchdog-17-123456-quiescence-unproven.json".into(),
            "watchdog-17-123456-fence.json".into(),
            "watchdog-receipt-1.json".into(),
            "watchdog-receipt-4294967295.json".into(),
        ]
    }

    #[test]
    fn classifies_each_lifecycle_pattern_in_removal_order() {
        assert_eq!(classify("runtime.sqlite"), Some(Rank::Data));
        assert_eq!(classify("check-runtime.json"), Some(Rank::Configuration));
        assert_eq!(
            classify("watchdog-1-2-kill-sent.json"),
            Some(Rank::Diagnostic)
        );
        assert_eq!(classify("recovery-crash-7.json"), Some(Rank::Recovery));
        assert_eq!(
            classify(&format!("recovery-proof-{PROOF}.json")),
            Some(Rank::Recovery)
        );
        assert_eq!(
            classify("recovery-quiescence-receipt.json"),
            Some(Rank::Recovery)
        );
        assert_eq!(classify("watchdog.json"), Some(Rank::Marker));
        assert_eq!(classify("watchdog-1-2-fence.json"), Some(Rank::Marker));
        assert_eq!(classify("watchdog-receipt-3.json"), Some(Rank::Receipt));
        assert_eq!(
            classify("watchdog-1-2-quiescence-unproven.json"),
            Some(Rank::Diagnostic)
        );
        assert_eq!(classify("retirement-intent.json"), Some(Rank::Marker));
        assert_eq!(classify("retirement-receipt.json"), Some(Rank::Receipt));
        assert_eq!(classify("owner.1.pending"), Some(Rank::Owner));
        assert_eq!(classify("owner.json"), Some(Rank::Owner));
        assert_eq!(classify("owner.lock"), Some(Rank::Lock));
        for name in lifecycle_names() {
            assert!(classify(&name).is_some(), "{name}");
        }
    }

    #[test]
    fn near_miss_names_are_unrecognized() {
        let upper = PROOF.to_uppercase();
        let short = &PROOF[1..];
        let long = format!("{PROOF}0");
        for name in [
            "",
            ".",
            "runtime.sqlite.bak",
            "Runtime.sqlite",
            "owner.lock ",
            "retirement-intent.json.tmp",
            "recovery-crash-.json",
            "recovery-crash-0.json",
            "recovery-crash-01.json",
            "recovery-crash-+1.json",
            "recovery-crash--1.json",
            "recovery-crash-4294967296.json",
            "recovery-crash-1.json.tmp",
            "recovery-crash-1",
            "recovery-crash-1x.json",
            "recovery-proof-.json",
            &format!("recovery-proof-{upper}.json"),
            &format!("recovery-proof-{short}.json"),
            &format!("recovery-proof-{long}.json"),
            "recovery-proof-../x.json",
            "recovery-quiescence-other.json",
            "owner..pending",
            "owner.0.pending",
            "owner.007.pending",
            "owner.1.pending.tmp",
            "owner.-1.pending",
            "owner.4294967296.pending",
            "watchdog-.json",
            "watchdog-1-2-term.json",
            "watchdog-1-2-term-sent-x.json",
            "watchdog-0-2-term-sent.json",
            "watchdog-01-2-term-sent.json",
            "watchdog-1-02-term-sent.json",
            "watchdog-1-18446744073709551616-term-sent.json",
            "watchdog-1-2-stop-sent.json",
            "watchdog-1-2-term-maybe.json",
            "watchdog-1-2-TERM-sent.json",
            "watchdog-1-2-quiescence-sent.json",
            "watchdog-1-2-kill-unproven.json",
            "watchdog-0-2-fence.json",
            "watchdog-01-2-fence.json",
            "watchdog-1-2-fence.json.tmp",
            "watchdog-receipt-0.json",
            "watchdog-receipt-01.json",
            "watchdog-receipt-4294967296.json",
            "watchdog-receipt-.json",
            "watchdog-1-2-term-sent.json.tmp",
            "watchdog-x-2-term-sent.json",
            "controlled-mode",
        ] {
            assert_eq!(classify(name), None, "{name:?}");
        }
    }

    #[test]
    fn removes_every_real_lifecycle_artifact_with_the_lock_last() {
        let root = root();
        let dir = runtime(root.path());
        for name in lifecycle_names() {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(dir.join(name))
                .unwrap();
        }
        let order: Vec<_> = recognized(&dir)
            .unwrap()
            .into_iter()
            .map(|(r, _)| r)
            .collect();
        assert!(order.is_sorted());
        assert_eq!(order.last(), Some(&Rank::Lock));
        assert_eq!(order.first(), Some(&Rank::Data));
        assert_eq!(purge(Some(&dir)).unwrap(), Purged::Removed);
        assert!(!dir.exists());
    }

    #[test]
    fn lifecycle_artifacts_never_excuse_unsafe_or_unknown_content() {
        use std::os::unix::fs::PermissionsExt as _;
        // Each case: a near-miss name, a symlinked/hard-linked/directory lifecycle
        // name. All refuse before any file is removed.
        type Case<'a> = &'a dyn Fn(&Path, &Path);
        let cases: [Case<'_>; 5] = [
            &|dir, _| fs::write(dir.join("recovery-crash-01.json"), b"x").unwrap(),
            &|dir, root| symlink(root, dir.join("retirement-intent.json")).unwrap(),
            &|dir, root| {
                let outside = root.join("outside");
                fs::write(&outside, b"x").unwrap();
                fs::hard_link(&outside, dir.join(format!("recovery-proof-{PROOF}.json"))).unwrap();
            },
            &|dir, _| fs::create_dir(dir.join("watchdog-1-2-term-sent.json")).unwrap(),
            &|dir, _| {
                let path = dir.join("owner.9.pending");
                fs::write(&path, b"x").unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                fs::hard_link(&path, dir.join("owner.10.pending")).unwrap();
            },
        ];
        for case in cases {
            let root = root();
            let dir = runtime(root.path());
            fs::write(dir.join("retirement-receipt.json"), b"r").unwrap();
            case(&dir, root.path());
            let before: Vec<_> = {
                let mut v: Vec<_> = fs::read_dir(&dir)
                    .unwrap()
                    .map(|e| e.unwrap().file_name())
                    .collect();
                v.sort();
                v
            };
            assert!(purge(Some(&dir)).is_err());
            let mut after: Vec<_> = fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            after.sort();
            assert_eq!(after, before, "nothing removed");
        }
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
