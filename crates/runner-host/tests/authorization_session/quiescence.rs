//! Real retained recovery claim -> private control -> original subreaper proof.
//! Does not implement a `RecoveryHost`, ownership handoff or writable resume.
use super::*;
use forge_app::authorization_lease::{AuthorityHost as _, Repository as _, session::Delivery as _};
use forge_app::durable_execution::{
    Admission,
    recovery::{Claim, Repository as _, quiescence::Operation},
};
use forge_runner_host::{control::ControlPipe, inspection::ExclusiveInspection};

fn prepare(f: &mut Fixture) -> (ControlPipe, Operation, Value) {
    prepare_admission(f, None)
}

pub(super) fn prepare_admission(
    f: &mut Fixture,
    runtime_mode: Option<&str>,
) -> (ControlPipe, Operation, Value) {
    let worktree = f.root.join("worktree");
    prepare_admission_at(f, runtime_mode, &worktree)
}

pub(super) fn prepare_admission_at(
    f: &mut Fixture,
    runtime_mode: Option<&str>,
    worktree: &Path,
) -> (ControlPipe, Operation, Value) {
    let authority = DaemonAuthority::new().unwrap();
    let mut service = authority
        .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
        .unwrap();
    service.tick(&f.store).unwrap();
    event(&f.output, "authorized");
    let started = event(&f.output, "started");
    let pipe = service.retirement_control();
    drop(service);
    if let Some(mode) = runtime_mode {
        super::recovery_inspection::seed(f, mode);
    } else {
        f.store.begin_send("test", &f.launch).unwrap();
        f.store
            .record_result(
                "test",
                &f.launch,
                &Admission::Accepted {
                    admission_id: "original-input".into(),
                },
            )
            .unwrap();
    }
    let c = rusqlite::Connection::open(&f.database).unwrap();
    let worktree = worktree.to_str().unwrap().to_owned();
    c.execute(
        "INSERT INTO assignment_execution_bindings VALUES ('assignment', ?1)",
        [&worktree],
    )
    .unwrap();
    c.execute("INSERT INTO worktree_claims SELECT record_id, project_slug, ?1, 'active' FROM agent_assignments WHERE id = 'assignment'", [&worktree]).unwrap();
    drop(c);
    let candidate = f.store.recovery_candidate("test", &f.launch).unwrap();
    let claim =
        Claim::for_candidate(&candidate, "recover-original".into(), "new-daemon".into()).unwrap();
    f.store.claim_recovery("test", &claim).unwrap();
    let mut changed_candidate = candidate.clone();
    changed_candidate.execution_directory.push_str("-foreign");
    assert!(Operation::for_candidate(&changed_candidate, claim.clone()).is_err());
    let operation = Operation::for_candidate(&candidate, claim).unwrap();
    (pipe, operation, started)
}

