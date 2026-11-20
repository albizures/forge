use super::{Fixture, accept_runtime};
use crate::SqliteProjectRepository;
use forge_app::{
    AgentDispatchRepository,
    durable_execution::effects::{InterruptedEffect, Repository as _},
};

#[test]
fn interrupted_effects_block_lease_renewal_even_after_explicit_hold_clearance() {
    use forge_app::authorization_lease::Repository as _;
    let f = Fixture::new();
    accept_runtime(&f);
    let first = super::lease(&f);
    f.store.grant_lease("test", &first, 1_000).unwrap();
    f.store
        .retain_interrupted_effects("test", &f.launch, &[effect("1")])
        .unwrap();
    let mut next = first;
    next.sequence = 2;
    next.issued_boottime_millis += 10_000;
    next.expires_boottime_millis += 10_000;
    let record = f
        .store
        .agent_assignment_info("assignment")
        .unwrap()
        .assignment
        .record_id;
    f.store.resume_dispatch(record).unwrap();
    assert_eq!(
        f.reopen()
            .grant_lease("test", &next, 11_000)
            .unwrap_err()
            .code,
        "assignment-lease-blocked"
    );
}

#[test]
fn interrupted_effect_retention_preserves_accepted_outcome_bytes() {
    let f = Fixture::new();
    accept_runtime(&f);
    // Boundary fixture only: authenticated acceptance is separately exercised by
    // the component tracer. Retaining uncertainty must not rewrite any receipt.
    f.store.connection.lock().unwrap().execute("UPDATE record_attempts SET closed = 1, outcome_json = '{\"original\":true}', result_json = '{\"accepted\":true}' WHERE id = 'attempt'", []).unwrap();
    f.reopen()
        .retain_interrupted_effects("test", &f.launch, &[effect("1")])
        .unwrap();
    let (outcome, result): (String, String) = f
        .store
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT outcome_json, result_json FROM record_attempts WHERE id = 'attempt'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(outcome, "{\"original\":true}");
    assert_eq!(result, "{\"accepted\":true}");
    assert!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .recovery_blockers()
            .contains(&"interrupted-effect".into())
    );
}

#[test]
fn interrupted_effect_total_limit_is_atomic_and_invalid_identities_fail_closed() {
    let f = Fixture::new();
    accept_runtime(&f);
    for id in ["", " ", "control\nidentity"] {
        let mut invalid = effect("valid");
        invalid.call_id = id.into();
        assert!(
            f.store
                .retain_interrupted_effects("test", &f.launch, &[invalid])
                .is_err()
        );
    }
    let effects: Vec<_> = (0..1000).map(|i| effect(&i.to_string())).collect();
    f.store
        .retain_interrupted_effects("test", &f.launch, &effects)
        .unwrap();
    let before = f.store.agent_assignment_info("assignment").unwrap();
    assert_eq!(
        f.reopen()
            .retain_interrupted_effects("test", &f.launch, &[effect("overflow")])
            .unwrap_err()
            .code,
        "durable-effect-limit"
    );
    assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), before);
}

fn effect(task: &str) -> InterruptedEffect {
    InterruptedEffect {
        task_id: task.into(),
        conversation_id: "child-conversation".into(),
        call_id: format!("call-{task}"),
    }
}

#[test]
fn interrupted_effects_survive_restart_closure_hold_clearance_and_empty_observations() {
    let f = Fixture::new();
    accept_runtime(&f);
    let before = f.store.agent_assignment_info("assignment").unwrap();
    f.store
        .retain_interrupted_effects("test", &f.launch, &[effect("1"), effect("2")])
        .unwrap();
    let info = f.store.agent_assignment_info("assignment").unwrap();
    assert_eq!(info.assignment, before.assignment);
    assert_eq!(info.task_status, before.task_status);
    assert!(
        info.recovery_blockers()
            .contains(&"interrupted-effect".into())
    );
    assert!(info.dispatch_hold.is_some());
    let resources = info.resources;
    assert_eq!(
        resources
            .iter()
            .filter(|r| r.resource_kind == "interrupted-effect")
            .count(),
        2
    );
    for resource in resources
        .iter()
        .filter(|r| r.resource_kind == "interrupted-effect")
    {
        let retained: InterruptedEffect = serde_json::from_str(&resource.resource_id).unwrap();
        assert!(retained == effect("1") || retained == effect("2"));
        assert!(
            f.store
                .retire_agent_assignment_resource(
                    "assignment",
                    "interrupted-effect",
                    &resource.resource_id
                )
                .is_err()
        );
    }
    {
        let c = f.store.connection.lock().unwrap();
        c.execute("UPDATE agent_assignments SET closed_at_millis = 1, closure_reason = 'retired' WHERE id = 'assignment'", []).unwrap();
        c.execute(
            "UPDATE record_attempts SET closed = 1 WHERE id = 'attempt'",
            [],
        )
        .unwrap();
        c.execute("DELETE FROM dispatch_holds", []).unwrap();
    }
    let reopened = f.reopen();
    reopened
        .retain_interrupted_effects("test", &f.launch, &[effect("2"), effect("1")])
        .unwrap();
    reopened
        .retain_interrupted_effects("test", &f.launch, &[])
        .unwrap();
    let info = reopened.agent_assignment_info("assignment").unwrap();
    assert_eq!(info.resources, resources);
    assert!(info.capacity_reserved());
    assert!(info.dispatch_hold.is_some());
    assert!(
        info.recovery_blockers()
            .contains(&"interrupted-effect".into())
    );
    assert!(
        reopened
            .claim_next_agent_assignment("alpha", "replacement")
            .unwrap()
            .is_none()
    );
}

