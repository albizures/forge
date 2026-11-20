//! Dedicated Linux subreaper retirement. No PID adoption or universal same-user
//! containment: only this directly spawned supervisor's descendants are covered.
use crate::{directory, invalid, protected_file, supervision::Supervised};
use forge_app::{
    authorization_lease::TERM_GRACE_MILLIS,
    durable_execution::retirement::{Operation, Receipt},
};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    path::Path,
    time::{Duration, Instant},
};

pub(crate) fn persist(
    directory: &Path,
    name: &str,
    value: &impl serde::Serialize,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    match protected_file(&directory.join(name)) {
        Ok(mut file) => {
            let mut old = Vec::new();
            Read::by_ref(&mut file).take(65_537).read_to_end(&mut old)?;
            return if old == bytes { Ok(()) } else { Err(invalid()) };
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let fd = rustix::fs::open(
        directory.join(name),
        rustix::fs::OFlags::CREATE
            | rustix::fs::OFlags::EXCL
            | rustix::fs::OFlags::WRONLY
            | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )?;
    let mut file = fs::File::from(fd);
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::File::open(directory)?.sync_all()
}

/// # Errors
/// Rejects unsafe/changed storage or mismatched retained proof. Caller must hold
/// exclusive inspection until its Forge result transaction completes.
pub fn receipt(directory: &Path, operation: &Operation) -> io::Result<Receipt> {
    crate::directory(directory)?;
    let lock = protected_file(&directory.join("owner.lock"))?.metadata()?;
    if lock.dev().to_string() != operation.owner.lock_device
        || lock.ino().to_string() != operation.owner.lock_inode
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    protected_file(&directory.join("retirement-receipt.json"))?
        .take(65_537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err(invalid());
    }
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if receipt.version != 1 || !receipt.children_absent || receipt.operation != *operation {
        return Err(invalid());
    }
    Ok(receipt)
}

pub(crate) fn retire(
    worker: &mut Supervised<'_>,
    path: &Path,
    operation: &Operation,
) -> io::Result<()> {
    worker.verify()?;
    if &operation.owner != worker.owner() || operation.operation_id.trim().is_empty() {
        return Err(invalid());
    }
    stop_owned(worker, path, "retirement-intent.json", operation)?;
    persist(
        path,
        "retirement-receipt.json",
        &Receipt {
            version: 1,
            operation: operation.clone(),
            children_absent: true,
        },
    )
}

// Shared direct-child subreaper mechanism. Callers bind their distinct intent
// and receipt policy; neither a signal nor leader exit produces a receipt.
pub(crate) fn stop_owned(
    worker: &mut Supervised<'_>,
    path: &Path,
    intent_name: &str,
    intent: &impl serde::Serialize,
) -> io::Result<()> {
    worker.verify()?;
    let identity = worker.directory_identity();
    persist(path, intent_name, intent)?;
    worker.signal(rustix::process::Signal::TERM)?;
    let start = Instant::now();
    let mut escalated = false;
    loop {
        // This executable is a dedicated subreaper, set before its only spawn.
        // ECHILD is a kernel child-set observation, not leader exit, signal/abort
        // success, a heartbeat, or a /proc scan that can miss an escaped child.
        match rustix::process::wait(rustix::process::WaitOptions::NOHANG) {
            Ok(Some(_)) => continue,
            Err(rustix::io::Errno::CHILD) => break,
            Err(error) => return Err(error.into()),
            Ok(None) => {}
        }
        if start.elapsed() >= Duration::from_millis(TERM_GRACE_MILLIS) && !escalated {
            escalated = true;
            // If the leader is gone we cannot safely reuse its numeric group ID.
            // An escaped/surviving child therefore retains uncertainty and flock.
            worker.signal(rustix::process::Signal::KILL)?;
        }
        if start.elapsed() >= Duration::from_millis(TERM_GRACE_MILLIS + 2_000) {
            return Err(invalid());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let meta = directory(path)?;
    if (meta.dev(), meta.ino()) != identity {
        return Err(invalid());
    }
    worker.verify_storage()
}
