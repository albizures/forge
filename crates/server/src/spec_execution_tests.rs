#[test]
fn open_parent_explicit_acceptance_and_automatic_dispatch_never_allocate() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    fixture.configure(&app, &provider, "one");
    let store = fixture.store();
    let parent = store.open_typed_record(&NewTypedRecord {
        record_type: RecordType::Spec, body: "Planning".into(),
        project_slugs: vec!["one/api".into()], generated_by_ids: vec![],
    }).unwrap();
    let task = store.open_task_under_spec("Do not execute yet", parent.id, vec![]).unwrap();
    provider.reset();
    assert_eq!(app.dispatch_record(task.id).unwrap_err().code, "spec-execution-blocked");
    assert_eq!(app.execute_task_check_acceptance(task.id).unwrap_err().code, "spec-execution-blocked");
    app.reconcile_agent_dispatch("test", 1).unwrap();
    provider.no_provisioning();
    assert!(store.list_agent_assignments(task.id).unwrap().is_empty());
    assert!(store.worktree_claim(task.id).unwrap().is_none());
    assert_eq!(store.show_record(task.id).unwrap().status, forge_app::RecordStatus::Open);
    assert!(store.record_work_state(task.id).unwrap().input.is_none());
    // Reads remain available and do not claim, prepare or advance the parent.
    app.ready_records(false, false, true).unwrap();
    assert_eq!(store.show_record(parent.id).unwrap().status, forge_app::RecordStatus::Open);
    assert!(store.list_agent_assignments(task.id).unwrap().is_empty());
}

#[test]
fn legacy_started_plan_missing_inheritance_blocks_before_git_or_provider_work() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    fixture.configure(&app, &provider, "one");
    let store = fixture.store();
    let parent = store.open_typed_record(&NewTypedRecord {
        record_type: RecordType::Spec, body: "Legacy plan".into(),
        project_slugs: vec!["one/api".into()], generated_by_ids: vec![],
    }).unwrap();
    let first = store.open_task_under_spec("First", parent.id, vec![]).unwrap();
    let second = store.open_task_under_spec("Second", parent.id, vec![]).unwrap();
    store.relate_records(second.id, first.id, forge_app::RecordRelationshipKind::DependsOn).unwrap();
    // Reproduce a pre-gate snapshot using a separate connection, not a live recovery action.
    rusqlite::Connection::open(&fixture.database).unwrap().execute(
        "UPDATE records SET status = CASE WHEN id = ?1 THEN 'done' ELSE 'in_progress' END WHERE id IN (?1, ?2)",
        rusqlite::params![first.id.cast_signed(), parent.id.cast_signed()],
    ).unwrap();
    let checks = store.list_record_checks(second.id).unwrap();
    provider.reset();
    let error = app.dispatch_record(second.id).unwrap_err();
    assert_eq!(error.code, "invalid-spec-plan");
    assert!(error.message.contains(&format!("Task {}, prerequisite {}", second.id, first.id)));
    assert_eq!(app.execute_task_check_acceptance(second.id).unwrap_err().code, "invalid-spec-plan");
    app.reconcile_agent_dispatch("test", 1).unwrap();
    provider.no_provisioning();
    assert!(store.list_agent_assignments(second.id).unwrap().is_empty());
    assert!(store.worktree_claim(second.id).unwrap().is_none());
    assert_eq!(store.list_record_checks(second.id).unwrap(), checks);
}
