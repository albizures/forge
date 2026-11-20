use super::{Fixture, accept_runtime};
use forge_app::{
    AgentAssignmentClosureReason, AgentDispatchRepository, NewAgentAssignmentResource,
    durable_execution::{
        effect_resolution::{ExternalOutcome, Repository as _, Resolution},
        effects::{InterruptedEffect, Repository as _},
    },
};

fn resolution() -> Resolution {
    Resolution {
        operation_id: "resolve-1".into(),
        effect: InterruptedEffect {
            task_id: "task-1".into(),
            conversation_id: "child".into(),
            call_id: "call-1".into(),
        },
        outcome: ExternalOutcome::Applied,
        actor: "operator".into(),
        evidence: "Controlled endpoint receipt: publication-1".into(),
        message: "Investigated externally; no command replay".into(),
        confirm_old_agent_cannot_act: true,
        confirm_external_work_safe: true,
    }
}

fn prepare(f: &Fixture) {
    accept_runtime(f);
    f.store
        .retain_interrupted_effects("test", &f.launch, &[resolution().effect])
        .unwrap();
}

fn revoke(f: &Fixture) {
    f.store
        .close_agent_assignment("assignment", AgentAssignmentClosureReason::Retired)
        .unwrap();
}

#[test]
fn effect_resolution_is_atomic_targeted_audited_and_replayed_after_restart() {
    let f = Fixture::new();
    prepare(&f);
    // Accepted-result fixture: resolving an external effect must not rewrite it.
    f.store.connection.lock().unwrap().execute("UPDATE record_attempts SET closed = 1, outcome_json = '{\"original\":true}', result_json = '{\"accepted\":true}' WHERE id = 'attempt'", []).unwrap();
    revoke(&f);
    let before = f.store.agent_assignment_info("assignment").unwrap();
    let receipt = f
        .store
        .resolve_interrupted_effect("test", &f.launch, &resolution())
        .unwrap();
    let after = f.store.agent_assignment_info("assignment").unwrap();
    assert_eq!(after.assignment, before.assignment);
    assert_eq!(after.dispatch_hold, before.dispatch_hold);
    assert!(after.capacity_reserved()); // durable-runtime is still unresolved
    assert!(
        !after
            .recovery_blockers()
            .contains(&"interrupted-effect".into())
    );
    let audit = after
        .resources
        .iter()
        .find(|r| r.resource_kind == "interrupted-effect-resolution")
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Resolution>(&audit.resource_id).unwrap(),
        resolution()
    );
    assert_eq!(audit.created_at_millis, receipt.accepted_at_millis);
    assert_eq!(audit.retired_at_millis, Some(receipt.accepted_at_millis));
    assert_eq!(
        after
            .resources
            .iter()
            .find(|r| r.resource_kind == "durable-runtime"),
        before
            .resources
            .iter()
            .find(|r| r.resource_kind == "durable-runtime")
    );
    let reopened = f.reopen();
    assert_eq!(
        reopened
            .resolve_interrupted_effect("test", &f.launch, &resolution())
            .unwrap(),
        receipt
    );
    // Re-observation never overwrites the accepted resolution or reactivates its
    // resource. It also cannot manufacture a runtime result or execution grant.
    reopened
        .retain_interrupted_effects("test", &f.launch, &[resolution().effect])
        .unwrap();
    assert_eq!(reopened.agent_assignment_info("assignment").unwrap(), after);
    let c = reopened.connection.lock().unwrap();
    let results: (String, String) = c
        .query_row(
            "SELECT outcome_json, result_json FROM record_attempts WHERE id = 'attempt'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        results,
        ("{\"original\":true}".into(), "{\"accepted\":true}".into())
    );
}

