//! Revoked reservation cancellation is not authorized writable recovery.
use super::*;
use forge_app::{
    AgentAssignmentClosureReason,
    durable_execution::retirement::{Operation, Receipt, Repository as _},
};

fn receipt(f: &Fixture) -> Receipt {
    Receipt {
        version: 1,
        operation: Operation {
            operation_id: "cancel-reserved-recovery".into(),
            launch: f.launch.clone(),
            owner: lease(f).owner,
        },
        children_absent: true,
    }
}

fn revoke_and_begin(f: &Fixture, receipt: &Receipt) {
    f.store
        .close_agent_assignment("assignment", AgentAssignmentClosureReason::Retired)
        .unwrap();
    f.store
        .begin_retirement("test", &receipt.operation)
        .unwrap();
}

#[test]
fn verified_retirement_cancels_claim_but_preserves_original_result_and_history() {
    let f = prepared();
    let request = claim(&f);
    f.store.claim_recovery("test", &request).unwrap();
    let original = f
        .store
        .list_agent_assignment_resources("assignment")
        .unwrap();
    let original = original
        .iter()
        .find(|r| r.resource_kind == "recovery-claim")
        .unwrap();
    let receipt = receipt(&f);
    revoke_and_begin(&f, &receipt);
    let result = f.reopen().reconcile_retirement("test", &receipt).unwrap();
    let info = f.reopen().agent_assignment_info("assignment").unwrap();
    assert!(!info.capacity_reserved());
    let cancelled = info
        .resources
        .iter()
        .find(|r| r.resource_kind == "recovery-claim")
        .unwrap();
    assert_eq!(cancelled.resource_id, original.resource_id);
    assert_eq!(cancelled.created_at_millis, original.created_at_millis);
    assert_eq!(cancelled.retired_at_millis, result.reconciled_at_millis);
    let accepted = snapshot(&f);
    for _ in 0..3 {
        assert_eq!(
            f.reopen().reconcile_retirement("test", &receipt).unwrap(),
            result
        );
        assert_eq!(
            f.reopen().claim_recovery("test", &request).unwrap(),
            request
        );
        assert!(candidate(&f).is_err());
        assert!(f.reopen().grant_lease("test", &lease(&f), 1_000).is_err());
        assert_eq!(snapshot(&f), accepted);
    }
    let mut changed = request.clone();
    changed.daemon_epoch.push_str("-replacement");
    assert!(f.reopen().claim_recovery("test", &changed).is_err());
    assert!(f.reopen().claim_recovery("foreign", &request).is_err());
    assert_eq!(snapshot(&f), accepted);
}

