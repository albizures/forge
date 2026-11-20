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
pub fn waiting_receipt(directory: &Path, operation: &Operation) -> io::Result<Receipt> {
    for name in [
        "watchdog.json",
        "retirement-intent.json",
        "retirement-receipt.json",
    ] {
        match std::fs::symlink_metadata(directory.join(name)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(invalid()),
        }
    }
    receipt(directory, operation)
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
