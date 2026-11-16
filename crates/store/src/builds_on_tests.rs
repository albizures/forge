//! Canonical relationship topology contracts; no second Stack declaration.
use super::*;

#[test]
fn unchanged_code_consumers_carry_transitive_lineage_without_extra_builds_on_edges() {
    let store = fixture();
    let [a, review, c] = std::array::from_fn(|_| task(&store));
    store.configure_task_stack(a, &root()).unwrap();
    store.relate_records(review, a, RecordRelationshipKind::BuildsOn).unwrap();
    store.relate_records(c, review, RecordRelationshipKind::BuildsOn).unwrap();
    pin(&store, a, &"1".repeat(40));
    complete(&store, a, &"a".repeat(40));
    pin(&store, review, &"a".repeat(40));
    cumulative::unchanged(&store, review, &"a".repeat(40));
    let inherited = pin(&store, c, &"a".repeat(40));
    assert_eq!(inherited.predecessor, Some(review));
    assert_eq!(inherited.base_branch, task_branch(a));
    assert!(store.task_stack(review).unwrap().unwrap().publication.is_none());
    assert!(!store.show_record(c).unwrap().relationships.iter().any(|r| r.to_id == a));
}

#[test]
fn incomplete_plans_can_add_and_remove_independent_waits_without_resolving_root() {
    let store = fixture();
    let [a, b, decision] = std::array::from_fn(|_| task(&store));
    store.relate_records(b, a, RecordRelationshipKind::BuildsOn).unwrap();
    store.relate_records(b, decision, RecordRelationshipKind::WaitsFor).unwrap();
    store.unrelate_records(b, decision, RecordRelationshipKind::WaitsFor).unwrap();
    assert!(store.task_stack(b).unwrap_err().message.contains("root execution configuration"));
}

#[test]
fn canonical_suffix_in_started_spec_cannot_be_replanned_through_external_ancestor() {
    let store = fixture();
    let parent = store.open_typed_record(&NewTypedRecord {
        record_type: RecordType::Spec, project_slugs: vec!["alpha".into()],
        body: "Plan".into(), generated_by_ids: vec![],
    }).unwrap().id;
    let child = store.open_task_under_spec("Member", parent, vec![]).unwrap();
    store.set_record_execution_mode(child.id, child.execution_mode, ExecutionMode::Manual, "operator", "Test").unwrap();
    let member = child.id;
    let [a, b] = std::array::from_fn(|_| task(&store));
    store.configure_task_stack(a, &root()).unwrap();
    store.relate_records(b, a, RecordRelationshipKind::BuildsOn).unwrap();
    store.relate_records(member, b, RecordRelationshipKind::BuildsOn).unwrap();
    store.start_record(parent).unwrap();
    let history = store.list_record_updates(b).unwrap();
    assert_eq!(store.unrelate_records(b, a, RecordRelationshipKind::BuildsOn).unwrap_err().code, "spec-plan-started");
    assert_eq!(store.configure_task_stack(a, &root()).unwrap_err().code, "spec-plan-started");
    assert_eq!(store.list_record_updates(b).unwrap(), history);
    assert_eq!(store.task_stack(b).unwrap().unwrap().predecessor, Some(a));
}

#[test]
fn builds_on_alone_derives_linear_transitive_topology_and_keeps_waits_separate() {
    let store = fixture();
    let [a, b, c, decision] = std::array::from_fn(|_| task(&store));
    // Incomplete Open plans are allowed, including declaring links before the root.
    store.relate_records(b, a, RecordRelationshipKind::BuildsOn).unwrap();
    store.relate_records(c, b, RecordRelationshipKind::BuildsOn).unwrap();
    store.configure_task_stack(a, &root()).unwrap();
    store.relate_records(c, decision, RecordRelationshipKind::WaitsFor).unwrap();
    let c_stack = store.task_stack(c).unwrap().unwrap();
    assert_eq!(c_stack.predecessor, Some(b));
    assert_eq!(c_stack.destination.branch, "feature/payments");
    assert_eq!(store.task_stack(b).unwrap().unwrap().predecessor, Some(a));
    assert!(store.task_stack(decision).unwrap().is_none());
    assert!(store.start_record(b).is_err());
    assert!(store.configure_task_stack(b, &root()).is_err());
    let record = store.show_record(c).unwrap();
    assert!(record.relationships.iter().any(|r| r.kind == RecordRelationshipKind::BuildsOn && r.to_id == b));
    assert!(record.relationships.iter().any(|r| r.kind == RecordRelationshipKind::WaitsFor && r.to_id == decision));
    // Canonical successors retain execution state, but no duplicated mutable link.
    let connection = store.connection.lock().unwrap();
    let duplicated: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM task_stacks WHERE record_id IN (?1, ?2) AND predecessor IS NOT NULL)", params![b.cast_signed(), c.cast_signed()], |r| r.get(0)).unwrap();
    assert!(!duplicated);
}

