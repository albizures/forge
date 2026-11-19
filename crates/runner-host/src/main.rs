//! Linux single-owner supervision boundary. Not wired to production dispatch.
//! The lock deliberately outlives the worker: exit is not verified cleanup.
use fs2::FileExt;
use std::{
    env, fs,
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{fs::MetadataExt, process::CommandExt},
    },
    path::Path,
    process::{Command, Stdio},
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

fn forward_input(mut input: std::process::ChildStdin) {
    // Do not use io::copy: Linux socket-to-pipe splice can hold the
    // destination pipe lock while awaiting input, blocking the handshake.
    let result = (|| -> io::Result<()> {
        let mut buffer = [0; 8192];
        let mut source = io::stdin().lock();
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                return Ok(());
            }
            input.write_all(&buffer[..count])?;
        }
    })();
    if let Err(error) = result {
        eprintln!("runner-host-input: {error}");
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err(
            "usage: forge-runner-host RUNTIME_DIRECTORY WORKTREE ABSOLUTE_NODE ABSOLUTE_RUNNER"
                .into(),
        );
    }
    let directory = Path::new(&args[0]);
    protected_directory(directory)?;
    let worktree = fs::canonicalize(&args[1])?;
    if directory.starts_with(&worktree) || worktree.starts_with(directory) {
        return Err("runtime storage must be separate from the Managed Worktree".into());
    }
    for program in [&args[2], &args[3]] {
        if !Path::new(program).is_absolute() {
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
    let mut child = Command::new(&args[2])
        .arg(&args[3])
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
    let initialization = (|| -> Result<(), Box<dyn std::error::Error>> {
        let evidence = serde_json::json!({
            "owner": identity,
            "worker_pid": child.id(),
            "worker_start": process_start(child.id())?,
            "process_group": child.id(),
        });
        let pending = directory.join(format!("owner.{host_pid}.pending"));
        // Safe because the canonical directory is private to the trusted host user;
        // NOFOLLOW below also refuses unexpected final-component redirection.
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
        Ok(())
    })();
    if let Err(error) = initialization {
        child.kill()?;
        child.wait()?;
        return Err(error);
    }
    let mut input = child.stdin.take().ok_or("missing worker input pipe")?;
    writeln!(input, "{{\"host_ready\":true}}")?;
    std::thread::spawn(move || forward_input(input));
    let status = child.wait()?;
    eprintln!("runner-worker-exited: {status}; ownership retained pending reconciliation");
    // No cleanup/adoption API exists in T04. An external controller must retain
    // uncertainty even if this host itself dies. T05 adds the independent lease
    // watchdog; T09 supplies verified retirement. No PID-based kill here.
    loop {
        std::thread::park();
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("runner-host: {error}");
        std::process::exit(1);
    }
}
