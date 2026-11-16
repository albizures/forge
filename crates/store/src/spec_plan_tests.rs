use super::*;
#[path = "spec_plan_race_tests.rs"]
mod races;
#[path = "spec_execution_tests.rs"]
mod execution;
#[path = "completion_dependency_tests.rs"]
mod completion_dependencies;
use crate::SqliteProjectRepository;
use forge_app::{ExecutionMode, RecordRepository, stacks::StackSource};

fn spec(store: &SqliteProjectRepository) -> u64 {
    store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Spec,
            body: "Initial plan".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap()
        .id
}
fn child(store: &SqliteProjectRepository, spec: u64) -> u64 {
    let record = store.open_task_under_spec("Deliver", spec, vec![]).unwrap();
    store
        .set_record_execution_mode(
            record.id,
            record.execution_mode,
            ExecutionMode::Manual,
            "operator",
            "Manual verification",
        )
        .unwrap();
    record.id
}
fn link(store: &SqliteProjectRepository, task: u64, predecessor: u64) {
    store
        .configure_task_stack(
            task,
            &StackSource::Predecessor {
                record_id: predecessor,
            },
        )
        .unwrap();
}

#[test]
fn incomplete_legacy_plan_stays_open_without_status_or_audit_writes() {
    let store = fixture();
    let parent = spec(&store);
    let first = child(&store, parent);
    let second = child(&store, parent);
    store
        .relate_records(second, first, RecordRelationshipKind::DependsOn)
        .unwrap();
    let before = store.show_record(parent).unwrap();
    let updates = store.list_record_updates(parent).unwrap();
    let error = store.start_record(parent).unwrap_err();
    assert_eq!(error.code, "invalid-spec-plan");
    assert!(
        error
            .message
            .contains(&format!("Task {second}, prerequisite {first}"))
    );
    assert_eq!(store.show_record(parent).unwrap(), before);
    assert_eq!(store.list_record_updates(parent).unwrap(), updates);
    // An incomplete Open plan can be repaired without rewriting Checks or existing Tasks.
    store.configure_task_stack(first, &root()).unwrap();
    link(&store, second, first);
    assert_eq!(
        store.start_record(parent).unwrap().status,
        RecordStatus::InProgress
    );
}

#[test]
fn explicit_ordering_exceptions_do_not_hide_ambiguous_inheritance() {
    let store = fixture();
    let parent = spec(&store);
    let first = child(&store, parent);
    let other = child(&store, parent);
    let last = child(&store, parent);
    store.configure_task_stack(first, &root()).unwrap();
    store.configure_task_stack(other, &root()).unwrap();
    link(&store, last, first);
    store
        .relate_records(last, other, RecordRelationshipKind::DependsOn)
        .unwrap();
    assert_eq!(
        store.start_record(parent).unwrap_err().code,
        "invalid-spec-plan"
    );
    store
        .unrelate_records(last, other, RecordRelationshipKind::DependsOn)
        .unwrap();
    store.relate_ordering_only_records(last, other).unwrap();
    // Deliberate independent roots plus an ordering-only prerequisite are supported.
    assert_eq!(
        store.start_record(parent).unwrap().status,
        RecordStatus::InProgress
    );
}

#[test]
fn transitive_cumulative_prerequisites_are_not_mistaken_for_a_merge() {
    let store = fixture();
    let parent = spec(&store);
    let first = child(&store, parent);
    let second = child(&store, parent);
    let last = child(&store, parent);
    store.configure_task_stack(first, &root()).unwrap();
    link(&store, second, first);
    link(&store, last, second);
    store
        .relate_records(last, first, RecordRelationshipKind::DependsOn)
        .unwrap();
    store.start_record(parent).unwrap();
}

#[test]
fn started_spec_rejects_every_ordinary_structural_write_without_partial_history() {
    let store = fixture();
    let parent = spec(&store);
    let first = child(&store, parent);
    let second = child(&store, parent);
    let other = child(&store, parent);
    store.configure_task_stack(first, &root()).unwrap();
    link(&store, second, first);
    store.relate_ordering_only_records(second, other).unwrap();
    store.start_record(parent).unwrap();
    let before = store.show_record(second).unwrap();
    let history = store.list_record_updates(second).unwrap();
    let stack = store.task_stack(second).unwrap();
    let records = store
        .list_records(&RecordListFilter {
            project_slug: None,
            status: None,
            record_type: None,
        })
        .unwrap();
    assert_eq!(
        store
            .open_task_under_spec("Late Task", parent, vec![])
            .unwrap_err()
            .code,
        "spec-plan-started"
    );
    for result in [
        store.relate_records(other, first, RecordRelationshipKind::DependsOn),
        store.relate_ordering_only_records(second, first),
        store.unrelate_records(second, other, RecordRelationshipKind::DependsOn),
        store.unrelate_records(second, first, RecordRelationshipKind::DependsOn),
    ] {
        assert_eq!(result.unwrap_err().code, "spec-plan-started");
    }
    assert_eq!(
        store
            .configure_task_stack(second, &root())
            .unwrap_err()
            .code,
        "spec-plan-started"
    );
    assert_eq!(
        store
            .list_records(&RecordListFilter {
                project_slug: None,
                status: None,
                record_type: None
            })
            .unwrap(),
        records
    );
    assert_eq!(store.show_record(second).unwrap(), before);
    assert_eq!(store.list_record_updates(second).unwrap(), history);
    assert_eq!(store.task_stack(second).unwrap(), stack);
    store
        .add_record_comment(second, "Operator context", None)
        .unwrap();
    store.edit_record(second, "Clarified work").unwrap();
    // Pinning/preparation is not replanning and remains available after start.
    pin(&store, first, &"1".repeat(40));
    assert_eq!(
        store.configure_task_stack(first, &root()).unwrap_err().code,
        "spec-plan-started"
    );
}

