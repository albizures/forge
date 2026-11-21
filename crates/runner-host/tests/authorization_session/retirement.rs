//! T09 real app/store/operator adapter -> private pipe -> subreaper process tests.
use super::*;
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentProfile,
    Application, RepositoryInspector,
    durable_execution::{Admission, retirement::Repository as _},
};
use forge_server::durable_authorization::retirement::{Controller, Factory};
use std::sync::Arc;

struct NoHerdr;
impl AgentDispatchAdapterFactory for NoHerdr {
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        panic!("durable retirement must not fall back to Herdr")
    }
}
struct NoGit;
impl RepositoryInspector for NoGit {
    fn inspect(&self, _: &Path) -> Result<Project, forge_app::RegistrationError> {
        panic!("retirement must preserve Git")
    }
}
fn app(f: &Fixture, controller: Arc<Controller>) -> Application {
    Application::with_agent_dispatch(
        Box::new(open(&f.database)),
        Box::new(open(&f.database)),
        Box::new(Factory {
            database: f.database.clone(),
            controller,
            recovery: None,
            original: Box::new(NoHerdr),
        }),
        Box::new(NoGit),
    )
}

// Own test cleanup evidence independently of the product registry. A failed
// retirement intentionally leaves its host/descendants running and fenced.
struct Cleanup {
    pid: u32,
    start: String,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", self.pid))
            && stat
                .rsplit_once(')')
                .and_then(|(_, r)| r.split_whitespace().nth(19))
                == Some(&self.start)
            && let Some(pid) = rustix::process::Pid::from_raw(self.pid.cast_signed())
        {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            let _ = rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::empty());
        }
    }
}
// Protocol-fake effect observation for the session fixture's deliberately
// noncanonical controlled.sqlite. Real canonical inspection is covered below
// and by the actual Check-dispatch/CLI tracer, never inferred from this fake.
fn inspection_controller(f: &Fixture) -> Arc<Controller> {
    let inspector = f.root.join("controlled-retirement-inspector.py");
    if !inspector.exists() {
        fs::write(&inspector, "import sys,json\nr=json.load(sys.stdin)\nprint(json.dumps({'protocol':1,'launch':r['launch'],'effects':[]}))\n").unwrap();
    }
    Arc::new(
        Controller::default()
            .with_inspection(
                forge_server::durable_authorization::operator::InspectionComponents {
                    node: PathBuf::from("/usr/bin/python3"),
                    inspector,
                },
            )
            .unwrap(),
    )
}
fn handoff(f: &mut Fixture) -> (Arc<Controller>, Cleanup) {
    handoff_to(f, inspection_controller(f))
}
fn handoff_to(f: &mut Fixture, controller: Arc<Controller>) -> (Arc<Controller>, Cleanup) {
    let authority = DaemonAuthority::new().unwrap();
    let mut service = authority
        .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
        .unwrap();
    service.tick(&f.store).unwrap();
    event(&f.output, "authorized");
    event(&f.output, "started");
    let pipe = service.retirement_control();
    drop(service);
    f.store.begin_send("test", &f.launch).unwrap();
    f.store
        .record_result(
            "test",
            &f.launch,
            &Admission::Accepted {
                admission_id: "controlled".into(),
            },
        )
        .unwrap();
    let cleanup = Cleanup {
        pid: f.child.id(),
        start: f.evidence["owner"]["host_start"].as_str().unwrap().into(),
    };
    // The fixture's original Drop still owns worker/group cleanup. Substitute
    // only its directly spawned host handle while the real registry owns it.
    let mut placeholder = Command::new("/bin/true").spawn().unwrap();
    placeholder.wait().unwrap();
    let child = std::mem::replace(&mut f.child, placeholder);
    controller
        .register(&f.store, "test", f.launch.clone(), child, pipe)
        .unwrap();
    (controller, cleanup)
}