#[test]
fn cancellation_requires_revocation_scope_and_original_complete_cleanup() {
    let f = prepared();
    let request = claim(&f);
    f.store.claim_recovery("test", &request).unwrap();
    let receipt = receipt(&f);
    let live = snapshot(&f);
    assert!(
        f.store
            .begin_retirement("test", &receipt.operation)
            .is_err()
    );
    assert!(f.store.reconcile_retirement("test", &receipt).is_err());
    assert_eq!(snapshot(&f), live);
    revoke_and_begin(&f, &receipt);
    let before = snapshot(&f);
    let mut incomplete = receipt.clone();
    incomplete.children_absent = false;
    let mut wrong_owner = receipt.clone();
    wrong_owner
        .operation
        .owner
        .worker_start
        .push_str("-lookalike");
    let mut wrong_version = receipt.clone();
    wrong_version.version += 1;
    for denied in [&incomplete, &wrong_owner, &wrong_version] {
        assert!(f.reopen().reconcile_retirement("test", denied).is_err());
        assert_eq!(snapshot(&f), before);
    }
    assert!(
        f.reopen()
            .reconcile_retirement("foreign", &receipt)
            .is_err()
    );
    f.store
        .resolve_unknown_agent_resources("assignment")
        .unwrap();
    assert!(
        f.store
            .finish_agent_assignment_retirement("assignment")
            .is_err()
    );
    assert!(
        f.reopen()
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}

#[test]
fn malformed_or_foreign_recovery_claim_rolls_back_all_retirement_effects() {
    for invalid in [
        "not-json".to_owned(),
        "{}".to_owned(),
        {
            let f = prepared();
            let mut foreign = claim(&f);
            foreign.launch.runtime_id.push_str("-foreign");
            serde_json::to_string(&foreign).unwrap()
        },
        {
            let f = prepared();
            let mut malformed = claim(&f);
            malformed.candidate_sha256 = "invalid".into();
            serde_json::to_string(&malformed).unwrap()
        },
    ] {
        let f = prepared();
        let request = claim(&f);
        f.store.claim_recovery("test", &request).unwrap();
        // Corrupt storage to verify fail-closed behavior, not a public mutation.
        f.store.connection.lock().unwrap().execute(
            "UPDATE agent_assignment_resources SET resource_id = ?1 WHERE resource_kind = 'recovery-claim'",
            [invalid],
        ).unwrap();
        let receipt = receipt(&f);
        revoke_and_begin(&f, &receipt);
        let before = snapshot(&f);
        assert!(f.reopen().reconcile_retirement("test", &receipt).is_err());
        assert_eq!(snapshot(&f), before);
        assert!(
            f.reopen()
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
    }
}

#[test]
fn claim_cancellation_failure_rolls_back_receipt_and_runtime_retirement() {
    let f = prepared();
    let request = claim(&f);
    f.store.claim_recovery("test", &request).unwrap();
    let receipt = receipt(&f);
    revoke_and_begin(&f, &receipt);
    f.store.connection.lock().unwrap().execute_batch(
        "CREATE TRIGGER fail_claim_cancellation BEFORE UPDATE OF retired_at_millis ON agent_assignment_resources WHEN OLD.resource_kind = 'recovery-claim' BEGIN SELECT RAISE(ABORT, 'injected'); END;",
    ).unwrap();
    let before = snapshot(&f);
    assert!(f.reopen().reconcile_retirement("test", &receipt).is_err());
    assert_eq!(snapshot(&f), before);
    f.store
        .connection
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_claim_cancellation")
        .unwrap();
    f.reopen().reconcile_retirement("test", &receipt).unwrap();
    assert!(
        !f.reopen()
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}

#[test]
fn cancellation_preserves_unrelated_blockers_holds_and_concurrent_exact_results() {
    let f = prepared();
    let request = claim(&f);
    f.store.claim_recovery("test", &request).unwrap();
    for kind in ["interrupted-effect", "unknown"] {
        f.store.connection.lock().unwrap().execute(
            "INSERT INTO agent_assignment_resources VALUES ('assignment', ?1, 'retain', 1, NULL)", [kind],
        ).unwrap();
    }
    let record = f.store.agent_assignment("assignment").unwrap().record_id;
    f.store
        .hold_dispatch(record, "retain investigation")
        .unwrap();
    let receipt = receipt(&f);
    revoke_and_begin(&f, &receipt);
    let a = f.reopen();
    let b = f.reopen();
    let barrier = Barrier::new(2);
    std::thread::scope(|s| {
        let one = s.spawn(|| {
            barrier.wait();
            a.reconcile_retirement("test", &receipt).unwrap()
        });
        let two = s.spawn(|| {
            barrier.wait();
            b.reconcile_retirement("test", &receipt).unwrap()
        });
        assert_eq!(one.join().unwrap(), two.join().unwrap());
    });
    let info = f.reopen().agent_assignment_info("assignment").unwrap();
    assert!(info.capacity_reserved());
    assert!(info.dispatch_hold.is_some());
    for resource in info
        .resources
        .iter()
        .filter(|r| matches!(r.resource_kind.as_str(), "interrupted-effect" | "unknown"))
    {
        assert!(resource.retired_at_millis.is_none());
    }
    assert!(
        f.store
            .finish_agent_assignment_retirement("assignment")
            .is_err()
    );
}
