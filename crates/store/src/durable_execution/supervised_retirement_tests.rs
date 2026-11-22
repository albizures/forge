//! Trusted host-receipt persistence port tests; process proof is tested in host.
use super::*;
use forge_app::{
    AgentAssignmentClosureReason,
    authorization_lease::Owner,
    durable_execution::retirement::{Operation, Receipt, Repository as _},
};
pub(super) fn operation(f: &Fixture) -> Operation {
    Operation {
        operation_id: "retire".into(),
        launch: f.launch.clone(),
        owner: Owner {
            boot_id: "boot".into(),
            time_namespace: "time".into(),
            host_pid: 10,
            host_start: "10".into(),
            worker_pid: 11,
            worker_start: "11".into(),
            lock_device: "1".into(),
            lock_inode: "2".into(),
        },
    }
}
pub(super) fn revoke(f: &Fixture) {
    // Protocol-policy fixture: seed typed original grant ownership without
    // claiming real host authority. The composed host tests issue actual grants.
    let mut lease: forge_app::authorization_lease::Lease = serde_json::from_str(include_str!(
        "../../../../runner/test/authorization-lease.json"
    ))
    .unwrap();
    lease.launch = f.launch.clone();
    lease.owner = operation(f).owner;
    f.store.connection.lock().unwrap().execute("INSERT INTO assignment_authorization_leases(assignment_id, sequence, lease_json) VALUES ('assignment', 1, ?1)", [serde_json::to_string(&lease).unwrap()]).unwrap();
    f.store
        .close_agent_assignment("assignment", AgentAssignmentClosureReason::Retired)
        .unwrap();
}
#[test]
fn supervised_receipt_scope_revocation_conflict_and_exact_replay_are_atomic() {
    let f = Fixture::new();
    accept_runtime(&f);
    let op = operation(&f);
    assert_eq!(
        f.store.begin_retirement("test", &op).unwrap_err().code,
        "assignment-not-revoked"
    );
    revoke(&f);
    assert!(f.store.begin_retirement("foreign", &op).is_err());
    let original = f.store.begin_retirement("test", &op).unwrap();
    let mut wrong = op.clone();
    wrong.owner.worker_start = "different".into();
    assert!(f.store.begin_retirement("test", &wrong).is_err());
    assert!(
        f.store
            .reconcile_retirement(
                "test",
                &Receipt {
                    version: 1,
                    operation: wrong,
                    children_absent: true
                }
            )
            .is_err()
    );
    assert!(
        f.store
            .reconcile_retirement(
                "test",
                &Receipt {
                    version: 1,
                    operation: op.clone(),
                    children_absent: false
                }
            )
            .is_err()
    );
    assert_eq!(
        f.reopen().retirement("test", &f.launch).unwrap().unwrap(),
        original
    );
    let receipt = Receipt {
        version: 1,
        operation: op,
        children_absent: true,
    };
    let result = f.store.reconcile_retirement("test", &receipt).unwrap();
    assert!(result.reconciled_at_millis.is_some());
    assert!(f.store.reconcile_retirement("foreign", &receipt).is_err());
    assert_eq!(
        f.reopen().reconcile_retirement("test", &receipt).unwrap(),
        result
    );
    let c = f.store.connection.lock().unwrap();
    assert!(
        c.execute("DELETE FROM durable_assignment_retirements", [])
            .is_err()
    );
    assert!(
        c.execute(
            "UPDATE durable_assignment_retirements SET reconciled_at_millis = 0",
            []
        )
        .is_err()
    );
    assert!(
        c.execute(
            "UPDATE durable_assignment_retirements SET operation_json = '{}'",
            []
        )
        .is_err()
    );
}
#[test]
fn supervised_reconciliation_retains_effects_unknown_resources_and_hold() {
    let f = Fixture::new();
    accept_runtime(&f);
    revoke(&f);
    f.store
        .hold_dispatch(
            f.store.agent_assignment("assignment").unwrap().record_id,
            "retained hold",
        )
        .unwrap();
    for (kind, id) in [
        ("interrupted-effect", "effect"),
        ("provision-intent", "unknown"),
        ("authorization-uncertain", "unrecognized"),
        ("missing-outcome", "other-attempt"),
        ("retirement-pending", "operator"),
    ] {
        // Seed retained resource classes directly for this transaction-policy
        // fixture; real interrupted-effect admission is covered separately.
        f.store.connection.lock().unwrap().execute("INSERT INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) VALUES ('assignment', ?1, ?2, 1)", params![kind, id]).unwrap();
    }
    let op = operation(&f);
    f.store.begin_retirement("test", &op).unwrap();
    f.store
        .reconcile_retirement(
            "test",
            &Receipt {
                version: 1,
                operation: op,
                children_absent: true,
            },
        )
        .unwrap();
    let info = f.reopen().agent_assignment_info("assignment").unwrap();
    assert!(info.capacity_reserved());
    assert_eq!(
        info.dispatch_hold.as_ref().map(|h| h.reason.as_str()),
        Some("retained hold")
    );
    assert!(
        info.resources
            .iter()
            .filter(|r| r.resource_kind != "durable-runtime")
            .all(|r| r.retired_at_millis.is_none())
    );
    assert!(
        f.store
            .finish_agent_assignment_retirement("assignment")
            .is_err()
    );
}
#[test]
fn failed_resource_reconciliation_rolls_back_receipt_and_capacity_together() {
    let f = Fixture::new();
    accept_runtime(&f);
    revoke(&f);
    let op = operation(&f);
    f.store.begin_retirement("test", &op).unwrap();
    f.store.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_cleanup BEFORE UPDATE OF retired_at_millis ON agent_assignment_resources BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    let receipt = Receipt {
        version: 1,
        operation: op,
        children_absent: true,
    };
    assert!(f.store.reconcile_retirement("test", &receipt).is_err());
    assert!(
        f.reopen()
            .retirement("test", &f.launch)
            .unwrap()
            .unwrap()
            .reconciled_at_millis
            .is_none()
    );
    assert!(
        f.reopen()
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    f.store
        .connection
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_cleanup")
        .unwrap();
    f.store.reconcile_retirement("test", &receipt).unwrap();
    assert!(
        !f.reopen()
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}
#[test]
fn original_grant_owner_cannot_be_replaced_by_newly_spawned_lookalike() {
    let f = Fixture::new();
    accept_runtime(&f);
    f.store
        .close_agent_assignment("assignment", AgentAssignmentClosureReason::Retired)
        .unwrap();
    let op = operation(&f);
    assert_eq!(
        f.store.begin_retirement("test", &op).unwrap_err().code,
        "durable-retirement-owner-unverified"
    );
    // Seed the simulated original owner, then request another incarnation's
    // equally well-formed owner. A fresh process receipt cannot waive the old one.
    let mut lease: forge_app::authorization_lease::Lease = serde_json::from_str(include_str!(
        "../../../../runner/test/authorization-lease.json"
    ))
    .unwrap();
    lease.launch = f.launch.clone();
    lease.owner = op.owner.clone();
    f.store.connection.lock().unwrap().execute("INSERT INTO assignment_authorization_leases(assignment_id, sequence, lease_json) VALUES ('assignment', 1, ?1)", [serde_json::to_string(&lease).unwrap()]).unwrap();
    let mut lookalike = op.clone();
    lookalike.owner.worker_start = "new-start".into();
    assert_eq!(
        f.store
            .begin_retirement("test", &lookalike)
            .unwrap_err()
            .code,
        "durable-retirement-owner-unverified"
    );
    assert!(f.store.retirement("test", &f.launch).unwrap().is_none());
    assert!(
        f.reopen()
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    f.store.begin_retirement("test", &op).unwrap();
}

#[test]
fn unknown_admission_cannot_be_waived_by_owned_cleanup() {
    let f = Fixture::new();
    f.store.reserve("test", &f.launch).unwrap();
    f.store.begin_send("test", &f.launch).unwrap();
    revoke(&f);
    assert_eq!(
        f.store
            .begin_retirement("test", &operation(&f))
            .unwrap_err()
            .code,
        "durable-retirement-admission-unknown"
    );
    assert!(
        f.reopen()
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}
