//! Real SQLite -> daemon service -> protected pipe -> Rust supervisor -> pinned
//! Node consumer integration. Requires Node 24 and runner's locked npm install.
//! Synthetic provider only; no live Forge Records, credentials or rollout.
use forge_app::{
    AgentDispatchRepository, NewAgentAssignment, NewTypedRecord, Project, ProjectRepository,
    RecordRepository, RecordType, WorkspaceRepository,
    authorization_lease::session::Tick,
    durable_execution::{Launch, Repository as _},
    outcomes::Attempt,
};
use forge_server::durable_authorization::DaemonAuthority;
use forge_store::SqliteProjectRepository;
#[path = "authorization_session/effect_admission.rs"]
mod effect_admission;
#[path = "authorization_session/purge_runtime.rs"]
mod purge_runtime;
#[path = "authorization_session/quiesced_inspection.rs"]
mod quiesced_inspection;
#[path = "authorization_session/quiescence.rs"]
mod quiescence;
#[path = "authorization_session/recovery_crash.rs"]
mod recovery_crash;
#[path = "authorization_session/recovery_handoff.rs"]
mod recovery_handoff;
#[path = "authorization_session/recovery_inspection.rs"]
mod recovery_inspection;
#[path = "authorization_session/recovery_progress.rs"]
mod recovery_progress;
#[path = "authorization_session/recovery_receiver.rs"]
mod recovery_receiver;
#[path = "authorization_session/recovery_reconciliation.rs"]
mod recovery_reconciliation;
#[path = "authorization_session/retirement.rs"]
mod retirement;
#[path = "authorization_session/transcript_status.rs"]
mod transcript_status;
#[path = "authorization_session/waiting_recovery_inspection.rs"]
mod waiting_recovery_inspection;
use rusqlite::params;
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver},
    },
    time::Duration,
};