#[test]
fn effect_resolution_rejects_live_unconfirmed_unknown_missing_and_conflicting_requests() {
    let f = Fixture::new();
    prepare(&f);
    let before = f.store.agent_assignment_info("assignment").unwrap();
    assert_eq!(
        f.store
            .resolve_interrupted_effect("test", &f.launch, &resolution())
            .unwrap_err()
            .code,
        "durable-effect-resolution-revocation-required"
    );
    assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), before);
    revoke(&f);
    let before = f.store.agent_assignment_info("assignment").unwrap();
    let mut invalids = vec![];
    let r = resolution();
    let mut missing_confirmation = r.clone();
    missing_confirmation.confirm_old_agent_cannot_act = false;
    invalids.push(missing_confirmation);
    let mut missing_confirmation = r.clone();
    missing_confirmation.confirm_external_work_safe = false;
    invalids.push(missing_confirmation);
    for field in ["actor", "evidence", "message", "operation_id"] {
        for value in [
            String::new(),
            " ".into(),
            "secret\ncontrol".into(),
            "x".repeat(4097),
        ] {
            let mut json = serde_json::to_value(&r).unwrap();
            json[field] = value.into();
            invalids.push(serde_json::from_value(json).unwrap());
        }
    }
    let mut missing = r.clone();
    missing.effect.task_id = "not-retained".into();
    invalids.push(missing);
    let mut wrong_call = r.clone();
    wrong_call.effect.call_id = "foreign-call".into();
    invalids.push(wrong_call);
    for invalid in invalids {
        assert!(
            f.store
                .resolve_interrupted_effect("test", &f.launch, &invalid)
                .is_err()
        );
        assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), before);
    }
    let mut unknown = serde_json::to_value(&r).unwrap();
    unknown["outcome"] = "unknown".into();
    assert!(serde_json::from_value::<Resolution>(unknown).is_err());
    f.store
        .resolve_interrupted_effect("test", &f.launch, &r)
        .unwrap();
    let before = f.store.agent_assignment_info("assignment").unwrap();
    for field in [
        "operation_id",
        "actor",
        "message",
        "evidence",
        "outcome",
        "effect",
    ] {
        let mut json = serde_json::to_value(&r).unwrap();
        json[field] = match field {
            "outcome" => "not-applied".into(),
            "effect" => serde_json::to_value(InterruptedEffect {
                task_id: "foreign".into(),
                ..r.effect.clone()
            })
            .unwrap(),
            _ => "conflicting".into(),
        };
        let conflict = serde_json::from_value(json).unwrap();
        assert!(
            f.reopen()
                .resolve_interrupted_effect("test", &f.launch, &conflict)
                .is_err()
        );
    }
    assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), before);
}

#[test]
fn effect_resolution_authorizes_full_scope_before_exact_replay() {
    let f = Fixture::new();
    prepare(&f);
    revoke(&f);
    f.store
        .resolve_interrupted_effect("test", &f.launch, &resolution())
        .unwrap();
    assert!(
        f.store
            .resolve_interrupted_effect("foreign", &f.launch, &resolution())
            .is_err()
    );
    let launch = serde_json::to_value(&f.launch).unwrap();
    for key in launch.as_object().unwrap().keys() {
        let mut foreign = launch.clone();
        foreign[key] = if key == "profile_revision" || key == "runtime_schema" {
            999.into()
        } else {
            "foreign".into()
        };
        let foreign = serde_json::from_value(foreign).unwrap();
        assert!(
            f.reopen()
                .resolve_interrupted_effect("test", &foreign, &resolution())
                .is_err(),
            "{key}"
        );
    }
}

#[test]
fn accepted_effect_resolution_is_a_scoped_read_not_new_authority() {
    let f = Fixture::new();
    prepare(&f);
    let before = f.store.agent_assignment_info("assignment").unwrap();
    assert_eq!(
        f.store
            .accepted_effect_resolution("test", &f.launch, &resolution())
            .unwrap(),
        None
    );
    assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), before);
    revoke(&f);
    let receipt = f
        .store
        .resolve_interrupted_effect("test", &f.launch, &resolution())
        .unwrap();
    let before = f.store.agent_assignment_info("assignment").unwrap();
    assert_eq!(
        f.reopen()
            .accepted_effect_resolution("test", &f.launch, &resolution())
            .unwrap(),
        Some(receipt)
    );
    assert!(
        f.store
            .accepted_effect_resolution("foreign", &f.launch, &resolution())
            .is_err()
    );
    let mut foreign = f.launch.clone();
    foreign.component_hash = "foreign".into();
    assert!(
        f.store
            .accepted_effect_resolution("test", &foreign, &resolution())
            .is_err()
    );
    let mut conflict = resolution();
    conflict.evidence = "changed evidence".into();
    assert!(
        f.store
            .accepted_effect_resolution("test", &f.launch, &conflict)
            .is_err()
    );
    assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), before);
}

