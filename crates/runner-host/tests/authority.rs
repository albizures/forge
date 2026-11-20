//! Real Linux process/lock evidence, with a deliberately inert Python worker.
//! This is not pi-durable admission, process-death proof or release verification.
use forge_app::authorization_lease::AuthorityHost;
use forge_runner_host::{SpawnedOwner, inspection::ExclusiveInspection};
use fs2::FileExt;
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver},
    },
    time::Duration,
};

struct Fixture {
    root: PathBuf,
    directory: PathBuf,
    child: Child,
    input: ChildStdin,
    output: Receiver<String>,
    errors: Receiver<String>,
    evidence: Value,
}

fn lines(source: impl std::io::Read + Send + 'static) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(source).lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    rx
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "forge-authority-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let directory = root.join("runtime");
        let worktree = root.join("worktree");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(&worktree).unwrap();
        let worker = root.join("worker.py");
        fs::write(
            &worker,
            r#"import sys, os, json, fcntl
assert sys.stdin.readline() == '{"host_ready":true}\n'
print('ready', flush=True)
hold_after_eof = False
for line in sys.stdin:
    command = line.strip()
    if command == 'exit':
        break
    fd = json.loads(os.environ['FORGE_RUNNER_OWNER'])['lock_fd']
    if command == 'unlock':
        fcntl.flock(fd, fcntl.LOCK_UN)
    elif command == 'close-lock':
        os.close(fd)
    elif command == 'hold-after-eof':
        hold_after_eof = True
    print('ack', flush=True)
if hold_after_eof:
    import time
    while True:
        time.sleep(3600)
"#,
        )
        .unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_forge-runner-host"))
            .args([
                directory.as_os_str(),
                worktree.as_os_str(),
                std::ffi::OsStr::new("/usr/bin/python3"),
                worker.as_os_str(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = lines(child.stdout.take().unwrap());
        let errors = lines(child.stderr.take().unwrap());
        assert_eq!(
            output.recv_timeout(Duration::from_secs(10)).unwrap(),
            "ready"
        );
        let evidence =
            serde_json::from_slice(&fs::read(directory.join("owner.json")).unwrap()).unwrap();
        Self {
            root,
            directory,
            child,
            input,
            output,
            errors,
            evidence,
        }
    }

    fn command(&mut self, command: &str) {
        writeln!(self.input, "{command}").unwrap();
        if command == "exit" {
            // An unarmed host now retains ownership uncertainty before reporting
            // child exit. Still require the exit diagnostic within one deadline,
            // not an incidental ordering against the independent watchdog.
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                let line = self
                    .errors
                    .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                    .unwrap();
                if line.contains("runner-worker-exited") {
                    break;
                }
            }
        } else {
            assert_eq!(
                self.output.recv_timeout(Duration::from_secs(10)).unwrap(),
                "ack"
            );
        }
    }

    fn save_evidence(&self, evidence: &Value) {
        fs::write(self.directory.join("owner.json"), evidence.to_string()).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Test controller only: terminate the fixture worker iff its retained
        // start identity still matches. Never use this as a retirement policy.
        let pid = u32::try_from(self.evidence["worker_pid"].as_u64().unwrap()).unwrap();
        let process_info = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        let start = process_info
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(19));
        if start == self.evidence["worker_start"].as_str() {
            if let Some(pid) = rustix::process::Pid::from_raw(pid.cast_signed()) {
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            }
            // Host waits its worker before emitting this line. Bounded only to
            // avoid hanging a failed fixture, not to infer production absence.
            if self.child.try_wait().unwrap().is_none() {
                let _ = self.errors.recv_timeout(Duration::from_secs(2));
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        fs::remove_dir_all(&self.root).unwrap();
    }
}

/// A transient identity failure must not turn into permission to escalate after
/// descriptive evidence is restored. This tests the actual host watchdog and
/// real worker; it is not the operator retirement/revocation integration.
#[test]
fn failed_signal_ownership_stays_fenced_after_evidence_restoration() {
    let mut f = Fixture::new();
    f.command("hold-after-eof");
    let original = fs::read(f.directory.join("owner.json")).unwrap();
    let runtime = f.directory.join("runtime-preserved");
    let work = f.root.join("worktree/untracked-preserved");
    fs::write(&runtime, b"runtime bytes").unwrap();
    fs::write(&work, b"worktree bytes").unwrap();
    let mut changed = f.evidence.clone();
    changed["worker_start"] = "mismatched-start".into();
    f.save_evidence(&changed);
    // Malformed trusted control fences admission and initiates cancellation.
    // The worker never receives this control or any execution permission.
    writeln!(f.input, "{{\"host_control\":0}}").unwrap();
    let term = f.directory.join(format!(
        "watchdog-{}-{}-term-unverified.json",
        f.evidence["owner"]["host_pid"].as_u64().unwrap(),
        f.evidence["owner"]["host_start"].as_str().unwrap()
    ));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    // Synchronize on the persisted failed TERM attempt, not a guessed delay.
    while !term.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "missing TERM diagnostic"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(f.directory.join("owner.json"), &original).unwrap();
    loop {
        let line = f.errors.recv_timeout(Duration::from_secs(10)).unwrap();
        if line.contains("runner-watchdog-kill-") {
            assert!(line.contains("kill-unverified"), "{line}");
            break;
        }
    }
    let marker: Value =
        serde_json::from_slice(&fs::read(f.directory.join("watchdog.json")).unwrap()).unwrap();
    assert_eq!(marker["cleanup"], "unverified");
    assert!(ExclusiveInspection::acquire(&f.directory).is_err());
    let pid = f.evidence["worker_pid"].as_u64().unwrap();
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .collect();
    assert_eq!(fields[19], f.evidence["worker_start"].as_str().unwrap());
    assert!(!matches!(fields[0], "Z" | "X" | "x"));
    assert_eq!(fs::read(runtime).unwrap(), b"runtime bytes");
    assert_eq!(fs::read(work).unwrap(), b"worktree bytes");
    assert_eq!(fs::read(f.directory.join("owner.json")).unwrap(), original);
}

#[test]
fn live_spawn_has_shared_kernel_flock_and_monotonic_boottime_without_storage_open() {
    let mut f = Fixture::new();
    let mut authority = SpawnedOwner::capture(&mut f.child, &f.directory).unwrap();
    let first = authority.observe().unwrap();
    let second = authority.observe().unwrap();
    assert!(second.boottime_millis >= first.boottime_millis);
    assert_eq!(first.owner, second.owner);
    assert_eq!(
        first.owner.host_pid,
        u32::try_from(f.evidence["owner"]["host_pid"].as_u64().unwrap()).unwrap()
    );
    assert_eq!(
        first.owner.worker_start,
        f.evidence["worker_start"].as_str().unwrap()
    );
    assert_eq!(fs::read_dir(&f.directory).unwrap().count(), 2); // owner.lock + owner.json only
}

#[test]
fn changed_retained_identity_or_ancestry_never_authorizes_capture() {
    let changes: Vec<fn(&mut Value)> = vec![
        |v| v["owner"]["host_pid"] = serde_json::json!(std::process::id()),
        |v| v["owner"]["host_start"] = "stale".into(),
        |v| v["owner"]["boot_id"] = "stale".into(),
        |v| v["owner"]["lock_inode"] = "0".into(),
        |v| v["owner"]["lock_device"] = "0".into(),
        |v| v["owner"]["lock_fd"] = 1.into(),
        |v| v["worker_start"] = "stale".into(),
        |v| v["worker_pid"] = serde_json::json!(std::process::id()),
        |v| v["process_group"] = 1.into(),
        |v| v["extra"] = true.into(),
    ];
    let mut f = Fixture::new();
    for change in changes {
        let mut evidence = f.evidence.clone();
        change(&mut evidence);
        f.save_evidence(&evidence);
        assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_err());
    }
    f.save_evidence(&f.evidence);
    assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_ok());
}

