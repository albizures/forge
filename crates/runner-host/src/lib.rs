//! Trusted Linux evidence for a directly spawned runner supervisor. This does not
//! adopt retained processes, release resources or authorize work. Supervision
//! can signal only its own directly spawned, still-verified process group.
pub mod control;
pub mod inspection;
pub mod model_auth;
pub mod quiescence;
pub mod retirement;
pub mod storage;
pub mod supervision;
pub mod supervisor;
mod watchdog;
use forge_app::{
    RegistrationError,
    authorization_lease::{AuthorityHost, Observation, Owner},
};
use serde::Deserialize;
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Child,
};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostRecord {
    host_pid: u32,
    host_start: String,
    boot_id: String,
    lock_device: String,
    lock_inode: String,
    lock_fd: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    owner: HostRecord,
    worker_pid: u32,
    worker_start: String,
    process_group: u32,
}

fn denied() -> RegistrationError {
    RegistrationError::new(
        "assignment-host-unverified",
        "live exclusive runner ownership and boottime evidence are required",
    )
}

fn invalid() -> io::Error {
    io::Error::other("unverified runner ownership")
}

fn owned(meta: &fs::Metadata, mode: u32) -> bool {
    meta.uid() == rustix::process::getuid().as_raw() && meta.mode() & 0o7777 == mode
}

fn directory(path: &Path) -> io::Result<fs::Metadata> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || !owned(&meta, 0o700) || fs::canonicalize(path)? != path {
        return Err(invalid());
    }
    Ok(meta)
}

fn protected_file(path: &Path) -> io::Result<fs::File> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
    let file = fs::File::from(fd);
    let meta = file.metadata()?;
    if !meta.is_file() || meta.nlink() != 1 || !owned(&meta, 0o600) {
        return Err(invalid());
    }
    Ok(file)
}

fn record(path: &Path) -> io::Result<Record> {
    let mut bytes = Vec::new();
    protected_file(&path.join("owner.json"))?
        .take(16_385)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 16_384 {
        return Err(invalid());
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

#[derive(Eq, PartialEq)]
struct Process {
    start: String,
    parent: u32,
    group: u32,
}

fn process(pid: u32) -> io::Result<Process> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    // comm may itself contain ')' and spaces. Index 19 after comm is field 22.
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .ok_or_else(invalid)?
        .1
        .split_whitespace()
        .collect();
    if fields.len() < 20 || matches!(fields[0], "Z" | "X" | "x") {
        return Err(invalid());
    }
    Ok(Process {
        start: fields[19].into(),
        parent: fields[1].parse().map_err(|_| invalid())?,
        group: fields[2].parse().map_err(|_| invalid())?,
    })
}

fn namespace(pid: u32) -> io::Result<PathBuf> {
    fs::read_link(format!("/proc/{pid}/ns/time"))
}

fn boottime() -> io::Result<u64> {
    use rustix::time::{ClockId, DynamicClockId, clock_gettime_dynamic};
    let time = clock_gettime_dynamic(DynamicClockId::Known(ClockId::Boottime))?;
    let seconds = u64::try_from(time.tv_sec).map_err(|_| invalid())?;
    let nanos = u64::try_from(time.tv_nsec).map_err(|_| invalid())?;
    if nanos >= 1_000_000_000 {
        return Err(invalid());
    }
    seconds
        .checked_mul(1_000)
        .and_then(|millis| millis.checked_add(nanos / 1_000_000))
        .ok_or_else(invalid)
}

fn flock(pid: u32, fd: u32, lock: &fs::Metadata, host: u32) -> io::Result<()> {
    let inherited = fs::metadata(format!("/proc/{pid}/fd/{fd}"))?;
    if inherited.dev() != lock.dev() || inherited.ino() != lock.ino() {
        return Err(invalid());
    }
    let info = fs::read_to_string(format!("/proc/{pid}/fdinfo/{fd}"))?;
    let device_inode = format!(
        "{:02x}:{:02x}:{}",
        rustix::fs::major(lock.dev()),
        rustix::fs::minor(lock.dev()),
        lock.ino()
    );
    let host = host.to_string();
    // fdinfo reports locks belonging to this open description, unlike a global
    // /proc/locks search which could accidentally find an unrelated owner.
    if !info.lines().any(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        fields.len() == 9
            && fields[0] == "lock:"
            && fields[2..]
                == [
                    "FLOCK",
                    "ADVISORY",
                    "WRITE",
                    &host,
                    &device_inode,
                    "0",
                    "EOF",
                ]
    }) {
        return Err(invalid());
    }
    Ok(())
}

