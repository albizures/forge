//! Completion reports actual workflow changes without claiming dispatch or lineage evidence.
use super::*;
use forge_app::AgentDispatchRepository;

fn force(store: &SqliteProjectRepository, id: u64) -> Vec<u64> {
    let result = store
        .force_done_record(id, Some("Obsolete"), "operator")
        .unwrap();
    let consequences = result.dependency_consequences.unwrap();
    assert!(consequences.warning.contains("may dispatch"));
    assert!(consequences.warning.contains("not dispatchability"));
    consequences.newly_eligible_record_ids
}

#[test]
fn force_reports_only_newly_eligible_dependents_and_retries_report_no_transition() {
    let store = fixture();
    let prerequisite = task(&store);
    let other = task(&store);
    let ready = task(&store);
    let blocked = task(&store);
    let started = task(&store);
    for dependent in [ready, blocked, started] {
        store
            .relate_records(dependent, prerequisite, RecordRelationshipKind::DependsOn)
            .unwrap();
    }
    store
        .relate_records(blocked, other, RecordRelationshipKind::DependsOn)
        .unwrap();
    // Legacy In Progress work is not newly eligible Open work.
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE records SET status = 'in_progress' WHERE id = ?1",
            [started.cast_signed()],
        )
        .unwrap();
    let checks = store.list_record_checks(ready).unwrap();
    let relationships = store.show_record(ready).unwrap().relationships;
    assert_eq!(force(&store, prerequisite), vec![ready]);
    assert_eq!(store.list_record_checks(ready).unwrap(), checks);
    assert_eq!(
        store.show_record(ready).unwrap().relationships,
        relationships
    );
    assert_eq!(store.show_record(ready).unwrap().status, RecordStatus::Open);
    assert!(store.list_agent_assignments(ready).unwrap().is_empty());
    let retry = store
        .force_done_record(prerequisite, Some("different"), "operator")
        .unwrap();
    assert!(retry.already_done);
    assert!(retry.dependency_consequences.is_none());
    assert_eq!(force(&store, other), vec![blocked]);
    assert!(force(&store, ready).is_empty());
}

#[test]
fn forced_done_does_not_supply_parent_plan_or_stack_evidence() {
    let store = fixture();
    let prerequisite = task(&store);
    let open_parent = spec(&store);
    let under_open = child(&store, open_parent);
    let invalid_parent = spec(&store);
    let invalid = child(&store, invalid_parent);
    let sibling = child(&store, invalid_parent);
    store
        .relate_records(invalid, sibling, RecordRelationshipKind::DependsOn)
        .unwrap();
    let missing_stack = task(&store);
    store.configure_task_stack(prerequisite, &root()).unwrap();
    link(&store, missing_stack, prerequisite);
    // Stack configuration already creates the predecessor dependency.
    for dependent in [under_open, invalid] {
        store
            .relate_records(dependent, prerequisite, RecordRelationshipKind::DependsOn)
            .unwrap();
    }
    // A legacy started plan has no configured inheritance. Force must not repair it.
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE records SET status = 'in_progress' WHERE id = ?1",
            [invalid_parent.cast_signed()],
        )
        .unwrap();
    assert!(force(&store, sibling).is_empty());
    let stack = store.task_stack(missing_stack).unwrap();
    let checks = store.list_record_checks(missing_stack).unwrap();
    assert!(force(&store, prerequisite).is_empty());
    assert_eq!(store.task_stack(missing_stack).unwrap(), stack);
    assert_eq!(store.list_record_checks(missing_stack).unwrap(), checks);
    assert!(
        store
            .record_work_state(prerequisite)
            .unwrap()
            .output
            .is_none()
    );
    for (dependent, code) in [
        (under_open, "spec-execution-blocked"),
        (invalid, "invalid-spec-plan"),
        (missing_stack, "stack-blocked"),
    ] {
        assert_eq!(store.start_record(dependent).unwrap_err().code, code);
    }
}

#[test]
fn normal_automatic_claim_observes_forced_done_without_completion_launch_or_pause() {
    let store = fixture();
    let parent = spec(&store);
    let prerequisite = child(&store, parent);
    let dependent = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Dispatch normally".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap()
        .id;
    store
        .relate_records(dependent, prerequisite, RecordRelationshipKind::DependsOn)
        .unwrap();
    assert!(
        store
            .claim_next_agent_assignment("alpha", "before-force")
            .unwrap()
            .is_none()
    );
    assert_eq!(force(&store, prerequisite), vec![dependent]);
    assert!(store.list_agent_assignments(dependent).unwrap().is_empty());
    // Only normal reconciliation's claim operation allocates an Assignment.
    let claim = store
        .claim_next_agent_assignment("alpha", "after-force")
        .unwrap()
        .unwrap();
    assert_eq!(claim.record_id, dependent);
    assert_eq!(store.list_agent_assignments(dependent).unwrap().len(), 1);
}