#[test]
fn changed_evidence_fences_observer_even_after_file_is_restored() {
    let mut f = Fixture::new();
    let original = f.evidence.clone();
    let mut authority = SpawnedOwner::capture(&mut f.child, &f.directory).unwrap();
    let mut changed = original.clone();
    changed["worker_start"] = "changed".into();
    fs::write(f.directory.join("owner.json"), changed.to_string()).unwrap();
    assert!(authority.observe().is_err());
    fs::write(f.directory.join("owner.json"), original.to_string()).unwrap();
    assert!(authority.observe().is_err());
}

#[test]
fn unlock_or_missing_worker_descriptor_cannot_be_replaced_by_lock_file_identity() {
    for command in ["unlock", "close-lock"] {
        let mut f = Fixture::new();
        assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_ok());
        f.command(command);
        assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_err());
    }
}

#[test]
fn exited_worker_is_not_permission_even_while_supervisor_retains_lock() {
    let mut f = Fixture::new();
    assert!(ExclusiveInspection::acquire(&f.directory).is_err());
    assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_ok());
    f.command("exit");
    assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_err());
    let contender = fs::File::open(f.directory.join("owner.lock")).unwrap();
    assert!(contender.try_lock_exclusive().is_err());
    assert!(ExclusiveInspection::acquire(&f.directory).is_err());
    f.child.kill().unwrap();
    f.child.wait().unwrap();
    let evidence = fs::read(f.directory.join("owner.json")).unwrap();
    let guard = ExclusiveInspection::acquire(&f.directory).unwrap();
    guard.verify().unwrap();
    assert!(contender.try_lock_exclusive().is_err());
    assert_eq!(fs::read(f.directory.join("owner.json")).unwrap(), evidence);
    drop(guard);
    contender.try_lock_exclusive().unwrap();
}