/// Evidence can be captured only from a live, directly spawned `Child`, not a
/// PID/owner record supplied by an RPC or recovered from disk. The mutable borrow
/// keeps that child handle unavailable for reaping/reuse elsewhere while grants
/// are being issued. Observation performs no writable runtime open or signalling.
///
/// Trusted same-user host boundary only: checks are observations, not a sandbox
/// or an atomic guarantee that a process cannot exit immediately afterwards.
pub struct SpawnedOwner<'a> {
    supervisor: &'a mut Child,
    directory: PathBuf,
    directory_device: u64,
    directory_inode: u64,
    record: Record,
    time_namespace: PathBuf,
    last_boottime: Option<u64>,
    fenced: bool,
}

impl<'a> SpawnedOwner<'a> {
    /// Capture after the owned supervisor's readiness handshake. File evidence
    /// alone never authorizes adoption: process ancestry, start identities and
    /// both inherited kernel flock descriptions must also match on every read.
    /// # Errors
    /// Rejects exited/unrelated children, unsafe paths and missing/mismatched
    /// kernel evidence. No grant is issued by capture.
    pub fn capture(supervisor: &'a mut Child, path: &Path) -> Result<Self, RegistrationError> {
        let meta = directory(path).map_err(|_| denied())?;
        let record = record(path).map_err(|_| denied())?;
        let mut result = Self {
            supervisor,
            directory: path.into(),
            directory_device: meta.dev(),
            directory_inode: meta.ino(),
            record,
            time_namespace: namespace(std::process::id()).map_err(|_| denied())?,
            last_boottime: None,
            fenced: false,
        };
        result.observe()?;
        Ok(result)
    }

    fn verify(&mut self) -> io::Result<Observation> {
        if self.supervisor.try_wait()?.is_some() {
            return Err(invalid());
        }
        let meta = directory(&self.directory)?;
        let r = &self.record;
        if meta.dev() != self.directory_device
            || meta.ino() != self.directory_inode
            || record(&self.directory)? != *r
            || r.owner.host_pid != self.supervisor.id()
            || r.worker_pid == 0
            || r.owner.lock_fd < 3
            || r.process_group != r.worker_pid
            || r.owner.boot_id != fs::read_to_string("/proc/sys/kernel/random/boot_id")?.trim()
        {
            return Err(invalid());
        }
        let host = process(r.owner.host_pid)?;
        let worker = process(r.worker_pid)?;
        if host.start != r.owner.host_start
            || host.parent != std::process::id()
            || worker.start != r.worker_start
            || worker.parent != r.owner.host_pid
            || worker.group != r.process_group
        {
            return Err(invalid());
        }
        let lock = protected_file(&self.directory.join("owner.lock"))?.metadata()?;
        if lock.dev().to_string() != r.owner.lock_device
            || lock.ino().to_string() != r.owner.lock_inode
        {
            return Err(invalid());
        }
        for pid in [r.owner.host_pid, r.worker_pid] {
            flock(pid, r.owner.lock_fd, &lock, r.owner.host_pid)?;
        }
        let now = boottime()?;
        // Recheck process and clock coordinates after all external observations.
        // Any unreadable or changed evidence fails closed; no cached fallback.
        for pid in [std::process::id(), r.owner.host_pid, r.worker_pid] {
            if namespace(pid)? != self.time_namespace {
                return Err(invalid());
            }
        }
        if process(r.owner.host_pid)? != host || process(r.worker_pid)? != worker {
            return Err(invalid());
        }
        if self.last_boottime.is_some_and(|last| now < last) {
            return Err(invalid());
        }
        self.last_boottime = Some(now);
        Ok(Observation {
            owner: Owner {
                boot_id: r.owner.boot_id.clone(),
                time_namespace: self.time_namespace.to_string_lossy().into_owned(),
                host_pid: r.owner.host_pid,
                host_start: r.owner.host_start.clone(),
                worker_pid: r.worker_pid,
                worker_start: r.worker_start.clone(),
                lock_device: r.owner.lock_device.clone(),
                lock_inode: r.owner.lock_inode.clone(),
            },
            boottime_millis: now,
        })
    }
}

impl AuthorityHost for SpawnedOwner<'_> {
    fn observe(&mut self) -> Result<Observation, RegistrationError> {
        if self.fenced {
            return Err(denied());
        }
        self.fenced = true;
        let observation = self.verify().map_err(|_| denied())?;
        self.fenced = false;
        Ok(observation)
    }
}
