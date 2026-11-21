//! Original subreaper's verified crash boundary, never inferred from a PID/file.
use crate::{directory, invalid, protected_file, supervision::Supervised};
use forge_app::authorization_lease::Lease;
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::Path,
};

pub(crate) const RECEIPT_PREFIX: &str = "recovery-crash-";
fn name(lease: &Lease) -> String {
    format!("{RECEIPT_PREFIX}{}.json", lease.sequence)
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    version: u32,
    lease: Lease,
    children_absent: bool,
}

/// Only the original dedicated supervisor can produce this after reaping its
/// directly spawned worker. ECHILD covers escaped/reparented descendants too.
/// No signals, cleanup retry, authority revocation or capacity release occurs.
pub(crate) fn record(worker: &Supervised<'_>, path: &Path, lease: &Lease) -> io::Result<()> {
    if worker.owner() != &lease.owner {
        return Err(invalid());
    }
    worker.verify_storage()?;
    loop {
        match rustix::process::wait(rustix::process::WaitOptions::NOHANG) {
            Ok(Some(_)) => {}
            Err(rustix::io::Errno::CHILD) => break,
            _ => return Err(invalid()),
        }
    }
    worker.verify_storage()?;
    crate::retirement::persist(
        path,
        &name(lease),
        &Receipt {
            version: 1,
            lease: lease.clone(),
            children_absent: true,
        },
    )
}

/// Read protected proof from the exact original owner/grant under separately
/// verified exclusive ownership. A newly free flock or missing host is not proof.
/// # Errors
/// Rejects foreign owners/grants, partial evidence and unsafe canonical storage.
pub fn receipt(path: &Path, lease: &Lease) -> io::Result<Receipt> {
    directory(path)?;
    let lock = protected_file(&path.join("owner.lock"))?.metadata()?;
    if lock.dev().to_string() != lease.owner.lock_device
        || lock.ino().to_string() != lease.owner.lock_inode
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    protected_file(&path.join(name(lease)))?
        .take(65_537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err(invalid());
    }
    let proof: Receipt = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if proof.version != 1 || !proof.children_absent || proof.lease != *lease {
        return Err(invalid());
    }
    Ok(proof)
}
