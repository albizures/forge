//! Recovery-specific predecessor stop and protected descendant proof. No store
//! mutation, authority revocation, effect resolution or writable runtime open.
use crate::{
    invalid, protected_file,
    retirement::{persist, stop_owned},
    supervision::Supervised,
};
use forge_app::durable_execution::recovery::quiescence::{Operation, Receipt};
use std::{
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::Path,
};

pub(crate) const INTENT: &str = "recovery-quiescence-intent.json";
const RECEIPT: &str = "recovery-quiescence-receipt.json";

pub(crate) fn quiesce(
    worker: &mut Supervised<'_>,
    path: &Path,
    operation: &Operation,
) -> io::Result<()> {
    operation.validate().map_err(|_| invalid())?;
    if &operation.predecessor.owner != worker.owner() {
        return Err(invalid());
    }
    stop_owned(worker, path, INTENT, operation)?;
    persist(
        path,
        RECEIPT,
        &Receipt {
            version: 1,
            operation: operation.clone(),
            children_absent: true,
        },
    )
}

/// Require predecessor proof with no retained host-fault or retirement marker.
/// Call only while retaining separately verified exclusive ownership; absence of
/// these markers alone grants nothing. Any metadata error remains uncertainty.
/// # Errors
/// Rejects unrelated safety fences and all failures of exact receipt inspection.
#[derive(serde::Serialize)]
pub enum PredecessorProof {
    Quiesced(Box<Receipt>),
    Crashed(Box<crate::crash::Receipt>),
    /// The original host's post-watchdog-fence descendant proof.
    Fenced(Box<crate::fence::Receipt>),
    /// The original host fenced before accepting any grant (T26).
    Ungranted(Box<crate::fence::UngrantedReceipt>),
}

/// The original host's proof that nothing of `lease`'s owner still runs.
///
/// Exact crash or post-fence proof for this lease, or else (T18 F1) the same
/// host's proof for an *earlier* lease of the same Launch, or its ungranted
/// receipt. Forge commits a lease before delivering it: a commit whose
/// delivery failed leaves Forge's last lease ahead of the host's. The host's
/// own crash/fence/ungranted proof binds the last lease it actually accepted
/// and is written only after ECHILD; a fenced host accepts nothing later.
/// # Errors
/// No matching proof, or unsafe or tampered evidence.
pub fn predecessor_proof(
    directory: &Path,
    lease: &forge_app::authorization_lease::Lease,
) -> io::Result<PredecessorProof> {
    if let Ok(proof) = crate::crash::receipt(directory, lease) {
        return Ok(PredecessorProof::Crashed(Box::new(proof)));
    }
    if let Ok(proof) = crate::fence::receipt(directory, lease) {
        return Ok(PredecessorProof::Fenced(Box::new(proof)));
    }
    if let Ok(proof) = crate::crash::receipt_for_owner(directory, lease) {
        return Ok(PredecessorProof::Crashed(Box::new(proof)));
    }
    if let Ok(proof) = crate::fence::receipt_for_owner(directory, lease) {
        return Ok(PredecessorProof::Fenced(Box::new(proof)));
    }
    crate::fence::ungranted_receipt_for(directory, &lease.owner)
        .map(|proof| PredecessorProof::Ungranted(Box::new(proof)))
}

fn reaped(
    directory: &Path,
    lease: &forge_app::authorization_lease::Lease,
) -> io::Result<PredecessorProof> {
    predecessor_proof(directory, lease)
}

/// Verify original-host quiescence or a directly reaped crash boundary.
/// # Errors
/// Missing proof or any unrelated safety marker remains uncertainty.
pub fn waiting_receipt(directory: &Path, operation: &Operation) -> io::Result<PredecessorProof> {
    for name in ["retirement-intent.json", "retirement-receipt.json"] {
        match std::fs::symlink_metadata(directory.join(name)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(invalid()),
        }
    }
    // A watchdog fence blocks recovery unless its own original host proved
    // descendant absence afterwards. A settled marker is history, not proof
    // for this predecessor: that still needs the exact receipt below.
    crate::fence::settled(directory)?;
    operation.validate().map_err(|_| invalid())?;
    match std::fs::symlink_metadata(directory.join(INTENT)) {
        Ok(_) => {
            let mut bytes = Vec::new();
            protected_file(&directory.join(INTENT))?
                .take(65_537)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 65_536 {
                return Err(invalid());
            }
            let old: Operation = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            let proof = receipt(directory, &old)?;
            if old == *operation {
                return Ok(PredecessorProof::Quiesced(Box::new(proof)));
            }
            if old.claim.launch != operation.claim.launch
                || old.predecessor.sequence >= operation.predecessor.sequence
            {
                return Err(invalid());
            }
            // Preserve completed earlier quiescence evidence. It cannot stand in
            // for the current predecessor: require that exact grant's new proof.
            reaped(directory, &operation.predecessor)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            reaped(directory, &operation.predecessor)
        }
        Err(error) => Err(error),
    }
}