#[test]
fn recovery_quiescence_proves_owned_descendants_without_revoking_or_rebinding_assignment() {
    let mut f = Fixture::new("retire-delayed");
    let work = f.root.join("worktree/dirty-progress");
    let runtime = f.directory.join("runtime.sqlite");
    fs::write(&work, "uncommitted bytes").unwrap();
    let (mut pipe, operation, _) = prepare(&mut f);
    fs::write(&runtime, "retained runtime sentinel, never opened").unwrap();
    let before = open(&f.database)
        .agent_assignment_info("assignment")
        .unwrap();
    let owner = fs::read(f.directory.join("owner.json")).unwrap();
    let resources = f
        .store
        .list_agent_assignment_resources("assignment")
        .unwrap();
    pipe.quiesce(&operation).unwrap();
    diagnostic(&f.errors, "runner-quiescence-verified");
    assert!(f.child.wait().unwrap().success());
    let guard = ExclusiveInspection::acquire(&f.directory).unwrap();
    let receipt = forge_runner_host::quiescence::receipt(&f.directory, &operation).unwrap();
    assert!(receipt.children_absent);
    for _ in 0..3 {
        assert_eq!(
            forge_runner_host::quiescence::receipt(&f.directory, &operation).unwrap(),
            receipt
        );
    }
    assert_eq!(
        open(&f.database)
            .agent_assignment_info("assignment")
            .unwrap(),
        before
    );
    assert!(before.capacity_reserved());
    assert!(before.assignment.closed_at_millis.is_none());
    assert_eq!(
        f.store
            .list_agent_assignment_resources("assignment")
            .unwrap(),
        resources
    );
    assert_eq!(fs::read(f.directory.join("owner.json")).unwrap(), owner);
    assert_eq!(fs::read_to_string(work).unwrap(), "uncommitted bytes");
    assert_eq!(
        fs::read_to_string(runtime).unwrap(),
        "retained runtime sentinel, never opened"
    );
    assert!(!f.directory.join("retirement-intent.json").exists());
    assert!(!f.directory.join("retirement-receipt.json").exists());
    // Proof alone cannot renew old authority, remove the claim or authorize a
    // replacement. Ordinary restart must still fail despite an available flock.
    let mut renewal = operation.predecessor.clone();
    renewal.sequence += 1;
    renewal.issued_boottime_millis += 10_000;
    renewal.expires_boottime_millis += 10_000;
    assert!(
        f.store
            .grant_lease("test", &renewal, renewal.issued_boottime_millis)
            .is_err()
    );
    guard.verify().unwrap();
    drop(guard);

    // Even a syntactically valid new-owner sequence-one control cannot turn the
    // retained shutdown intent into ordinary startup permission.
    let (child, output, errors) = spawn(&f.directory, &f.root.join("worktree"));
    f.child = child;
    f.output = output;
    f.errors = errors;
    f.evidence = event(&f.output, "owned")["identity"].clone();
    let mut restarted_pipe = ControlPipe::take(&mut f.child).unwrap();
    let observation = forge_runner_host::SpawnedOwner::capture(&mut f.child, &f.directory)
        .unwrap()
        .observe()
        .unwrap();
    let mut forged_start = operation.predecessor.clone();
    forged_start.owner = observation.owner;
    forged_start.sequence = 1;
    forged_start.daemon_epoch = "new-uncommitted-control".into();
    forged_start.issued_boottime_millis = observation.boottime_millis;
    forged_start.expires_boottime_millis = observation.boottime_millis + 30_000;
    restarted_pipe.deliver(&forged_start).unwrap();
    diagnostic(&f.errors, "runner-host-input-failed");
    assert_eq!(
        forge_runner_host::quiescence::receipt(&f.directory, &operation).unwrap(),
        receipt
    );
    assert!(
        !f.output
            .try_iter()
            .any(|line| line.contains("authorized") || line.contains("started"))
    );
}

#[test]
fn recovery_quiescence_rejects_foreign_original_grant_without_signalling_or_producing_proof() {
    let mut f = Fixture::new("retire-delayed");
    let (mut pipe, mut operation, _) = prepare(&mut f);
    operation.predecessor.owner.worker_start.push('x');
    pipe.quiesce(&operation).unwrap();
    diagnostic(&f.errors, "runner-quiescence-unverified");
    assert!(f.child.try_wait().unwrap().is_none());
    assert!(!f.directory.join("recovery-quiescence-intent.json").exists());
    assert!(
        !f.directory
            .join("recovery-quiescence-receipt.json")
            .exists()
    );
    assert!(ExclusiveInspection::acquire(&f.directory).is_err());
    assert!(
        open(&f.database)
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}

// Test-owned descendant cleanup; never product PID adoption or signal retry.
struct Escaped {
    pid: u32,
    start: String,
}
impl Drop for Escaped {
    fn drop(&mut self) {
        if let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", self.pid))
            && stat
                .rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().nth(19))
                == Some(self.start.as_str())
            && let Some(pid) = rustix::process::Pid::from_raw(self.pid.cast_signed())
        {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
        }
    }
}

