//! Actual executable supervisor/Node recovery preparation and grant reception.
//! External Git policy/protected digest verification are explicitly synthetic;
//! no writable runtime reopen, bootstrap, model or tool resumes in this tracer.
use super::*;
use forge_app::{
    RegistrationError,
    authorization_lease::{AuthorityHost, Observation},
    durable_execution::recovery::{
        Candidate, Claim, Repository as _,
        authorization::{Prepared, RecoveryHost, Verification},
        session::{Delivery as _, Session},
    },
};
use forge_runner_host::control::ControlPipe;
use std::io::Write;

struct SyntheticVerification(Option<Observation>);
impl RecoveryHost for SyntheticVerification {
    fn verify(&mut self, _: &Candidate, _: &Claim) -> Result<Verification, RegistrationError> {
        Ok(Verification {
            observation: self.0.take().unwrap(),
            // Deliberately synthetic: this is NOT protected Git/proof persistence.
            evidence_sha256: "a".repeat(64),
        })
    }
}

fn waiting(
    mode: &str,
) -> (
    Fixture,
    forge_app::durable_execution::recovery::quiescence::Operation,
) {
    let runner = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../runner/test/synthetic-runner.ts")
        .canonicalize()
        .unwrap();
    waiting_recovery_inspection::waiting_runner(mode, Some(&runner))
}

fn prepared(f: &mut Fixture, claim: &Claim) -> Prepared {
    let candidate = f.store.claimed_recovery_candidate("test", claim).unwrap();
    let observed = forge_runner_host::SpawnedOwner::capture(&mut f.child, &f.directory)
        .unwrap()
        .observe()
        .unwrap();
    Prepared::verify(
        candidate,
        claim.clone(),
        &mut SyntheticVerification(Some(observed)),
    )
    .unwrap()
}

fn lease_count(f: &Fixture) -> i64 {
    lease_count_in(&f.database)
}

fn lease_count_in(database: &Path) -> i64 {
    rusqlite::Connection::open(database)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM assignment_authorization_leases",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn executable_recovery_receiver_prepares_without_authority_then_accepts_exact_committed_envelope() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = quiesced_inspection::runtime();
    let (mut f, operation) = waiting("placed");
    let mut pipe = ControlPipe::take(&mut f.child).unwrap();
    let before = recovery_inspection::runtime_bytes(&f.directory);
    let original_attempt = f.store.assignment_check_attempt("assignment").unwrap();
    let mut inspected = runtime
        .block_on(
            forge_server::durable_authorization::waiting_recovery_inspection::inspect(
                &f.store,
                "test",
                &operation.claim,
                &mut f.child,
                &components.node,
                &inspector,
            ),
        )
        .unwrap();
    let observation = inspected.observe(&f.store, "test").unwrap();
    let prepared = Prepared::verify(
        inspected.candidate().clone(),
        operation.claim,
        &mut SyntheticVerification(Some(observation)),
    )
    .unwrap();
    pipe.prepare_recovery(&prepared).unwrap();
    event(&f.output, "recovery-prepared");
    // Exact preparation retry is unarmed and cannot replace the deadline.
    pipe.prepare_recovery(&prepared).unwrap();
    assert_eq!(lease_count_in(&f.database), 1);
    assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
    assert!(
        !f.output
            .try_iter()
            .any(|line| line.contains("authorized") || line.contains("settled"))
    );
    let expected = prepared.grant().clone();
    let mut session = Session::new("test".into(), prepared).unwrap();
    let Tick::Delivered { lease, .. } = session.tick(&f.store, &mut inspected, &mut pipe).unwrap()
    else {
        panic!("recovery grant not delivered");
    };
    assert_eq!(*lease, expected.lease);
    assert_eq!(event(&f.output, "authorized")["sequence"], lease.sequence);
    assert_eq!(lease_count_in(&f.database), 2);
    assert_eq!(
        f.store.assignment_check_attempt("assignment").unwrap(),
        original_attempt
    );
    assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
    // Exact live recovery delivery retry cannot submit another input or extend
    // authority. No Harness is opened by this receiver-only entrypoint.
    pipe.deliver_recovery(&expected).unwrap();
    assert_eq!(event(&f.output, "authorized")["sequence"], lease.sequence);
    assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
    assert!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    drop(inspected);
}

#[test]
fn executable_prepared_receiver_stays_unarmed_when_forge_revokes_before_commit() {
    let (mut f, operation) = waiting("queued");
    let prepared = prepared(&mut f, &operation.claim);
    let mut pipe = ControlPipe::take(&mut f.child).unwrap();
    let before = recovery_inspection::runtime_bytes(&f.directory);
    pipe.prepare_recovery(&prepared).unwrap();
    event(&f.output, "recovery-prepared");
    f.store
        .close_agent_assignment(
            "assignment",
            forge_app::AgentAssignmentClosureReason::Retired,
        )
        .unwrap();
    let mut host = forge_runner_host::SpawnedOwner::capture(&mut f.child, &f.directory).unwrap();
    let mut session = Session::new("test".into(), prepared).unwrap();
    assert!(session.tick(&f.store, &mut host, &mut pipe).is_err());
    assert!(session.tick(&f.store, &mut host, &mut pipe).is_err());
    assert_eq!(lease_count_in(&f.database), 1);
    assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
    assert!(
        !f.output
            .try_iter()
            .any(|line| line.contains("authorized") || line.contains("settled"))
    );
    assert!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}