#[test]
fn operator_retires_long_running_model_and_child_only_after_revocation() {
    for mode in ["model", "child"] {
        let mut f = Fixture::new(mode);
        let runtime_sentinel = f.directory.join("preserved-history");
        let work_sentinel = f.root.join("worktree/preserved-work");
        fs::write(&runtime_sentinel, "retained runtime").unwrap();
        fs::write(&work_sentinel, "retained work").unwrap();
        let (controller, _cleanup) = handoff(&mut f);
        let retired = app(&f, controller.clone())
            .retire_agent_assignment("assignment")
            .unwrap();
        assert!(retired.closed_at_millis.is_some());
        let info = open(&f.database)
            .agent_assignment_info("assignment")
            .unwrap();
        assert!(!info.capacity_reserved());
        assert!(info.dispatch_hold.is_some());
        let retirement = f.store.retirement("test", &f.launch).unwrap().unwrap();
        assert!(retirement.reconciled_at_millis.is_some());
        assert_eq!(
            app(&f, Arc::new(Controller::default()))
                .retire_agent_assignment("assignment")
                .unwrap(),
            retired
        );
        assert_eq!(
            open(&f.database)
                .retirement("test", &f.launch)
                .unwrap()
                .unwrap(),
            retirement
        );
        assert_eq!(
            fs::read_to_string(runtime_sentinel).unwrap(),
            "retained runtime"
        );
        assert_eq!(fs::read_to_string(work_sentinel).unwrap(), "retained work");
        assert!(forge_runner_host::inspection::ExclusiveInspection::acquire(&f.directory).is_ok());
        diagnostic(&f.errors, "runner-retirement-verified");
    }
}

#[test]
fn delayed_group_shutdown_preserves_tracked_staged_untracked_and_canonical_runtime_bytes() {
    let mut f = Fixture::new("retire-delayed");
    let worktree = f.root.join("worktree");
    let home = f.root.clone();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .current_dir(&worktree)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", &home)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    git(&["init", "--quiet"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    git(&["config", "user.name", "Retirement Fixture"]);
    fs::write(worktree.join("tracked"), "base").unwrap();
    fs::write(worktree.join("staged"), "base").unwrap();
    git(&["add", "tracked", "staged"]);
    git(&["commit", "--quiet", "-m", "fixture"]);
    fs::write(worktree.join("tracked"), "dirty tracked").unwrap();
    fs::write(worktree.join("staged"), "staged bytes").unwrap();
    git(&["add", "staged"]);
    fs::write(worktree.join("untracked"), "untracked bytes").unwrap();
    let status = git(&[
        "--no-optional-locks",
        "status",
        "--porcelain=v2",
        "--branch",
    ]);
    let files = ["tracked", "staged", "untracked", ".git/index"];
    let before: Vec<_> = files
        .iter()
        .map(|p| fs::read(worktree.join(p)).unwrap())
        .collect();
    let (controller, _cleanup) = handoff(&mut f);
    let runtime = f.directory.join("runtime.sqlite");
    let c = rusqlite::Connection::open(&runtime).unwrap();
    c.execute_batch("CREATE TABLE retained(value TEXT); INSERT INTO retained VALUES ('preserve');")
        .unwrap();
    drop(c);
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o600)).unwrap();
    let runtime_before = fs::read(&runtime).unwrap();
    let owner_before = fs::read(f.directory.join("owner.json")).unwrap();
    app(&f, controller)
        .retire_agent_assignment("assignment")
        .unwrap();
    for (path, bytes) in files.iter().zip(before) {
        assert_eq!(fs::read(worktree.join(path)).unwrap(), bytes);
    }
    assert_eq!(fs::read(runtime).unwrap(), runtime_before);
    assert_eq!(
        fs::read(f.directory.join("owner.json")).unwrap(),
        owner_before
    );
    assert_eq!(
        git(&[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch"
        ]),
        status
    );
}

#[test]
fn owned_process_cleanup_scans_real_committed_intent_before_capacity_release() {
    let components = super::effect_admission::components();
    let mut f = Fixture::with_component(
        "model",
        super::effect_admission::component_hash(&components),
    );
    let controller = Arc::new(
        Controller::default()
            .with_inspection(components.clone())
            .unwrap(),
    );
    let (controller, _cleanup) = handoff_to(&mut f, controller);
    super::effect_admission::seed(&f, &components, "interrupted");
    let bytes = fs::read(f.directory.join("runtime.sqlite")).unwrap();
    let error = app(&f, controller)
        .retire_agent_assignment("assignment")
        .unwrap_err();
    assert_eq!(error.code, "assignment-retirement-unreconciled");
    assert_retained(&f);
    let info = open(&f.database)
        .agent_assignment_info("assignment")
        .unwrap();
    assert!(
        info.resources
            .iter()
            .any(|r| r.resource_kind == "interrupted-effect" && r.retired_at_millis.is_none())
    );
    assert!(
        f.store
            .retirement("test", &f.launch)
            .unwrap()
            .unwrap()
            .reconciled_at_millis
            .is_some()
    );
    assert_eq!(fs::read(f.directory.join("runtime.sqlite")).unwrap(), bytes);
    assert!(
        app(&f, Arc::new(Controller::default()))
            .retire_agent_assignment("assignment")
            .is_err()
    );
    assert_retained(&f);
}