#[test]
fn interrupted_effects_authorize_scope_and_identity_before_retry_and_reject_conflicts_atomically() {
    let f = Fixture::new();
    accept_runtime(&f);
    f.store
        .retain_interrupted_effects("test", &f.launch, &[effect("1")])
        .unwrap();
    let before = f.store.agent_assignment_info("assignment").unwrap();
    assert!(
        f.store
            .retain_interrupted_effects("foreign", &f.launch, &[effect("1")])
            .is_err()
    );
    let mut launch = f.launch.clone();
    launch.incarnation_id = "foreign".into();
    assert!(
        f.store
            .retain_interrupted_effects("test", &launch, &[effect("1")])
            .is_err()
    );
    let mut conflict = effect("1");
    conflict.call_id = "conflicting-call".into();
    assert!(
        f.store
            .retain_interrupted_effects("test", &f.launch, &[effect("new"), conflict])
            .is_err()
    );
    let mut invalid = effect("invalid");
    invalid.call_id = "x".repeat(1025);
    assert!(
        f.store
            .retain_interrupted_effects("test", &f.launch, &[effect("new"), invalid])
            .is_err()
    );
    assert!(
        f.store
            .retain_interrupted_effects("test", &f.launch, &vec![effect("1"); 1001])
            .is_err()
    );
    assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), before);
}

#[test]
fn interrupted_effects_and_hold_rollback_together_without_overwriting_existing_hold() {
    let f = Fixture::new();
    accept_runtime(&f);
    let before = f.store.agent_assignment_info("assignment").unwrap();
    f.store.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_effect_hold BEFORE INSERT ON dispatch_holds BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    assert!(
        f.store
            .retain_interrupted_effects("test", &f.launch, &[effect("1")])
            .is_err()
    );
    assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), before);
    {
        let c = f.store.connection.lock().unwrap();
        c.execute_batch("DROP TRIGGER fail_effect_hold").unwrap();
        c.execute("INSERT INTO dispatch_holds(record_id, reason) SELECT record_id, 'unrelated safety hold' FROM agent_assignments WHERE id = 'assignment'", []).unwrap();
    }
    f.store
        .retain_interrupted_effects("test", &f.launch, &[effect("1")])
        .unwrap();
    assert_eq!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .dispatch_hold
            .unwrap()
            .reason,
        "unrelated safety hold"
    );
}

#[test]
fn interrupted_effects_racing_connections_preserve_one_receipt_per_identity() {
    let f = Fixture::new();
    accept_runtime(&f);
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let mut threads = Vec::new();
        for _ in 0..4 {
            let barrier = &barrier;
            let f = &f;
            threads.push(scope.spawn(move || {
                let store: SqliteProjectRepository = f.reopen();
                barrier.wait();
                store
                    .retain_interrupted_effects("test", &f.launch, &[effect("1")])
                    .unwrap();
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
    });
    assert_eq!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .resources
            .iter()
            .filter(|r| r.resource_kind == "interrupted-effect")
            .count(),
        1
    );
}

#[test]
fn interrupted_effects_are_retained_even_when_admission_reply_is_unknown() {
    use forge_app::durable_execution::Repository as _;
    let f = Fixture::new();
    f.store.reserve("test", &f.launch).unwrap();
    f.store.begin_send("test", &f.launch).unwrap();
    f.reopen()
        .retain_interrupted_effects("test", &f.launch, &[effect("1")])
        .unwrap();
    assert!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .recovery_blockers()
            .contains(&"interrupted-effect".into())
    );
}