#[test]
fn unrelated_child_or_exited_supervisor_cannot_adopt_a_surviving_workers_lock() {
    let mut f = Fixture::new();
    let mut unrelated = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    assert!(SpawnedOwner::capture(&mut unrelated, &f.directory).is_err());
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
    f.child.kill().unwrap();
    f.child.wait().unwrap();
    assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_err());
    // The surviving worker retains the inherited flock; it is not adoptable.
    assert!(ExclusiveInspection::acquire(&f.directory).is_err());
    let contender = fs::File::open(f.directory.join("owner.lock")).unwrap();
    assert!(contender.try_lock_exclusive().is_err());
}

#[test]
fn observation_rejects_replaced_directory_and_unsafe_evidence_files() {
    let mut f = Fixture::new();
    let evidence = f.directory.join("owner.json");
    fs::set_permissions(&evidence, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_err());
    fs::set_permissions(&evidence, fs::Permissions::from_mode(0o600)).unwrap();
    let old_evidence = f.root.join("old.json");
    fs::rename(&evidence, &old_evidence).unwrap();
    symlink(&old_evidence, &evidence).unwrap();
    assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_err());
    fs::remove_file(&evidence).unwrap();
    fs::rename(&old_evidence, &evidence).unwrap();
    let mut authority = SpawnedOwner::capture(&mut f.child, &f.directory).unwrap();
    let old_directory = f.root.join("old-runtime");
    fs::rename(&f.directory, &old_directory).unwrap();
    fs::create_dir(&f.directory).unwrap();
    fs::set_permissions(&f.directory, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(authority.observe().is_err());
    fs::remove_dir(&f.directory).unwrap();
    fs::rename(old_directory, &f.directory).unwrap();
    assert!(authority.observe().is_err());
}

#[test]
fn unsafe_paths_permissions_hardlinks_and_replaced_inodes_fail_closed() {
    let mut f = Fixture::new();
    let alias = f.root.join("alias");
    symlink(&f.directory, &alias).unwrap();
    assert!(SpawnedOwner::capture(&mut f.child, &alias).is_err());
    fs::set_permissions(&f.directory, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_err());
    fs::set_permissions(&f.directory, fs::Permissions::from_mode(0o700)).unwrap();
    let lock = f.directory.join("owner.lock");
    let old = f.root.join("old.lock");
    fs::hard_link(&lock, &old).unwrap();
    assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_err());
    fs::remove_file(&old).unwrap();
    let original_inode = fs::metadata(&lock).unwrap().ino();
    fs::rename(&lock, &old).unwrap();
    fs::write(&lock, "replacement").unwrap();
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
    assert_ne!(fs::metadata(&lock).unwrap().ino(), original_inode);
    assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_err());
    fs::remove_file(&lock).unwrap();
    symlink(&old, &lock).unwrap();
    assert!(SpawnedOwner::capture(&mut f.child, &f.directory).is_err());
}