#[test]
fn escaped_child_retains_claim_capacity_and_ownership_without_receipt_or_signal_retry() {
    let mut f = Fixture::new("retire-escaped");
    let (mut pipe, operation, started) = prepare(&mut f);
    let pid = u32::try_from(started["child"].as_u64().unwrap()).unwrap();
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let escaped = Escaped {
        pid,
        start: stat
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .nth(19)
            .unwrap()
            .into(),
    };
    pipe.quiesce(&operation).unwrap();
    diagnostic(&f.errors, "runner-quiescence-unverified");
    assert!(f.child.try_wait().unwrap().is_none());
    assert!(f.directory.join("recovery-quiescence-intent.json").exists());
    assert!(
        !f.directory
            .join("recovery-quiescence-receipt.json")
            .exists()
    );
    assert!(ExclusiveInspection::acquire(&f.directory).is_err());
    let before = open(&f.database)
        .agent_assignment_info("assignment")
        .unwrap();
    assert!(before.capacity_reserved());
    assert!(before.assignment.closed_at_millis.is_none());
    // An identical stop request after incomplete cleanup cannot initiate another
    // signal attempt or manufacture proof. The original host remains fenced.
    pipe.quiesce(&operation).unwrap();
    assert_eq!(
        open(&f.database)
            .agent_assignment_info("assignment")
            .unwrap(),
        before
    );
    let retained = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    assert_eq!(
        retained
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .nth(19),
        Some(escaped.start.as_str())
    );
    assert!(
        !f.directory
            .join("recovery-quiescence-receipt.json")
            .exists()
    );
}

#[test]
fn conflicting_intent_or_receipt_never_gets_repaired_or_releases_ownership() {
    for name in [
        "recovery-quiescence-intent.json",
        "recovery-quiescence-receipt.json",
    ] {
        let mut f = Fixture::new("retire-delayed");
        let (mut pipe, operation, _) = prepare(&mut f);
        let path = f.directory.join(name);
        fs::write(&path, "conflicting protected evidence").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        pipe.quiesce(&operation).unwrap();
        diagnostic(&f.errors, "runner-quiescence-unverified");
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "conflicting protected evidence"
        );
        assert!(f.child.try_wait().unwrap().is_none());
        assert!(ExclusiveInspection::acquire(&f.directory).is_err());
        assert!(
            open(&f.database)
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
        assert!(!f.directory.join("retirement-receipt.json").exists());
        if name == "recovery-quiescence-intent.json" {
            let pid = f.evidence["worker_pid"].as_u64().unwrap();
            assert!(fs::read_to_string(format!("/proc/{pid}/stat")).is_ok());
        }
    }
}

#[test]
fn protected_recovery_proof_is_exact_and_cannot_be_reinterpreted_as_retirement() {
    let mut f = Fixture::new("retire-delayed");
    let (mut pipe, operation, _) = prepare(&mut f);
    pipe.quiesce(&operation).unwrap();
    diagnostic(&f.errors, "runner-quiescence-verified");
    f.child.wait().unwrap();
    let _guard = ExclusiveInspection::acquire(&f.directory).unwrap();
    let path = f.directory.join("recovery-quiescence-receipt.json");
    let bytes = fs::read(&path).unwrap();
    let mut foreign = operation.clone();
    foreign.claim.operation_id.push('x');
    assert!(forge_runner_host::quiescence::receipt(&f.directory, &foreign).is_err());
    for malformed in [b"{}".to_vec(), vec![b'x'; 65_537], {
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value["children_absent"] = false.into();
        serde_json::to_vec(&value).unwrap()
    }] {
        fs::write(&path, malformed).unwrap();
        assert!(forge_runner_host::quiescence::receipt(&f.directory, &operation).is_err());
    }
    fs::write(&path, &bytes).unwrap();
    let intent = f.directory.join("recovery-quiescence-intent.json");
    fs::write(&intent, b"{}").unwrap();
    assert!(forge_runner_host::quiescence::receipt(&f.directory, &operation).is_err());
}