#[test]
fn independent_tasks_empty_plans_and_external_dependencies_retain_existing_semantics() {
    let store = fixture();
    let empty = spec(&store);
    store.start_record(empty).unwrap();
    let parent = spec(&store);
    let first = child(&store, parent);
    let second = child(&store, parent);
    let external = task(&store);
    store
        .relate_records(first, external, RecordRelationshipKind::DependsOn)
        .unwrap();
    store.relate_ordering_only_records(second, first).unwrap();
    store.start_record(parent).unwrap();
    // Standalone dependency writes do not gain a Spec gate.
    store
        .relate_records(external, second, RecordRelationshipKind::DependsOn)
        .unwrap_err(); // cycle
    let external_second = task(&store);
    store
        .relate_records(external_second, external, RecordRelationshipKind::DependsOn)
        .unwrap();
}

#[test]
fn start_rechecks_configured_member_compatibility_without_repinning_checks() {
    let store = fixture();
    let parent = spec(&store);
    let first = child(&store, parent);
    store.configure_task_stack(first, &root()).unwrap();
    let checks = store.list_record_checks(first).unwrap();
    // Current mode changes already reject incompatible pins; model retained legacy state.
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE records SET execution_mode = 'dispatched' WHERE id = ?1",
            [first.cast_signed()],
        )
        .unwrap();
    let history = store.list_record_updates(parent).unwrap();
    let error = store.start_record(parent).unwrap_err();
    assert_eq!(error.code, "invalid-spec-plan");
    assert!(
        error
            .message
            .contains(&format!("Spec {parent}, Task {first}"))
    );
    assert!(error.message.contains("pinned profile"));
    assert_eq!(store.list_record_checks(first).unwrap(), checks);
    assert_eq!(store.list_record_updates(parent).unwrap(), history);
    assert_eq!(
        store.show_record(parent).unwrap().status,
        RecordStatus::Open
    );
}

#[test]
fn legacy_contradictory_stack_order_is_diagnosed_without_repair() {
    let store = fixture();
    let parent = spec(&store);
    let first = child(&store, parent);
    let second = child(&store, parent);
    store.configure_task_stack(first, &root()).unwrap();
    link(&store, second, first);
    // Model historical inconsistent state, not a supported mutation or repair path.
    store.connection.lock().unwrap().execute(
        "DELETE FROM record_relationships WHERE from_id = ?1 AND to_id = ?2 AND kind = 'depends-on'",
        rusqlite::params![second.cast_signed(), first.cast_signed()],
    ).unwrap();
    let before = store.show_record(second).unwrap();
    let history = store.list_record_updates(parent).unwrap();
    let error = store.start_record(parent).unwrap_err();
    assert_eq!(error.code, "invalid-spec-plan");
    assert!(error.message.contains(&format!(
        "Task {second} must depend on Stack Predecessor {first}"
    )));
    assert_eq!(store.show_record(second).unwrap(), before);
    assert_eq!(store.list_record_updates(parent).unwrap(), history);
    assert_eq!(
        store.show_record(parent).unwrap().status,
        RecordStatus::Open
    );
}

#[test]
fn changing_an_external_stack_ancestor_cannot_replan_a_started_spec_suffix() {
    let store = fixture();
    let parent = spec(&store);
    let external = task(&store);
    let member = child(&store, parent);
    store.configure_task_stack(external, &root()).unwrap();
    link(&store, member, external);
    store.start_record(parent).unwrap();
    let before = store.task_stack(member).unwrap();
    let history = store.list_record_updates(external).unwrap();
    let mut source = root();
    if let StackSource::Root { destination } = &mut source {
        destination.branch = "another-input".into();
    }
    assert_eq!(
        store
            .configure_task_stack(external, &source)
            .unwrap_err()
            .code,
        "spec-plan-started"
    );
    assert_eq!(store.task_stack(member).unwrap(), before);
    assert_eq!(store.list_record_updates(external).unwrap(), history);
}

#[test]
fn spec_transition_and_history_are_atomic_when_audit_insertion_fails() {
    let store = fixture();
    let parent = spec(&store);
    let before = store.list_record_updates(parent).unwrap();
    store.connection.lock().unwrap().execute_batch(
        "CREATE TEMP TRIGGER reject_start_audit BEFORE INSERT ON record_updates WHEN NEW.kind = 'status-transition' BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END;"
    ).unwrap();
    assert!(store.start_record(parent).is_err());
    assert_eq!(
        store.show_record(parent).unwrap().status,
        RecordStatus::Open
    );
    assert_eq!(store.list_record_updates(parent).unwrap(), before);
}