#[test]
fn topology_rejects_forks_multiple_inputs_mixed_cycles_and_conflicting_intents() {
    let store = fixture();
    let [a, b, c, d] = std::array::from_fn(|_| task(&store));
    store.configure_task_stack(a, &root()).unwrap();
    store.relate_records(b, a, RecordRelationshipKind::BuildsOn).unwrap();
    for (from, to, kind) in [
        (c, a, RecordRelationshipKind::BuildsOn),
        (b, c, RecordRelationshipKind::BuildsOn),
        (b, a, RecordRelationshipKind::WaitsFor),
        (b, a, RecordRelationshipKind::BuildsOn),
        (a, b, RecordRelationshipKind::WaitsFor),
    ] {
        let before = store.show_record(from).unwrap();
        let history = store.list_record_updates(from).unwrap();
        assert!(store.relate_records(from, to, kind).is_err());
        assert_eq!(store.show_record(from).unwrap(), before);
        assert_eq!(store.list_record_updates(from).unwrap(), history);
    }
    store.relate_records(c, d, RecordRelationshipKind::WaitsFor).unwrap();
    assert!(store.relate_records(d, c, RecordRelationshipKind::BuildsOn).is_err());
    assert!(store.task_stack(d).unwrap().is_none());
    assert!(store.unrelate_records(b, a, RecordRelationshipKind::WaitsFor).is_err());
    assert!(store.unrelate_records(b, a, RecordRelationshipKind::DependsOn).is_err());
    store.unrelate_records(b, a, RecordRelationshipKind::BuildsOn).unwrap();
    assert!(store.task_stack(b).unwrap().is_none());
    store.relate_records(b, a, RecordRelationshipKind::WaitsFor).unwrap();
    assert!(store.relate_records(b, a, RecordRelationshipKind::BuildsOn).is_err());
    store.unrelate_records(b, a, RecordRelationshipKind::WaitsFor).unwrap();
    store.relate_records(b, a, RecordRelationshipKind::BuildsOn).unwrap();
}

#[test]
fn inherited_result_and_downstream_pins_restrict_topology_transactionally() {
    let store = fixture();
    let [a, b] = std::array::from_fn(|_| task(&store));
    store.configure_task_stack(a, &root()).unwrap();
    pin(&store, a, &"1".repeat(40));
    complete(&store, a, &"a".repeat(40));
    // A completed predecessor may acquire a new unprepared successor.
    store.relate_records(b, a, RecordRelationshipKind::BuildsOn).unwrap();
    let prepared = pin(&store, b, &"a".repeat(40));
    assert_eq!(prepared.base_branch, task_branch(a));
    let before = store.show_record(b).unwrap();
    let history = store.list_record_updates(b).unwrap();
    assert!(store.unrelate_records(b, a, RecordRelationshipKind::BuildsOn).is_err());
    assert_eq!(store.show_record(b).unwrap(), before);
    assert_eq!(store.list_record_updates(b).unwrap(), history);
    assert_eq!(store.task_stack(b).unwrap(), Some(prepared));
}

#[test]
fn builds_on_rejects_projectless_cross_project_and_non_task_endpoints() {
    let store = fixture();
    store.register(&project("beta", "/other-stack-project")).unwrap();
    let a = task(&store);
    for (record_type, scope) in [
        (RecordType::Task, vec![]),
        (RecordType::Task, vec!["beta".into()]),
        (RecordType::Spec, vec!["alpha".into()]),
    ] {
        let other = store.open_typed_record(&NewTypedRecord { record_type, project_slugs: scope, body: "Other".into(), generated_by_ids: vec![] }).unwrap();
        assert!(store.relate_records(a, other.id, RecordRelationshipKind::BuildsOn).is_err());
        assert!(store.relate_records(other.id, a, RecordRelationshipKind::BuildsOn).is_err());
    }
}

#[test]
fn failed_removal_history_restores_relationship_and_derived_execution_state() {
    let store = fixture();
    let [a, b] = std::array::from_fn(|_| task(&store));
    store.configure_task_stack(a, &root()).unwrap();
    store.relate_records(b, a, RecordRelationshipKind::BuildsOn).unwrap();
    let before = store.show_record(b).unwrap();
    let stack = store.task_stack(b).unwrap();
    let history = store.list_record_updates(b).unwrap();
    store.connection.lock().unwrap().execute_batch(
        "CREATE TEMP TRIGGER reject_removal BEFORE INSERT ON record_updates WHEN NEW.kind = 'relationship-removed' BEGIN SELECT RAISE(ABORT, 'history unavailable'); END;"
    ).unwrap();
    assert!(store.unrelate_records(b, a, RecordRelationshipKind::BuildsOn).is_err());
    assert_eq!(store.show_record(b).unwrap(), before);
    assert_eq!(store.task_stack(b).unwrap(), stack);
    assert_eq!(store.list_record_updates(b).unwrap(), history);
}

#[test]
fn failed_builds_on_audit_rolls_back_relationship_designation_and_execution_state() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let store = fixture();
    let [a, b] = std::array::from_fn(|_| task(&store));
    store.configure_task_stack(a, &root()).unwrap();
    let before = store.show_record(b).unwrap();
    let history = store.list_record_updates(b).unwrap();
    store.connection.lock().unwrap().authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Insert { table_name: "record_comments" } => Authorization::Deny,
        _ => Authorization::Allow,
    })).unwrap();
    assert!(store.relate_records(b, a, RecordRelationshipKind::BuildsOn).is_err());
    store.connection.lock().unwrap().authorizer(None::<fn(AuthContext<'_>) -> Authorization>).unwrap();
    assert_eq!(store.show_record(b).unwrap(), before);
    assert_eq!(store.list_record_updates(b).unwrap(), history);
    assert!(store.task_stack(b).unwrap().is_none());
    assert_eq!(store.task_base_commit(b).unwrap(), Some("0".repeat(40)));
}