#[test]
fn effect_resolution_audit_and_resource_retirement_rollback_together() {
    let f = Fixture::new();
    prepare(&f);
    revoke(&f);
    let before = f.store.agent_assignment_info("assignment").unwrap();
    f.store.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_resolution BEFORE UPDATE OF retired_at_millis ON agent_assignment_resources WHEN OLD.resource_kind = 'interrupted-effect' BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    assert!(
        f.store
            .resolve_interrupted_effect("test", &f.launch, &resolution())
            .is_err()
    );
    assert_eq!(
        f.reopen().agent_assignment_info("assignment").unwrap(),
        before
    );
    f.store
        .connection
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_resolution")
        .unwrap();
    f.store
        .resolve_interrupted_effect("test", &f.launch, &resolution())
        .unwrap();
}

#[test]
fn effect_resolution_racing_connections_preserve_one_original_audit() {
    let f = Fixture::new();
    prepare(&f);
    revoke(&f);
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let f = &f;
                let barrier = &barrier;
                scope.spawn(move || {
                    let store = f.reopen();
                    barrier.wait();
                    store
                        .resolve_interrupted_effect("test", &f.launch, &resolution())
                        .unwrap()
                })
            })
            .collect();
        let receipts: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(receipts.iter().all(|r| r == &receipts[0]));
    });
    assert_eq!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .resources
            .iter()
            .filter(|r| r.resource_kind == "interrupted-effect-resolution")
            .count(),
        1
    );
}

#[test]
fn effect_resolution_preserves_unrelated_effects_holds_and_capacity() {
    let f = Fixture::new();
    prepare(&f);
    let other = InterruptedEffect {
        task_id: "other-task".into(),
        ..resolution().effect
    };
    f.store
        .retain_interrupted_effects("test", &f.launch, std::slice::from_ref(&other))
        .unwrap();
    f.store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE dispatch_holds SET reason = 'unrelated safety hold'",
            [],
        )
        .unwrap();
    revoke(&f);
    f.store
        .resolve_interrupted_effect("test", &f.launch, &resolution())
        .unwrap();
    let info = f.store.agent_assignment_info("assignment").unwrap();
    assert_eq!(info.dispatch_hold.unwrap().reason, "unrelated safety hold");
    let other_resource = info
        .resources
        .iter()
        .find(|r| {
            r.resource_kind == "interrupted-effect"
                && r.resource_id == serde_json::to_string(&other).unwrap()
        })
        .unwrap();
    assert!(other_resource.retired_at_millis.is_none());
    let record = info.assignment.record_id;
    f.store.resume_dispatch(record).unwrap();
    let info = f.store.agent_assignment_info("assignment").unwrap();
    assert!(
        info.recovery_blockers()
            .contains(&"interrupted-effect".into())
    );
    assert!(info.capacity_reserved());
    assert!(
        f.store
            .claim_next_agent_assignment("alpha", "replacement")
            .unwrap()
            .is_none()
    );
}

#[test]
fn effect_resolution_conflicting_concurrent_decisions_accept_only_one() {
    let f = Fixture::new();
    prepare(&f);
    revoke(&f);
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = [ExternalOutcome::Applied, ExternalOutcome::NotApplied]
            .into_iter()
            .map(|outcome| {
                let f = &f;
                let barrier = &barrier;
                scope.spawn(move || {
                    let store = f.reopen();
                    let mut r = resolution();
                    r.outcome = outcome;
                    barrier.wait();
                    store.resolve_interrupted_effect("test", &f.launch, &r)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| r
                .as_ref()
                .is_err_and(|e| e.code == "durable-effect-resolution-conflict"))
            .count(),
        1
    );
    let accepted = results.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(
        f.reopen()
            .resolve_interrupted_effect("test", &f.launch, &accepted.resolution)
            .unwrap(),
        accepted
    );
}

#[test]
fn effect_resources_and_audits_cannot_be_forged_or_retired_through_generic_ports() {
    let f = Fixture::new();
    prepare(&f);
    revoke(&f);
    f.store
        .resolve_interrupted_effect("test", &f.launch, &resolution())
        .unwrap();
    for kind in ["interrupted-effect", "interrupted-effect-resolution"] {
        assert!(
            f.store
                .add_agent_assignment_resource(&NewAgentAssignmentResource {
                    assignment_id: "assignment".into(),
                    resource_kind: kind.into(),
                    resource_id: "forged".into(),
                })
                .is_err()
        );
        let resource = f
            .store
            .agent_assignment_info("assignment")
            .unwrap()
            .resources
            .into_iter()
            .find(|r| r.resource_kind == kind)
            .unwrap();
        assert!(
            f.store
                .retire_agent_assignment_resource("assignment", kind, &resource.resource_id)
                .is_err()
        );
    }
}