#[test]
fn unavailable_effect_inspector_preserves_pending_proof_until_matched_read_only_retry() {
    let components = super::effect_admission::components();
    let mut f = Fixture::with_component(
        "model",
        super::effect_admission::component_hash(&components),
    );
    let mut unavailable = components.clone();
    unavailable.inspector = f.root.join("missing-inspector.ts");
    let controller = Arc::new(Controller::default().with_inspection(unavailable).unwrap());
    let (controller, _cleanup) = handoff_to(&mut f, controller);
    super::effect_admission::seed(&f, &components, "interrupted");
    assert!(
        app(&f, controller)
            .retire_agent_assignment("assignment")
            .is_err()
    );
    assert_retained(&f);
    let pending = f.store.retirement("test", &f.launch).unwrap().unwrap();
    assert!(pending.reconciled_at_millis.is_none());
    assert!(f.directory.join("retirement-receipt.json").exists());
    let retry = Arc::new(Controller::default().with_inspection(components).unwrap());
    assert_eq!(
        app(&f, retry)
            .retire_agent_assignment("assignment")
            .unwrap_err()
            .code,
        "assignment-retirement-unreconciled"
    );
    let result = f.store.retirement("test", &f.launch).unwrap().unwrap();
    assert_eq!(result.operation, pending.operation);
    assert!(result.reconciled_at_millis.is_some());
    assert_retained(&f);
}

fn assert_retained(f: &Fixture) {
    let info = open(&f.database)
        .agent_assignment_info("assignment")
        .unwrap();
    assert!(info.assignment.closed_at_millis.is_some());
    assert!(info.capacity_reserved());
    assert!(info.dispatch_hold.is_some());
    assert!(
        info.resources
            .iter()
            .any(|r| r.resource_kind == "retirement-pending" && r.retired_at_millis.is_none())
    );
}

#[test]
fn receipt_commit_failure_reconciles_original_operation_after_controller_restart() {
    let mut f = Fixture::new("retire-delayed");
    let (controller, _cleanup) = handoff(&mut f);
    let c = rusqlite::Connection::open(&f.database).unwrap();
    c.execute_batch("CREATE TRIGGER fail_retirement_commit BEFORE UPDATE ON durable_assignment_retirements BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    assert!(
        app(&f, controller)
            .retire_agent_assignment("assignment")
            .is_err()
    );
    assert_retained(&f);
    let pending = f.store.retirement("test", &f.launch).unwrap().unwrap();
    assert!(pending.reconciled_at_millis.is_none());
    assert!(f.directory.join("retirement-receipt.json").exists());
    c.execute_batch("DROP TRIGGER fail_retirement_commit")
        .unwrap();
    let receipt_path = f.directory.join("retirement-receipt.json");
    let receipt_bytes = fs::read(&receipt_path).unwrap();
    fs::write(&receipt_path, b"partial receipt").unwrap();
    assert!(
        app(&f, Arc::new(Controller::default()))
            .retire_agent_assignment("assignment")
            .is_err()
    );
    assert_retained(&f);
    fs::write(&receipt_path, &receipt_bytes).unwrap();
    let lock = f.directory.join("owner.lock");
    let saved_lock = f.directory.join("controlled-original-lock");
    fs::rename(&lock, &saved_lock).unwrap();
    fs::write(&lock, b"").unwrap();
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        app(&f, Arc::new(Controller::default()))
            .retire_agent_assignment("assignment")
            .is_err()
    );
    assert_retained(&f);
    fs::remove_file(&lock).unwrap();
    fs::rename(saved_lock, lock).unwrap();
    // No live registry, provider authentication or writable runner construction.
    app(&f, inspection_controller(&f))
        .retire_agent_assignment("assignment")
        .unwrap();
    let result = open(&f.database)
        .retirement("test", &f.launch)
        .unwrap()
        .unwrap();
    assert_eq!(result.operation, pending.operation);
    assert!(result.reconciled_at_millis.is_some());
    assert!(
        !open(&f.database)
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}

