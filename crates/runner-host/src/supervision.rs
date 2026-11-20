//! Independent host watchdog's process boundary; no PID-based adoption.
use crate::{directory, flock, invalid, namespace, process, protected_file, record};
use forge_app::authorization_lease::Owner;
use std::{
    fs, io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::{Child, ExitStatus},
};

/// Fallible `CLOCK_BOOTTIME` in this host's verified time namespace.
/// # Errors
/// Rejects unavailable clocks and overflowing/invalid kernel readings.
pub fn boottime_millis() -> io::Result<u64> {
    crate::boottime()
}

/// Borrow the actual child handle; descriptive records alone cannot construct an
/// adopted process. Only this controller reaps it, preventing PID reuse between
/// verification and signalling. Exit/kill success never releases the flock.
pub struct Supervised<'a> {
    child: &'a mut Child,
    path: PathBuf,
    device: u64,
    inode: u64,
    owner: Owner,
    record: crate::Record,
    fenced: bool,
}

impl<'a> Supervised<'a> {
    /// # Errors
    /// Fails closed on missing protected evidence, identity or shared flock.
    pub fn capture(child: &'a mut Child, path: &Path) -> io::Result<Self> {
        let meta = directory(path)?;
        let owner = verify(child, path)?;
        Ok(Self {
            child,
            path: path.into(),
            device: meta.dev(),
            inode: meta.ino(),
            owner,
            record: record(path)?,
            fenced: false,
        })
    }

    #[must_use]
    pub fn owner(&self) -> &Owner {
        &self.owner
    }

    /// Captured protected directory coordinates, for retained diagnostic writes.
    #[must_use]
    pub fn directory_identity(&self) -> (u64, u64) {
        (self.device, self.inode)
    }

    /// # Errors
    /// Unreadable/changed evidence terminally fences this handle. Restoring a
    /// descriptive file cannot authorize a later escalation or retirement retry.
    pub fn verify(&mut self) -> io::Result<()> {
        if self.fenced {
            return Err(invalid());
        }
        // Set before fallible I/O: every failed observation retains uncertainty,
        // including errors reading the directory, process identity or flock.
        self.fenced = true;
        let meta = directory(&self.path)?;
        if meta.dev() != self.device
            || meta.ino() != self.inode
            || record(&self.path)? != self.record
            || verify(self.child, &self.path)? != self.owner
        {
            return Err(invalid());
        }
        self.fenced = false;
        Ok(())
    }

    /// Protected storage must still match after all children are gone; this does
    /// not verify live execution or clear a previously observed identity fault.
    pub(crate) fn verify_storage(&self) -> io::Result<()> {
        let meta = directory(&self.path)?;
        let lock = protected_file(&self.path.join("owner.lock"))?.metadata()?;
        if self.fenced
            || meta.dev() != self.device
            || meta.ino() != self.inode
            || record(&self.path)? != self.record
            || lock.dev().to_string() != self.owner.lock_device
            || lock.ino().to_string() != self.owner.lock_inode
        {
            return Err(invalid());
        }
        Ok(())
    }

    /// # Errors
    /// A failed wait does not establish worker exit or process-group absence.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// # Errors
    /// Signals only while the directly spawned leader is still verified/live.
    /// If it has exited, descendants remain uncertain and no retained numeric
    /// group ID is used to kill a possibly unrelated/reused process group.
    pub fn signal(&mut self, signal: rustix::process::Signal) -> io::Result<()> {
        self.verify()?;
        let pid = i32::try_from(self.child.id()).map_err(|_| invalid())?;
        let group = rustix::process::Pid::from_raw(pid).ok_or_else(invalid)?;
        rustix::process::kill_process_group(group, signal)?;
        Ok(())
    }
}

fn verify(child: &mut Child, path: &Path) -> io::Result<Owner> {
    if child.try_wait()?.is_some() {
        return Err(invalid());
    }
    let r = record(path)?;
    let host_pid = std::process::id();
    if r.owner.host_pid != host_pid
        || r.worker_pid != child.id()
        || r.process_group != child.id()
        || r.owner.lock_fd < 3
        || r.owner.boot_id != fs::read_to_string("/proc/sys/kernel/random/boot_id")?.trim()
    {
        return Err(invalid());
    }
    let host = process(host_pid)?;
    let worker = process(child.id())?;
    let time_namespace = namespace(host_pid)?;
    let lock = protected_file(&path.join("owner.lock"))?.metadata()?;
    if host.start != r.owner.host_start
        || worker.start != r.worker_start
        || worker.parent != host_pid
        || worker.group != child.id()
        || namespace(child.id())? != time_namespace
        || lock.dev().to_string() != r.owner.lock_device
        || lock.ino().to_string() != r.owner.lock_inode
    {
        return Err(invalid());
    }
    for pid in [host_pid, child.id()] {
        flock(pid, r.owner.lock_fd, &lock, host_pid)?;
    }
    // Recheck identities after I/O; the borrowed Child is not reaped elsewhere.
    if process(host_pid)? != host || process(child.id())? != worker {
        return Err(invalid());
    }
    Ok(Owner {
        boot_id: r.owner.boot_id,
        time_namespace: time_namespace.to_string_lossy().into_owned(),
        host_pid,
        host_start: host.start,
        worker_pid: child.id(),
        worker_start: worker.start,
        lock_device: r.owner.lock_device,
        lock_inode: r.owner.lock_inode,
    })
}