fn output(source: impl std::io::Read + Send + 'static) -> Receiver<String> {
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
fn event(receiver: &Receiver<String>, kind: &str) -> Value {
    loop {
        let line = receiver
            .recv_timeout(Duration::from_secs(40))
            .expect("controlled worker event timed out");
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_ne!(
            value["event"], "fixture-failed",
            "controlled runtime fixture failed"
        );
        if value["event"] == kind {
            return value;
        }
    }
}
fn diagnostic(receiver: &Receiver<String>, code: &str) {
    loop {
        let line = receiver
            .recv_timeout(Duration::from_secs(40))
            .unwrap_or_else(|error| panic!("supervisor diagnostic {code} timed out: {error}"));
        if line.contains(code) {
            return;
        }
    }
}
/// T26: a pipe lost before any grant races two valid host paths — the worker
/// exits on EOF first, or the host fences on the lost issuer first. Wait for
/// whichever diagnostic of `codes` appears.
fn diagnostic_any(receiver: &Receiver<String>, codes: &[&str]) {
    loop {
        let line = receiver
            .recv_timeout(Duration::from_secs(40))
            .unwrap_or_else(|error| panic!("supervisor diagnostics {codes:?} timed out: {error}"));
        if codes.iter().any(|code| line.contains(code)) {
            return;
        }
    }
}

fn open(path: &Path) -> SqliteProjectRepository {
    SqliteProjectRepository::open(path)
        .unwrap()
        .in_record_workspace("test")
        .unwrap()
}

struct Fixture {
    root: PathBuf,
    directory: PathBuf,
    database: PathBuf,
    store: SqliteProjectRepository,
    launch: Launch,
    record: u64,
    child: Child,
    output: Receiver<String>,
    errors: Receiver<String>,
    evidence: Value,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        Self::with_component(mode, "controlled-component".into())
    }

    fn create_assignment(store: &SqliteProjectRepository) -> (u64, u64) {
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "controlled lease test".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "assignment".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 2,
            })
            .unwrap();
        (task.id, store.list_record_checks(task.id).unwrap()[0].id)
    }

    /// A second open Assignment (`legacy`) on a new Task, for T26 claim tests.
    fn create_second_assignment(store: &SqliteProjectRepository) -> (u64, u64) {
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "legacy claim".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "legacy".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 2,
            })
            .unwrap();
        (task.id, store.list_record_checks(task.id).unwrap()[0].id)
    }

    fn with_component(mode: &str, component_hash: String) -> Self {
        Self::with_repository(mode, component_hash, false)
    }

    fn with_repository(mode: &str, component_hash: String, git_repository: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "forge-lease-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = root.canonicalize().unwrap();
        let base = git_repository.then(|| recovery_progress::repository(&root));
        let database = root.join("forge.sqlite");
        let store = SqliteProjectRepository::open(&database).unwrap();
        store.create_workspace("test", "Test", &root).unwrap();
        let store = store.in_record_workspace("test").unwrap();
        store
            .register(&Project {
                reference: "alpha".into(),
                workspace_id: "test".into(),
                slug: "alpha".into(),
                project_root: root.clone(),
                origin_url: "https://example.test/repo.git".into(),
            })
            .unwrap();
        // Test-owned synthetic bound claim, matching existing store fixtures.
        // This is not profile registration or a production dispatch path.
        let c = rusqlite::Connection::open(&database).unwrap();
        c.execute("INSERT INTO agent_profiles(record_type, revision, provider, agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis) SELECT record_type, 2, 'pi-durable', agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis FROM agent_profiles WHERE record_type = 'task' AND revision = 1", []).unwrap();
        let (record, check) = Self::create_assignment(&store);
        let worktree = if git_repository {
            recovery_progress::worktree(&root, record)
        } else {
            let path = root.join("worktree");
            fs::create_dir(&path).unwrap();
            path
        };
        let attempt = Attempt {
            id: "attempt".into(),
            record_id: record,
            check_id: check,
            generation: 1,
            evidence: forge_app::outcomes::Evidence::Repository {
                repository: if git_repository {
                    root.display().to_string()
                } else {
                    "https://example.test/repo.git".into()
                },
                commit: base.clone().unwrap_or_else(|| "a".repeat(40)),
            },
            actor: "assignment".into(),
            body: "controlled lease test".into(),
            continuation: None,
            recovery: None,
        };
        c.execute("INSERT INTO record_attempts(id, record_id, check_id, attempt_json) VALUES ('attempt', ?1, ?2, ?3)", params![record.cast_signed(), check.cast_signed(), serde_json::to_string(&attempt).unwrap()]).unwrap();
        c.execute(
            "INSERT INTO assignment_check_attempts VALUES ('assignment', 'attempt')",
            [],
        )
        .unwrap();
        if let Some(base) = base {
            c.execute(
                "INSERT INTO task_base_commits VALUES (?1, ?2)",
                params![record.cast_signed(), base],
            )
            .unwrap();
        }
        drop(c);
        let launch = Launch {
            assignment_id: "assignment".into(),
            operation_id: "launch".into(),
            attempt_id: "attempt".into(),
            profile_revision: 2,
            runtime_id: "runtime".into(),
            incarnation_id: "incarnation".into(),
            conversation_id: "conversation".into(),
            store_id: "store".into(),
            forge_revision: "test-revision".into(),
            component_hash,
            runtime_schema: 2,
        };
        store.reserve("test", &launch).unwrap();
        let directory = DaemonAuthority::new()
            .unwrap()
            .prepare_directory(&store, "test", &launch)
            .unwrap();
        fs::write(directory.join("controlled-mode"), mode).unwrap();
        let (child, output, errors) = spawn(&directory, &worktree);
        let evidence = event(&output, "owned")["identity"].clone();
        Self {
            root,
            directory,
            database,
            store,
            launch,
            record,
            child,
            output,
            errors,
            evidence,
        }
    }
}

fn spawn(directory: &Path, worktree: &Path) -> (Child, Receiver<String>, Receiver<String>) {
    let worker = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../runner/test/session-worker.mjs")
        .canonicalize()
        .unwrap();
    spawn_component(directory, worktree, &worker)
}

