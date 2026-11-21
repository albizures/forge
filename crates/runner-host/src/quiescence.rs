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
    crate::fence::ungranted_receipt(directory)
        .ok()
        .filter(|proof| proof.owner == lease.owner)
        .map(|proof| PredecessorProof::Ungranted(Box::new(proof)))
        .ok_or_else(invalid)
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
