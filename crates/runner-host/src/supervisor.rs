//! Linux single-owner supervision boundary, spawned by the daemon's durable launcher.
//! The lock deliberately outlives the worker: exit is not verified cleanup.
use crate::{supervision::Supervised, watchdog};
use fs2::FileExt;
use std::{
    fs,
    io::{self, Write},
    os::{
        fd::AsRawFd,
        unix::{fs::MetadataExt, process::CommandExt},
    },
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

fn protected_directory(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.uid() != rustix::process::getuid().as_raw()
        || meta.mode() & 0o777 != 0o700
        || fs::canonicalize(path)? != path
    {
        return Err(io::Error::other(
            "runtime directory must be canonical, owner-only and owned",
        ));
    }
    Ok(())
}

fn process_start(pid: u32) -> io::Result<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    // comm may contain spaces and parentheses; field 22 follows the final ')'.
    stat.rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().nth(19))
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("missing Linux process start identity"))
}

fn persist_owner(
    directory: &Path,
    identity: &serde_json::Value,
    child: &std::process::Child,
) -> io::Result<()> {
    let evidence = serde_json::json!({
        "owner": identity,
        "worker_pid": child.id(),
        "worker_start": process_start(child.id())?,
        "process_group": child.id(),
    });
    let pending = directory.join(format!("owner.{}.pending", std::process::id()));
    // The canonical directory is private to the trusted host user; no-follow
    // also refuses unexpected final-component redirection.
    let evidence_fd = rustix::fs::open(
        &pending,
        rustix::fs::OFlags::CREATE
            | rustix::fs::OFlags::EXCL
            | rustix::fs::OFlags::WRONLY
            | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )?;
    let mut evidence_file = fs::File::from(evidence_fd);
    evidence_file.write_all(evidence.to_string().as_bytes())?;
    evidence_file.sync_all()?;
    fs::rename(pending, directory.join("owner.json"))?;
    fs::File::open(directory)?.sync_all()
}

/// Run the trusted single-owner supervisor with explicit component paths.
/// The executable and process-level integration tracer share this implementation;
/// calling it does not issue a grant or authorize writable execution.
/// # Errors
/// Rejects unsafe storage, unavailable ownership or failed worker construction.
/// Once supervision starts it retains ownership until explicitly terminated.
pub fn run(
    directory: &Path,
    worktree: &Path,
    node: &Path,
    runner: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    protected_directory(directory)?;
    let worktree = fs::canonicalize(worktree)?;
    if directory.starts_with(&worktree) || worktree.starts_with(directory) {
        return Err("runtime storage must be separate from the Managed Worktree".into());
    }
    for program in [node, runner] {
        if !program.is_absolute() {
            return Err("explicit absolute runtime component paths required".into());
        }
    }
    let fd = rustix::fs::open(
        directory.join("owner.lock"),
        rustix::fs::OFlags::CREATE
            | rustix::fs::OFlags::RDWR
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )?;
    let lock = fs::File::from(fd);
    let meta = lock.metadata()?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.uid() != rustix::process::getuid().as_raw()
        || meta.mode() & 0o777 != 0o600
    {
        return Err("unsafe ownership lock".into());
    }
    lock.try_lock_exclusive()
        .map_err(|_| io::Error::other("runner-owner-busy"))?;
    // A directory sealed as never owned (a failed launch reconciled by Forge)
    // is never owned afterwards: refuse before recording ownership or spawning.
    if crate::seal::sealed(directory)? {
        return Err("runner-runtime-sealed".into());
    }
    // Never unlink a stale lock. Stale descriptive evidence cannot authorize
    // adoption, termination or writable recovery. The worker checks its journal.
    // Share this *same* flock open description with the worker. If the host
    // dies first, its still-live worker must continue fencing other starts.
    // This is not a cleanup claim: either process may retain ownership.
    rustix::io::fcntl_setfd(&lock, rustix::io::FdFlags::empty())?;
    let host_pid = std::process::id();
    let identity = serde_json::json!({
        "host_pid": host_pid,
        "host_start": process_start(host_pid)?,
        "boot_id": fs::read_to_string("/proc/sys/kernel/random/boot_id")?.trim(),
        "lock_device": meta.dev().to_string(),
        "lock_inode": meta.ino().to_string(),
        "lock_fd": lock.as_raw_fd(),
    });
    // A dedicated subreaper retains escaped descendants after leader exit.
    // Only kernel ECHILD after cancellation can yield a cleanup receipt.
    rustix::process::set_child_subreaper(Some(rustix::process::Pid::INIT))?;
    let mut child = Command::new(node)
        .arg(runner)
        .arg(directory)
        .arg(&worktree)
        .env_clear()
        .env("FORGE_RUNNER_OWNER", identity.to_string())
        .current_dir(&worktree)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    if let Err(error) = persist_owner(directory, &identity, &child) {
        child.kill()?;
        child.wait()?;
        return Err(error.into());
    }
    let mut input = child.stdin.take().ok_or("missing worker input pipe")?;
    let supervised = match Supervised::capture(&mut child, directory) {
        Ok(supervised) => supervised,
        Err(error) => {
            child.kill()?;
            child.wait()?;
            return Err(error.into());
        }
    };
    // Existing diagnostic markers and receipts are never converted into permission
    // or removed by a restart. `owner.json` is different: it is descriptive only,
    // replaced by each supervisor start (`persist_owner` above), and never used to
    // authorize anything. Read-only observation can still run without receiving
    // any grant.
    let mut retained_blocker = false;
    for marker in [
        "watchdog.json",
        "retirement-intent.json",
        crate::quiescence::INTENT,
    ] {
        match fs::symlink_metadata(directory.join(marker)) {
            Ok(_) => retained_blocker = true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    for entry in fs::read_dir(directory)? {
        let name = entry?.file_name();
        let name = name.to_string_lossy();
        // Crash proof and (settled or not) fence markers of earlier episodes:
        // only the exact recovery preparation may grant again.
        if name.starts_with(crate::crash::RECEIPT_PREFIX) || crate::fence::is_marker(&name) {
            retained_blocker = true;
        }
    }
    let state = Arc::new(Mutex::new(watchdog::State::new(
        supervised.owner().clone(),
        retained_blocker,
    )));
    writeln!(input, "{{\"host_ready\":true}}")?;
    let input_state = Arc::clone(&state);
    let input_directory = directory.to_owned();
    std::thread::spawn(move || watchdog::forward_input(input, &input_state, &input_directory));
    watchdog::supervise(supervised, directory, &state);
    Ok(())
}