#[test]
fn intent_commit_failure_prevents_host_cancellation_and_keeps_revocation() {
    let mut f = Fixture::new("model");
    let (controller, _cleanup) = handoff(&mut f);
    let c = rusqlite::Connection::open(&f.database).unwrap();
    c.execute_batch("CREATE TRIGGER fail_retirement_intent BEFORE INSERT ON durable_assignment_retirements BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    // Keep the original registry/private pipe alive through these assertions.
    // Dropping the only controller with the temporary app can deliver EOF and
    // independently trigger legitimate worker-exit/watchdog uncertainty.
    assert!(
        app(&f, controller.clone())
            .retire_agent_assignment("assignment")
            .is_err()
    );
    assert_retained(&f);
    assert!(!f.directory.join("retirement-intent.json").exists());
    assert!(!f.directory.join("watchdog.json").exists());
}

#[test]
fn ownership_mismatch_is_not_repaired_by_restoring_evidence_on_retirement_retry() {
    let mut f = Fixture::new("model");
    let (controller, _cleanup) = handoff(&mut f);
    let original = fs::read(f.directory.join("owner.json")).unwrap();
    let mut wrong = f.evidence.clone();
    wrong["worker_start"] = "wrong-start".into();
    fs::write(f.directory.join("owner.json"), wrong.to_string()).unwrap();
    assert!(
        app(&f, controller.clone())
            .retire_agent_assignment("assignment")
            .is_err()
    );
    let pending = f.store.retirement("test", &f.launch).unwrap().unwrap();
    fs::write(f.directory.join("owner.json"), original).unwrap();
    assert!(
        app(&f, controller)
            .retire_agent_assignment("assignment")
            .is_err()
    );
    assert_eq!(
        f.store.retirement("test", &f.launch).unwrap().unwrap(),
        pending
    );
    assert_retained(&f);
    assert!(!f.directory.join("retirement-intent.json").exists());
}

#[test]
fn escaped_child_and_host_journal_write_failures_never_release_capacity() {
    for (mode, blocked) in [
        ("retire-escaped", None),
        ("retire-delayed", Some("retirement-intent.json")),
        ("retire-delayed", Some("retirement-receipt.json")),
    ] {
        let mut f = Fixture::new(mode);
        let (controller, _cleanup) = handoff(&mut f);
        let escaped = if mode == "retire-escaped" {
            let pid: u32 = fs::read_to_string(f.directory.join("controlled-escaped.pid"))
                .unwrap()
                .parse()
                .unwrap();
            let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
            Some(Cleanup {
                pid,
                start: stat
                    .rsplit_once(')')
                    .unwrap()
                    .1
                    .split_whitespace()
                    .nth(19)
                    .unwrap()
                    .into(),
            })
        } else {
            fs::create_dir(f.directory.join(blocked.unwrap())).unwrap();
            None
        };
        assert!(
            app(&f, controller)
                .retire_agent_assignment("assignment")
                .is_err()
        );
        assert_retained(&f);
        assert!(
            f.store
                .retirement("test", &f.launch)
                .unwrap()
                .unwrap()
                .reconciled_at_millis
                .is_none()
        );
        assert!(forge_runner_host::inspection::ExclusiveInspection::acquire(&f.directory).is_err());
        assert!(
            app(&f, Arc::new(Controller::default()))
                .retire_agent_assignment("assignment")
                .is_err()
        );
        assert_retained(&f);
        if let Some(escaped) = escaped {
            let stat = fs::read_to_string(format!("/proc/{}/stat", escaped.pid)).unwrap();
            assert!(!matches!(
                stat.rsplit_once(')')
                    .unwrap()
                    .1
                    .split_whitespace()
                    .next()
                    .unwrap(),
                "Z" | "X" | "x"
            ));
            drop(escaped);
        }
    }
}

#[test]
fn unknown_launch_and_missing_supervisor_keep_revocation_capacity_and_hold() {
    let f = Fixture::new("model");
    assert!(
        app(&f, Arc::new(Controller::default()))
            .retire_agent_assignment("assignment")
            .is_err()
    );
    let info = open(&f.database)
        .agent_assignment_info("assignment")
        .unwrap();
    assert!(info.assignment.closed_at_millis.is_some());
    assert!(info.capacity_reserved());
    assert!(info.dispatch_hold.is_some());
    assert!(!f.directory.join("retirement-intent.json").exists());
}