/// Read the exact protected original-host receipt under separately held exclusive
/// inspection. A receipt is not fresh authorization or evidence of safe effects.
/// The caller must keep inspection through its dependent verification/commit and
/// recheck all original Forge bindings; it cannot adopt a descriptive PID.
/// # Errors
/// Rejects unsafe ownership/lock identity, missing, partial, foreign or oversized
/// proof. This never creates/repairs storage, clears intent or signals a process.
pub fn receipt(directory: &Path, operation: &Operation) -> io::Result<Receipt> {
    operation.validate().map_err(|_| invalid())?;
    crate::directory(directory)?;
    let lock = protected_file(&directory.join("owner.lock"))?.metadata()?;
    let owner = &operation.predecessor.owner;
    if lock.dev().to_string() != owner.lock_device || lock.ino().to_string() != owner.lock_inode {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    protected_file(&directory.join(RECEIPT))?
        .take(65_537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err(invalid());
    }
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if receipt.version != 1 || !receipt.children_absent || receipt.operation != *operation {
        return Err(invalid());
    }
    // Intent remains an execution fence, not something receipt inspection clears.
    let mut intent = Vec::new();
    protected_file(&directory.join(INTENT))?
        .take(65_537)
        .read_to_end(&mut intent)?;
    if intent.len() > 65_536
        || serde_json::from_slice::<Operation>(&intent).map_err(|_| invalid())? != *operation
    {
        return Err(invalid());
    }
    Ok(receipt)
}

/// T18 third pass: the widened predecessor proof accepts only the same owner's
/// proof for the same Launch, never another owner's, a later lease's or
/// tampered evidence, and finds ungranted proof by the lease owner even after a
/// later host overwrote `owner.json`.
#[cfg(test)]
mod predecessor_tests {
    use super::predecessor_proof;
    use forge_app::{
        authorization_lease::{Lease, Owner},
        durable_execution::Launch,
    };
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::{Path, PathBuf},
    };

    fn runtime() -> (tempfile::TempDir, PathBuf, Owner) {
        let root = tempfile::tempdir().unwrap();
        let path = fs::canonicalize(root.path()).unwrap().join("runtime");
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        protected(&path.join("owner.lock"), b"");
        let lock = fs::metadata(path.join("owner.lock")).unwrap();
        let owner = make_owner(7, &lock);
        (root, path, owner)
    }

    fn make_owner(pid: u32, lock: &fs::Metadata) -> Owner {
        Owner {
            boot_id: "boot".into(),
            time_namespace: "time:[1]".into(),
            host_pid: pid,
            host_start: "100".into(),
            worker_pid: pid + 1,
            worker_start: "101".into(),
            lock_device: lock.dev().to_string(),
            lock_inode: lock.ino().to_string(),
        }
    }

    fn protected(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn launch(id: &str) -> Launch {
        Launch {
            assignment_id: id.into(),
            operation_id: "op".into(),
            attempt_id: "attempt".into(),
            profile_revision: 1,
            runtime_id: "runtime".into(),
            incarnation_id: "incarnation".into(),
            conversation_id: "conversation".into(),
            store_id: "store".into(),
            forge_revision: "rev".into(),
            component_hash: "hash".into(),
            runtime_schema: 2,
        }
    }

    fn lease(owner: &Owner, sequence: u32) -> Lease {
        Lease {
            launch: launch("assignment"),
            generation: 1,
            owner: owner.clone(),
            daemon_epoch: "epoch".into(),
            sequence,
            issued_boottime_millis: 1_000,
            expires_boottime_millis: 31_000,
        }
    }

    fn marker(path: &Path, owner: &Owner, last: Option<&Lease>) {
        let value = serde_json::json!({ "version": 1, "code": "runner-lease-expired", "owner": owner, "last_lease": last, "cleanup": "unverified" });
        protected(&path.join("watchdog.json"), format!("{value}\n").as_bytes());
    }

    #[test]
    fn fence_proof_for_an_earlier_lease_covers_only_later_leases_of_that_owner() {
        let (_root, path, owner) = runtime();
        let fenced = lease(&owner, 2);
        marker(&path, &owner, Some(&fenced));
        crate::fence::record(&path, "watchdog.json", &fenced).unwrap();
        // Exact, and the committed-but-undelivered later lease of that owner.
        assert!(predecessor_proof(&path, &fenced).is_ok());
        assert!(predecessor_proof(&path, &lease(&owner, 3)).is_ok());
        // An earlier lease than the proof, another owner (a later recovery
        // episode's host) or another Launch never match.
        assert!(predecessor_proof(&path, &lease(&owner, 1)).is_err());
        let lock = fs::metadata(path.join("owner.lock")).unwrap();
        assert!(predecessor_proof(&path, &lease(&make_owner(9, &lock), 3)).is_err());
        let mut foreign = lease(&owner, 3);
        foreign.launch = launch("other");
        assert!(predecessor_proof(&path, &foreign).is_err());
        // Tampered receipt or marker bytes are not proof.
        let receipt = path.join("watchdog-receipt-2.json");
        let original = fs::read(&receipt).unwrap();
        let mut tampered: serde_json::Value = serde_json::from_slice(&original).unwrap();
        tampered["children_absent"] = false.into();
        fs::write(&receipt, serde_json::to_vec(&tampered).unwrap()).unwrap();
        assert!(predecessor_proof(&path, &lease(&owner, 3)).is_err());
        fs::write(&receipt, &original).unwrap();
        let bytes = fs::read(path.join("watchdog.json")).unwrap();
        fs::write(
            path.join("watchdog.json"),
            [bytes.as_slice(), b" "].concat(),
        )
        .unwrap();
        assert!(predecessor_proof(&path, &lease(&owner, 3)).is_err());
    }

    #[test]
    fn crash_proof_for_an_earlier_lease_covers_only_that_owner_and_launch() {
        let (_root, path, owner) = runtime();
        let crashed = lease(&owner, 2);
        crate::retirement::persist(
            &path,
            "recovery-crash-2.json",
            &serde_json::json!({ "version": 1, "lease": crashed, "children_absent": true }),
        )
        .unwrap();
        assert!(predecessor_proof(&path, &lease(&owner, 4)).is_ok());
        assert!(predecessor_proof(&path, &lease(&owner, 1)).is_err());
        let lock = fs::metadata(path.join("owner.lock")).unwrap();
        assert!(predecessor_proof(&path, &lease(&make_owner(9, &lock), 4)).is_err());
        let mut foreign = lease(&owner, 4);
        foreign.launch = launch("other");
        assert!(predecessor_proof(&path, &foreign).is_err());
    }

    #[test]
    fn ungranted_proof_is_found_by_the_lease_owner_after_owner_json_changes() {
        let (_root, path, owner) = runtime();
        marker(&path, &owner, None);
        crate::fence::record_ungranted(&path, "watchdog.json", &owner).unwrap();
        // A later recovery host (another owner) has since rewritten owner.json.
        protected(&path.join("owner.json"), br#"{"owner":{"host_pid":99}}"#);
        assert!(predecessor_proof(&path, &lease(&owner, 1)).is_ok());
        let lock = fs::metadata(path.join("owner.lock")).unwrap();
        let later = make_owner(99, &lock);
        assert!(predecessor_proof(&path, &lease(&later, 2)).is_err());
        // The same receipt copied under another owner's name: owner equality refuses.
        let receipt = fs::read(path.join("ungranted-receipt-7-100.json")).unwrap();
        protected(&path.join("ungranted-receipt-99-100.json"), &receipt);
        assert!(predecessor_proof(&path, &lease(&later, 2)).is_err());
        // Another lock identity than the owner's refuses before reading.
        let mut relocked = owner.clone();
        relocked.lock_inode = "1".into();
        assert!(predecessor_proof(&path, &lease(&relocked, 1)).is_err());
        // A receipt whose exact marker bytes name a lease never settles as
        // ungranted (the marker bytes still match, so only `last_lease` refuses).
        marker(&path, &owner, Some(&lease(&owner, 1)));
        let named = fs::read_to_string(path.join("watchdog.json")).unwrap();
        let forged = serde_json::json!({ "version": 1, "marker": "watchdog.json", "marker_contents": named, "owner": owner, "children_absent": true });
        fs::remove_file(path.join("ungranted-receipt-7-100.json")).unwrap();
        protected(
            &path.join("ungranted-receipt-7-100.json"),
            &serde_json::to_vec(&forged).unwrap(),
        );
        assert!(predecessor_proof(&path, &lease(&owner, 1)).is_err());
    }
}
