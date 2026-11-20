//! Finalization facts are checked atomically, not inferred from adapter replies.
use super::*;
use forge_app::{AgentAssignmentClosureReason, NewAgentAssignmentResource};

fn marker(f: &Fixture) {
    f.store
        .add_agent_assignment_resource(&NewAgentAssignmentResource {
            assignment_id: "assignment".into(),
            resource_kind: "retirement-pending".into(),
            resource_id: "operator".into(),
        })
        .unwrap();
}

fn revoke(f: &Fixture) {
    f.store
        .close_agent_assignment("assignment", AgentAssignmentClosureReason::Retired)
        .unwrap();
}

fn pending(store: &SqliteProjectRepository) -> forge_app::AgentAssignmentResource {
    store
        .list_agent_assignment_resources("assignment")
        .unwrap()
        .into_iter()
        .find(|r| r.resource_kind == "retirement-pending" && r.resource_id == "operator")
        .unwrap()
}

#[test]
fn retirement_finalization_requires_revocation_and_cannot_use_generic_cleanup() {
    let f = Fixture::new();
    marker(&f);
    assert_eq!(
        f.store
            .finish_agent_assignment_retirement("missing")
            .unwrap_err()
            .code,
        "assignment-not-found"
    );
    assert_eq!(
        f.store
            .finish_agent_assignment_retirement("assignment")
            .unwrap_err()
            .code,
        "assignment-not-revoked"
    );
    assert!(pending(&f.store).retired_at_millis.is_none());
    revoke(&f);
    assert_eq!(
        f.store
            .retire_agent_assignment_resource("assignment", "retirement-pending", "operator")
            .unwrap_err()
            .code,
        "assignment-retirement-reconciliation-required"
    );
    f.store
        .finish_agent_assignment_retirement("assignment")
        .unwrap();
    let original = pending(&f.store);
    assert!(original.retired_at_millis.is_some());
    let reopened = f.reopen();
    reopened
        .finish_agent_assignment_retirement("assignment")
        .unwrap();
    assert_eq!(pending(&reopened), original);
}

#[test]
fn closed_runtime_and_every_uncertainty_class_block_finalization_across_restart() {
    for kind in [
        "durable-runtime",
        "provision-intent",
        "interrupted-effect",
        "authorization-uncertain",
        "pane",
        "retirement-pending",
    ] {
        let f = Fixture::new();
        marker(&f);
        // Typed specialized effect retention is exercised elsewhere. Seed the
        // persisted uncertainty here to test finalization independently of origin.
        f.store.connection.lock().unwrap().execute(
            "INSERT INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) VALUES ('assignment', ?1, 'other', 1)", [kind],
        ).unwrap();
        revoke(&f);
        for store in [&f.store, &f.reopen()] {
            assert_eq!(
                store
                    .finish_agent_assignment_retirement("assignment")
                    .unwrap_err()
                    .code,
                "assignment-retirement-unreconciled",
                "{kind}"
            );
            assert!(pending(store).retired_at_millis.is_none());
            assert!(
                store
                    .agent_assignment_info("assignment")
                    .unwrap()
                    .capacity_reserved()
            );
        }
    }
}

#[test]
fn finalization_does_not_waive_unknown_launch_confirmation_or_clear_hold() {
    let f = Fixture::new();
    marker(&f);
    f.store
        .add_agent_assignment_resource(&NewAgentAssignmentResource {
            assignment_id: "assignment".into(),
            resource_kind: "provision-intent".into(),
            resource_id: "once".into(),
        })
        .unwrap();
    let record = f.store.agent_assignment("assignment").unwrap().record_id;
    f.store
        .hold_dispatch(record, "operator investigation")
        .unwrap();
    revoke(&f);
    assert!(
        f.store
            .finish_agent_assignment_retirement("assignment")
            .is_err()
    );
    f.store
        .resolve_unknown_agent_resources("assignment")
        .unwrap();
    f.store
        .finish_agent_assignment_retirement("assignment")
        .unwrap();
    let info = f.reopen().agent_assignment_info("assignment").unwrap();
    assert!(!info.capacity_reserved());
    assert!(info.dispatch_hold.is_some());
    assert!(info.unknown_resource_resolution.is_some());
}

#[test]
fn storage_failure_leaves_retirement_pending_and_capacity_reserved() {
    let f = Fixture::new();
    marker(&f);
    revoke(&f);
    f.store.connection.lock().unwrap().execute_batch(
        "CREATE TRIGGER fail_retirement BEFORE UPDATE OF retired_at_millis ON agent_assignment_resources WHEN OLD.resource_kind = 'retirement-pending' BEGIN SELECT RAISE(ABORT, 'injected'); END;",
    ).unwrap();
    assert!(
        f.store
            .finish_agent_assignment_retirement("assignment")
            .is_err()
    );
    let reopened = f.reopen();
    assert!(pending(&reopened).retired_at_millis.is_none());
    assert!(
        reopened
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    assert!(
        reopened
            .agent_assignment("assignment")
            .unwrap()
            .closed_at_millis
            .is_some()
    );
}