fn spawn_component(
    directory: &Path,
    worktree: &Path,
    worker: &Path,
) -> (Child, Receiver<String>, Receiver<String>) {
    // PATH is used only by this test's executable-discovery probe, never by the
    // cleared supervisor/worker environment or production component selection.
    let probe = Command::new("node")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .args(["--print", "process.execPath"])
        .output()
        .expect("Node 24 is required for runner integration tests");
    assert!(probe.status.success());
    let node = PathBuf::from(String::from_utf8(probe.stdout).unwrap().trim());
    let mut child = Command::new(env!("CARGO_BIN_EXE_forge-runner-host"))
        .args([directory, worktree, &node, worker])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = output(child.stdout.take().unwrap());
    let errors = crate::output(child.stderr.take().unwrap());
    (child, output, errors)
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Test controller only, using its own directly spawned host's handshake.
        let pid = u32::try_from(self.evidence["worker_pid"].as_u64().unwrap()).unwrap();
        let info = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        let start = info
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(19));
        if start == self.evidence["worker_start"].as_str()
            && let Some(pid) = rustix::process::Pid::from_raw(pid.cast_signed())
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn outage(mode: &str) {
    let mut f = Fixture::new(mode);
    let authority = DaemonAuthority::new().unwrap();
    let mut service = authority
        .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
        .unwrap();
    let Tick::Delivered { lease, .. } = service.tick(&f.store).unwrap() else {
        panic!("initial grant was not delivered")
    };
    let lease = forge_app::authorization_lease::Lease::clone(&lease);
    assert_eq!(event(&f.output, "authorized")["sequence"], 1);
    let started = event(&f.output, "started");
    assert_eq!(started["requests"], 1);
    // Stop calling the driver but retain its open pipe: pure loss of renewal,
    // not EOF, worker restart or a shortened test lease. Real 30-second expiry.
    let result = event(&f.output, "post-expiry");
    assert_eq!(result["requests"], 1);
    assert_eq!(result["cancellations"], 1);
    assert_eq!(result["tools"], u64::from(mode == "child"));
    if mode == "child" {
        assert_eq!(result["child_exit"], "SIGTERM");
    }
    diagnostic(&f.errors, "runner-lease-expired");
    assert!(service.tick(&open(&f.database)).is_err());
    drop(service);
    let reopened = open(&f.database);
    let resources = reopened
        .list_agent_assignment_resources("assignment")
        .unwrap();
    assert!(resources.iter().any(
        |resource| resource.resource_kind == "authorization-uncertain"
            && resource.retired_at_millis.is_none()
    ));
    assert!(reopened.dispatch_hold(f.record).unwrap().is_some());
    reopened
        .close_agent_assignment(
            "assignment",
            forge_app::AgentAssignmentClosureReason::Crashed,
        )
        .unwrap();
    assert!(
        open(&f.database)
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    assert!(
        open(&f.database)
            .claim_next_agent_assignment("alpha", "replacement")
            .unwrap()
            .is_none()
    );
    let c = rusqlite::Connection::open(&f.database).unwrap();
    let retained: String = c
        .query_row(
            "SELECT lease_json FROM assignment_authorization_leases",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<forge_app::authorization_lease::Lease>(&retained).unwrap(),
        lease
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM assignment_authorization_leases",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    drop(c);
    diagnostic(&f.errors, "runner-worker-exited");
    assert_fence_proven(&mut f, &lease);
    assert_restart_denied(&mut f);
}

/// T25: after the watchdog fence the original subreaper proves descendant
/// absence, writes the exact receipt and exits, freeing the lock. Stale, foreign
/// or tampered evidence, and a marker without its receipt, are refused.
fn assert_fence_proven(f: &mut Fixture, lease: &forge_app::authorization_lease::Lease) {
    use forge_runner_host::fence::{self, Diagnosis};
    diagnostic(&f.errors, "runner-watchdog-quiescence-verified");
    assert!(f.child.wait().unwrap().success());
    let guard = forge_runner_host::inspection::ExclusiveInspection::acquire(&f.directory).unwrap();
    let receipt = fence::receipt(&f.directory, lease).unwrap();
    assert_eq!(receipt.marker, "watchdog.json");
    assert!(receipt.children_absent);
    fence::settled(&f.directory).unwrap();
    assert_eq!(Diagnosis::of(&f.directory), Diagnosis::Proven);
    let mut stale = lease.clone();
    stale.sequence += 1;
    assert!(fence::receipt(&f.directory, &stale).is_err());
    let mut foreign = lease.clone();
    foreign.owner.host_start = "other-host-start".into();
    assert!(fence::receipt(&f.directory, &foreign).is_err());
    let name = f
        .directory
        .join(format!("watchdog-receipt-{}.json", lease.sequence));
    let original = fs::read(&name).unwrap();
    let mut tampered: Value = serde_json::from_slice(&original).unwrap();
    tampered["children_absent"] = false.into();
    fs::write(&name, serde_json::to_vec(&tampered).unwrap()).unwrap();
    assert!(fence::receipt(&f.directory, lease).is_err());
    assert!(fence::settled(&f.directory).is_err());
    tampered["children_absent"] = true.into();
    tampered["lease"]["sequence"] = (lease.sequence + 1).into();
    fs::write(&name, serde_json::to_vec(&tampered).unwrap()).unwrap();
    assert!(fence::receipt(&f.directory, lease).is_err());
    assert!(fence::settled(&f.directory).is_err());
    fs::write(&name, &original).unwrap();
    let marker = f.directory.join("watchdog.json");
    let bytes = fs::read(&marker).unwrap();
    fs::write(&marker, [bytes.as_slice(), b" "].concat()).unwrap();
    assert!(fence::receipt(&f.directory, lease).is_err());
    assert_eq!(Diagnosis::of(&f.directory), Diagnosis::Unproven);
    fs::write(&marker, &bytes).unwrap();
    let moved = f.root.join("moved-receipt");
    fs::rename(&name, &moved).unwrap();
    assert!(fence::settled(&f.directory).is_err());
    assert_eq!(Diagnosis::of(&f.directory), Diagnosis::Unproven);
    fs::rename(&moved, &name).unwrap();
    fence::receipt(&f.directory, lease).unwrap();
    fence::settled(&f.directory).unwrap();
    drop(guard);
}

fn assert_restart_denied(f: &mut Fixture) {
    // A genuinely fresh daemon epoch and supervisor cannot turn retained grant
    // history/expired runtime bytes into permission after closure and reopen.
    // A settled fence is history: an ordinary grant still needs recovery.
    let _ = f.child.kill();
    f.child.wait().unwrap();
    let marker = fs::read(f.directory.join("watchdog.json")).unwrap();
    let runtime = fs::read(f.directory.join("controlled.sqlite")).unwrap();
    let (child, stdout, stderr) = spawn(&f.directory, &f.root.join("worktree"));
    f.child = child;
    f.output = stdout;
    f.errors = stderr;
    f.evidence = event(&f.output, "owned")["identity"].clone();
    let restarted = DaemonAuthority::new().unwrap();
    let mut rejected = restarted
        .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
        .unwrap();
    assert!(rejected.tick(&open(&f.database)).is_err());
    drop(rejected);
    diagnostic(&f.errors, "runner-worker-exited");
    assert_eq!(fs::read(f.directory.join("watchdog.json")).unwrap(), marker);
    assert_eq!(
        fs::read(f.directory.join("controlled.sqlite")).unwrap(),
        runtime
    );
    let reopened = open(&f.database);
    assert!(
        reopened
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    assert!(
        reopened
            .claim_next_agent_assignment("alpha", "replacement")
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        f.output.try_recv(),
        Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn canonical_binding_rejects_foreign_scope_missing_paths_permissions_and_redirected_ancestors() {
    use std::os::unix::fs::symlink;
    let mut f = Fixture::new("model");
    let authority = DaemonAuthority::new().unwrap();
    assert_eq!(
        authority
            .prepare_directory(&f.store, "test", &f.launch)
            .unwrap(),
        f.directory
    );
    let relative = f.directory.strip_prefix(&f.root).unwrap();
    assert_eq!(
        relative.parent().unwrap(),
        Path::new(".forge/assignment-runtimes")
    );
    assert_eq!(relative.file_name().unwrap().len(), 64);
    assert!(
        authority
            .prepare_directory(&f.store, "foreign", &f.launch)
            .is_err()
    );
    let mut changed = f.launch.clone();
    changed.store_id = "../../escape".into();
    assert!(
        authority
            .prepare_directory(&f.store, "test", &changed)
            .is_err()
    );
    assert!(!f.root.join("escape").exists());
    fs::set_permissions(&f.directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        authority
            .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
            .is_err()
    );
    fs::set_permissions(&f.directory, fs::Permissions::from_mode(0o700)).unwrap();
    let saved = f.root.join("saved-runtime");
    fs::rename(&f.directory, &saved).unwrap();
    assert!(
        authority
            .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
            .is_err()
    );
    assert!(!f.directory.exists());
    fs::rename(&saved, &f.directory).unwrap();
    let parent = f.directory.parent().unwrap();
    let saved = f.root.join("saved-runtimes");
    fs::rename(parent, &saved).unwrap();
    symlink(&saved, parent).unwrap();
    assert!(
        authority
            .prepare_directory(&f.store, "test", &f.launch)
            .is_err()
    );
    assert!(
        authority
            .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
            .is_err()
    );
    fs::remove_file(parent).unwrap();
    fs::rename(saved, parent).unwrap();
    assert!(
        f.child.stdin.is_some(),
        "rejected mappings must not take/deliver the control pipe"
    );
}

#[test]
fn committed_model_authority_expires_during_renewal_outage_with_retained_capacity() {
    outage("model");
}

#[test]
fn committed_tool_authority_cancels_real_child_during_outage_with_retained_capacity() {
    outage("child");
}

#[test]
fn refused_cancellation_persists_uncertainty_and_reserves_closed_assignment_capacity() {
    let mut f = Fixture::new("model");
    let authority = DaemonAuthority::new().unwrap();
    let mut service = authority
        .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
        .unwrap();
    service.tick(&f.store).unwrap();
    event(&f.output, "authorized");
    event(&f.output, "started");
    let mut changed = f.evidence.clone();
    changed["worker_start"] = "unverified".into();
    fs::write(
        f.directory.join("owner.json"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    assert!(service.tick(&open(&f.database)).is_err());
    diagnostic(&f.errors, "runner-watchdog-kill-unverified");
    let pid = f.evidence["worker_pid"].as_u64().unwrap();
    let info = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    assert_eq!(
        info.rsplit_once(')').unwrap().1.split_whitespace().nth(19),
        f.evidence["worker_start"].as_str()
    );
    let reopened = open(&f.database);
    reopened
        .close_agent_assignment(
            "assignment",
            forge_app::AgentAssignmentClosureReason::Crashed,
        )
        .unwrap();
    assert!(
        open(&f.database)
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    assert!(
        open(&f.database)
            .list_agent_assignment_resources("assignment")
            .unwrap()
            .iter()
            .any(
                |resource| resource.resource_kind == "authorization-uncertain"
                    && resource.retired_at_millis.is_none()
            )
    );
    assert!(
        open(&f.database)
            .claim_next_agent_assignment("alpha", "replacement")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(f.directory.join("watchdog.json")).unwrap())
            .unwrap()["cleanup"],
        "unverified"
    );
    // T25: an unverified KILL never yields quiescence proof. The host keeps the
    // lock and records a specific diagnostic; even restored evidence cannot help.
    diagnostic(&f.errors, "runner-watchdog-quiescence-unproven");
    fs::write(
        f.directory.join("owner.json"),
        serde_json::to_vec(&f.evidence).unwrap(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert_no_fence_proof(&f.directory);
    drop(service);
    assert!(f.child.try_wait().unwrap().is_none());
}

/// No receipt, a retained lock and an `Unproven` diagnosis.
fn assert_no_fence_proof(directory: &Path) {
    use forge_runner_host::fence::Diagnosis;
    assert!(!fs::read_dir(directory).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("watchdog-receipt-")
    }));
    assert!(fs::read_dir(directory).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with("-quiescence-unproven.json")
    }));
    assert!(forge_runner_host::inspection::ExclusiveInspection::acquire(directory).is_err());
    assert_eq!(Diagnosis::of(directory), Diagnosis::Unproven);
}

/// T25: lease expiry with an escaped (detached, own process group) descendant.
/// The group KILL cannot reach it, so the subreaper never observes ECHILD: no
/// receipt, the hold and lock are kept and a specific diagnostic is written.
/// Once the escaped child really exits, kernel proof follows and the host exits.
#[test]
fn escaped_descendant_keeps_the_fence_hold_until_it_really_exits() {
    let mut f = Fixture::new("retire-escaped");
    let authority = DaemonAuthority::new().unwrap();
    let mut service = authority
        .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
        .unwrap();
    let Tick::Delivered { lease, .. } = service.tick(&f.store).unwrap() else {
        panic!("initial grant was not delivered")
    };
    let lease = forge_app::authorization_lease::Lease::clone(&lease);
    event(&f.output, "authorized");
    let escaped = event(&f.output, "started")["child"].as_u64().unwrap();
    // Stop renewing but keep the pipe open: closing it would let the worker
    // stop its own child. The grant expires at its absolute deadline.
    diagnostic(&f.errors, "runner-lease-expired");
    diagnostic(&f.errors, "runner-watchdog-kill-sent");
    diagnostic(&f.errors, "runner-watchdog-quiescence-unproven");
    assert_no_fence_proof(&f.directory);
    drop(service);
    assert!(f.child.try_wait().unwrap().is_none());
    assert!(forge_runner_host::fence::receipt(&f.directory, &lease).is_err());
    // Test controller only: the escaped child is this fixture's own process.
    let pid = rustix::process::Pid::from_raw(i32::try_from(escaped).unwrap()).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::KILL).unwrap();
    diagnostic(&f.errors, "runner-watchdog-quiescence-verified");
    assert!(f.child.wait().unwrap().success());
    let _guard = forge_runner_host::inspection::ExclusiveInspection::acquire(&f.directory).unwrap();
    forge_runner_host::fence::receipt(&f.directory, &lease).unwrap();
    forge_runner_host::fence::settled(&f.directory).unwrap();
}

#[test]
fn revoked_assignment_and_already_stopped_driver_never_deliver_initial_permission() {
    for revoked in [true, false] {
        let mut f = Fixture::new("model");
        if revoked {
            f.store
                .close_agent_assignment(
                    "assignment",
                    forge_app::AgentAssignmentClosureReason::Retired,
                )
                .unwrap();
        }
        let authority = DaemonAuthority::new().unwrap();
        let mut service = authority
            .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
            .unwrap();
        if revoked {
            assert!(service.tick(&f.store).is_err());
        } else {
            let (sender, receiver) = mpsc::channel();
            drop(sender);
            service.run(&f.store, &receiver).unwrap();
        }
        drop(service);
        // Either path ends in the host's own pre-grant quiescence proof (T26).
        diagnostic_any(&f.errors, &["runner-worker-exited", "runner-issuer-lost"]);
        assert!(f.child.wait().unwrap().success());
        assert!(forge_runner_host::fence::ungranted_receipt(&f.directory).is_ok());
        assert!(!f.directory.join("controlled.sqlite").exists());
        let c = rusqlite::Connection::open(&f.database).unwrap();
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM assignment_authorization_leases",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert!(
            open(&f.database)
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
        assert!(open(&f.database).dispatch_hold(f.record).unwrap().is_some());
    }
}

#[test]
fn failed_binding_retains_scope_bound_pipe_or_owner_uncertainty_before_any_grant() {
    for missing_pipe in [false, true] {
        let mut f = Fixture::new("model");
        if missing_pipe {
            drop(f.child.stdin.take());
        } else {
            let mut changed = f.evidence.clone();
            changed["worker_start"] = "unverified".into();
            fs::write(
                f.directory.join("owner.json"),
                serde_json::to_vec(&changed).unwrap(),
            )
            .unwrap();
        }
        assert!(
            DaemonAuthority::new()
                .unwrap()
                .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
                .is_err()
        );
        diagnostic_any(&f.errors, &["runner-worker-exited", "runner-issuer-lost"]);
        let reopened = open(&f.database);
        assert!(reopened.dispatch_hold(f.record).unwrap().is_some());
        assert!(
            reopened
                .list_agent_assignment_resources("assignment")
                .unwrap()
                .iter()
                .any(
                    |resource| resource.resource_kind == "authorization-uncertain"
                        && resource.resource_id
                            == if missing_pipe {
                                "authorization-delivery-uncertain"
                            } else {
                                "authorization-unavailable"
                            }
                )
        );
        assert!(!f.directory.join("controlled.sqlite").exists());
        assert!(
            reopened
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
    }
}

#[test]
fn dedicated_driver_renews_committed_history_then_stops_without_releasing_capacity() {
    let mut f = Fixture::new("model");
    let authority = DaemonAuthority::new().unwrap();
    let mut service = authority
        .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
        .unwrap();
    let (stop, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        let store = &f.store;
        let service = &mut service;
        let task = scope.spawn(move || service.run(store, &receiver));
        assert_eq!(event(&f.output, "authorized")["sequence"], 1);
        event(&f.output, "started");
        assert_eq!(event(&f.output, "authorized")["sequence"], 2);
        stop.send(()).unwrap();
        task.join().unwrap().unwrap();
    });
    drop(service);
    let reopened = open(&f.database);
    assert!(reopened.dispatch_hold(f.record).unwrap().is_some());
    assert!(
        reopened
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    let c = rusqlite::Connection::open(&f.database).unwrap();
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM assignment_authorization_leases",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
}

/// T26: a host whose private pipe closes before any grant (its launching
/// daemon failed or died) fences, stops its worker, proves quiescence as the
/// original subreaper and writes an exact ungranted receipt before exiting.
#[test]
fn lost_issuer_before_any_grant_proves_ungranted_quiescence_and_releases_ownership() {
    use forge_runner_host::fence::{self, Diagnosis};
    let mut f = Fixture::new("model");
    drop(f.child.stdin.take());
    diagnostic(&f.errors, "runner-watchdog-quiescence-verified");
    assert!(f.child.wait().unwrap().success());
    let worker = f.evidence["worker_pid"].as_u64().unwrap();
    assert!(!Path::new(&format!("/proc/{worker}")).exists());
    let guard = forge_runner_host::inspection::ExclusiveInspection::acquire(&f.directory).unwrap();
    let receipt = fence::ungranted_receipt(&f.directory).unwrap();
    assert_eq!(receipt.marker, "watchdog.json");
    assert!(receipt.children_absent);
    assert_eq!(
        u64::from(receipt.owner.host_pid),
        f.evidence["owner"]["host_pid"].as_u64().unwrap()
    );
    let marker: Value =
        serde_json::from_slice(&fs::read(f.directory.join("watchdog.json")).unwrap()).unwrap();
    assert_eq!(marker["code"], "runner-issuer-lost");
    assert!(marker["last_lease"].is_null());
    fence::settled(&f.directory).unwrap();
    assert_eq!(Diagnosis::of(&f.directory), Diagnosis::Proven);
    // Owned directories are never sealed as never owned.
    assert!(forge_runner_host::seal::seal(&f.directory).is_err());
    drop(guard);
    // Tampered marker bytes or owner evidence are not proof.
    let marker_path = f.directory.join("watchdog.json");
    let bytes = fs::read(&marker_path).unwrap();
    fs::write(&marker_path, [bytes.as_slice(), b" "].concat()).unwrap();
    assert!(fence::ungranted_receipt(&f.directory).is_err());
    assert!(fence::settled(&f.directory).is_err());
    fs::write(&marker_path, &bytes).unwrap();
    let owner_path = f.directory.join("owner.json");
    let owner = fs::read(&owner_path).unwrap();
    let mut changed: Value = serde_json::from_slice(&owner).unwrap();
    changed["worker_start"] = "other".into();
    fs::write(&owner_path, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(fence::ungranted_receipt(&f.directory).is_err());
    fs::write(&owner_path, &owner).unwrap();
    fence::ungranted_receipt(&f.directory).unwrap();
}

/// T26: a directory sealed as never owned is never owned afterwards.
#[test]
fn sealed_runtime_directory_refuses_every_later_host() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let directory = root.join("runtime");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let worktree = root.join("worktree");
    fs::create_dir(&worktree).unwrap();
    drop(forge_runner_host::seal::seal(&directory).unwrap());
    let (mut child, _output, _errors) = spawn(&directory, &worktree);
    assert!(!child.wait().unwrap().success());
    assert!(!directory.join("owner.json").exists());
    assert!(forge_runner_host::seal::sealed(&directory).unwrap());
}