#[test]
fn executable_recovery_receiver_rejects_unprepared_grants_and_ordinary_first_controls() {
    use forge_app::authorization_lease::session::Delivery as _;
    for ordinary in [false, true] {
        let (mut f, operation) = waiting("queued");
        let prepared = prepared(&mut f, &operation.claim);
        let mut pipe = ControlPipe::take(&mut f.child).unwrap();
        if ordinary {
            pipe.deliver(&prepared.grant().lease).unwrap();
        } else {
            pipe.deliver_recovery(prepared.grant()).unwrap();
        }
        diagnostic(&f.errors, "runner-host-input-failed");
        assert_eq!(lease_count(&f), 1);
        assert!(
            !f.output
                .try_iter()
                .any(|line| line.contains("authorized") || line.contains("recovery-prepared"))
        );
        assert!(
            f.store
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
    }
}

#[test]
fn executable_recovery_preparation_requires_exact_predecessor_proof_and_waiting_owner() {
    for fault in 0..4 {
        let (mut f, operation) = waiting("queued");
        let prepared = prepared(&mut f, &operation.claim);
        let mut control = serde_json::to_value(
            forge_rpc::durable_execution::SupervisorRecoveryPreparation {
                host_prepare_recovery: 1,
                candidate: prepared.candidate().clone(),
                grant: prepared.grant().clone(),
            },
        )
        .unwrap();
        match fault {
            0 => fs::remove_file(f.directory.join("recovery-quiescence-receipt.json")).unwrap(),
            1 => control["grant"]["lease"]["owner"]["worker_start"] = "foreign".into(),
            2 => control["candidate"]["attempt"]["body"] = "PRIVATE-CANARY changed".into(),
            3 => control["host_prepare_recovery"] = 2.into(),
            _ => unreachable!(),
        }
        writeln!(f.child.stdin.as_mut().unwrap(), "{control}").unwrap();
        diagnostic(&f.errors, "runner-host-input-failed");
        assert_eq!(lease_count(&f), 1);
        assert!(!f.output.try_iter().any(|line| line.contains("authorized")
            || line.contains("recovery-prepared")
            || line.contains("PRIVATE-CANARY")));
        assert!(
            f.store
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
    }
}

#[test]
fn executable_unarmed_owner_fault_stays_fenced_after_owner_evidence_is_restored() {
    let (mut f, operation) = waiting("queued");
    let original = fs::read(f.directory.join("owner.json")).unwrap();
    let before = recovery_inspection::runtime_bytes(&f.directory);
    fs::write(f.directory.join("owner.json"), "{}").unwrap();
    diagnostic(&f.errors, "runner-owner-uncertain");
    fs::write(f.directory.join("owner.json"), original).unwrap();
    // Fresh descriptive capture after restoration is not the host's retained
    // safety state. Even synthetic external verification cannot waive its marker.
    let prepared = prepared(&mut f, &operation.claim);
    let mut pipe = ControlPipe::take(&mut f.child).unwrap();
    pipe.prepare_recovery(&prepared).unwrap();
    diagnostic(&f.errors, "runner-host-input-failed");
    assert_eq!(lease_count(&f), 1);
    assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
    assert!(
        !f.output
            .try_iter()
            .any(|line| line.contains("authorized") || line.contains("recovery-prepared"))
    );
    assert!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}

#[test]
fn executable_node_preparation_rejects_done_input_and_effect_uncertainty_before_any_grant() {
    for mode in ["done", "interrupted"] {
        let (mut f, operation) = waiting(mode);
        // Deliberately bypass the separate waiting inspector with synthetic
        // verification: the executable consumer must independently refuse this.
        let prepared = prepared(&mut f, &operation.claim);
        let mut pipe = ControlPipe::take(&mut f.child).unwrap();
        let before = recovery_inspection::runtime_bytes(&f.directory);
        pipe.prepare_recovery(&prepared).unwrap();
        let response = f.output.recv_timeout(Duration::from_secs(20)).unwrap();
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["error"]["code"], "assignment-lease-denied");
        assert_eq!(lease_count(&f), 1);
        assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
        assert!(
            f.store
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
    }
}

#[test]
fn executable_recovery_delivery_rechecks_proof_and_never_accepts_a_replaced_expectation() {
    for fault in 0..2 {
        let (mut f, operation) = waiting("queued");
        let prepared = prepared(&mut f, &operation.claim);
        let mut pipe = ControlPipe::take(&mut f.child).unwrap();
        pipe.prepare_recovery(&prepared).unwrap();
        event(&f.output, "recovery-prepared");
        if fault == 0 {
            fs::remove_file(f.directory.join("recovery-quiescence-receipt.json")).unwrap();
            pipe.deliver_recovery(prepared.grant()).unwrap();
        } else {
            let mut conflicting = prepared.grant().clone();
            conflicting.evidence_sha256 = "c".repeat(64);
            pipe.deliver_recovery(&conflicting).unwrap();
        }
        diagnostic(&f.errors, "runner-host-input-failed");
        assert_eq!(lease_count(&f), 1);
        assert!(!f.output.try_iter().any(|line| line.contains("authorized")));
        assert!(
            f.store
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
    }
}
