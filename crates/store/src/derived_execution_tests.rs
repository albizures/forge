//! Execution contracts use Builds-On alone, including read-only intermediate Tasks.
use super::*;

#[test]
fn derived_chain_prepares_non_default_root_and_carries_multiple_unchanged_results() {
    let store = fixture();
    let hosting = Hosting::default();
    let app = app(&store, &hosting);
    let [a, review, verification, c] = std::array::from_fn(|_| task(&store));
    store.configure_task_stack(a, &root()).unwrap();
    for (successor, predecessor) in [(review, a), (verification, review), (c, verification)] {
        store.relate_records(successor, predecessor, RecordRelationshipKind::BuildsOn).unwrap();
    }
    hosting.branches.lock().unwrap().insert("feature/payments".into(), "1".repeat(40));
    let prepared = app.prepare_task_stack(a).unwrap().unwrap();
    assert_eq!(prepared.base_commit, Some("1".repeat(40)));
    assert_eq!(prepared.base_branch, "feature/payments");
    complete(&store, a, &"a".repeat(40));
    let publication = store.task_stack(a).unwrap().unwrap().publication.unwrap();
    hosting.branches.lock().unwrap().insert(task_branch(a), "a".repeat(40));
    hosting.prs.lock().unwrap().insert(publication.url.clone(), observation(&publication));
    for id in [review, verification] {
        let inherited = app.prepare_task_stack(id).unwrap().unwrap();
        assert_eq!(inherited.base_commit, Some("a".repeat(40)));
        assert_eq!(inherited.base_branch, task_branch(a));
        cumulative::unchanged(&store, id, &"a".repeat(40));
        assert!(store.task_stack(id).unwrap().unwrap().publication.is_none());
        assert!(!hosting.branches.lock().unwrap().contains_key(&task_branch(id)));
    }
    let inherited = app.prepare_task_stack(c).unwrap().unwrap();
    assert_eq!(inherited.predecessor, Some(verification));
    assert_eq!(inherited.base_commit, Some("a".repeat(40)));
    assert_eq!(inherited.base_branch, task_branch(a));
    assert_eq!(app.prepare_task_stack(c).unwrap(), Some(inherited.clone()));
    let history = store.list_record_updates(c).unwrap();
    // Preparation already succeeded. Later manual start must still observe hosting,
    // without changing pins, lifecycle or history on failure.
    hosting.branches.lock().unwrap().remove(&task_branch(a));
    assert!(app.start_record(c).is_err());
    hosting.branches.lock().unwrap().insert(task_branch(a), "b".repeat(40));
    assert!(app.start_record(c).is_err());
    hosting.branches.lock().unwrap().insert(task_branch(a), "a".repeat(40));
    hosting.prs.lock().unwrap().remove(&publication.url);
    assert!(app.start_record(c).is_err());
    assert_eq!(store.task_stack(c).unwrap(), Some(inherited));
    assert_eq!(store.list_record_updates(c).unwrap(), history);
    assert_eq!(store.show_record(c).unwrap().status, RecordStatus::Open);
    hosting.prs.lock().unwrap().insert(publication.url.clone(), observation(&publication));
    app.start_record(c).unwrap();
    pass(&store, c, 0, &"c".repeat(40));
    pass(&store, c, 1, &"c".repeat(40));
    let publication = super::publication(c, &"c".repeat(40), &task_branch(a));
    hosting.prs.lock().unwrap().insert(publication.url.clone(), observation(&publication));
    hosting.branches.lock().unwrap().insert(task_branch(c), "c".repeat(40));
    app.publish_task_stack(c, &publication.url, None).unwrap();
    pass(&store, c, 2, &"c".repeat(40));
    app.done_record(c, None).unwrap();
}

#[test]
fn manual_attempt_reobserves_predecessor_hosting_after_successful_start() {
    let store = fixture();
    let hosting = Hosting::default();
    let app = app(&store, &hosting);
    let [a, b] = std::array::from_fn(|_| task(&store));
    store.configure_task_stack(a, &root()).unwrap();
    store.relate_records(b, a, RecordRelationshipKind::BuildsOn).unwrap();
    pin(&store, a, &"1".repeat(40));
    complete(&store, a, &"a".repeat(40));
    let publication = store.task_stack(a).unwrap().unwrap().publication.unwrap();
    hosting.branches.lock().unwrap().insert(task_branch(a), "a".repeat(40));
    hosting.prs.lock().unwrap().insert(publication.url.clone(), observation(&publication));
    let prepared = app.prepare_task_stack(b).unwrap().unwrap();
    app.start_record(b).unwrap();
    let before = store.record_work_state(b).unwrap();
    let history = store.list_record_updates(b).unwrap();
    let input = AttemptInput {
        id: "manual-after-start".into(),
        check_id: store.list_record_checks(b).unwrap()[0].id,
        generation: before.generation,
        evidence: Evidence::Repository {
            repository: store.project("alpha").unwrap().project_root.display().to_string(),
            commit: "a".repeat(40),
        },
        actor: "operator".into(),
    };
    for drift in ["deleted-branch", "moved-branch", "missing-pr", "closed-pr", "draft-pr"] {
        hosting.branches.lock().unwrap().insert(task_branch(a), "a".repeat(40));
        hosting.prs.lock().unwrap().insert(publication.url.clone(), observation(&publication));
        match drift {
            "deleted-branch" => { hosting.branches.lock().unwrap().remove(&task_branch(a)); }
            "moved-branch" => { hosting.branches.lock().unwrap().insert(task_branch(a), "b".repeat(40)); }
            "missing-pr" => { hosting.prs.lock().unwrap().remove(&publication.url); }
            "closed-pr" => { hosting.prs.lock().unwrap().get_mut(&publication.url).unwrap().open = false; }
            "draft-pr" => { hosting.prs.lock().unwrap().get_mut(&publication.url).unwrap().draft = true; }
            _ => unreachable!(),
        }
        let error = app.begin_record_attempt(b, &input).expect_err(drift);
        assert!(matches!(error.code, "stack-blocked" | "hosting-test"), "{drift}: {error:?}");
        assert!(error.message.contains("Stack Predecessor"), "{drift}: {error:?}");
        assert_eq!(store.record_work_state(b).unwrap(), before);
        assert_eq!(store.list_record_updates(b).unwrap(), history);
        assert_eq!(store.task_stack(b).unwrap(), Some(prepared.clone()));
        let attempts: i64 = store.connection.lock().unwrap().query_row(
            "SELECT COUNT(*) FROM record_attempts WHERE record_id=?1",
            [b.cast_signed()], |r| r.get(0),
        ).unwrap();
        assert_eq!(attempts, 0);
    }
    hosting.prs.lock().unwrap().insert(publication.url.clone(), observation(&publication));
    app.begin_record_attempt(b, &input).unwrap();
}

#[test]
fn missing_root_blocks_both_endpoints_without_fallback_or_partial_preparation() {
    let store = fixture();
    let hosting = Hosting::default();
    let app = app(&store, &hosting);
    let [a, b] = std::array::from_fn(|_| task(&store));
    store.relate_records(b, a, RecordRelationshipKind::BuildsOn).unwrap();
    for id in [a, b] {
        let history = store.list_record_updates(id).unwrap();
        let checks = store.list_record_checks(id).unwrap();
        for error in [store.validate_task_execution_plan(id).unwrap_err(), app.prepare_task_stack(id).unwrap_err(), app.start_record(id).unwrap_err()] {
            assert!(error.message.contains("root execution configuration"));
        }
        assert_eq!(store.list_record_updates(id).unwrap(), history);
        assert_eq!(store.list_record_checks(id).unwrap(), checks);
        assert_eq!(store.show_record(id).unwrap().status, RecordStatus::Open);
    }
    let ready = app.ready_records(false, false, false).unwrap();
    assert!(ready.is_empty());
    let blocked = app.ready_records(false, false, true).unwrap();
    assert_eq!(blocked.len(), 2);
    assert!(blocked.iter().all(|r| !r.execution_blockers.is_empty()));
}

#[test]
fn incomplete_derived_spec_cannot_start_or_rewrite_checks() {
    let store = fixture();
    let parent = store.open_typed_record(&NewTypedRecord {
        record_type: RecordType::Spec, project_slugs: vec!["alpha".into()],
        body: "Derived plan".into(), generated_by_ids: vec![],
    }).unwrap().id;
    let a = store.open_task_under_spec("Root", parent, vec![]).unwrap();
    let b = store.open_task_under_spec("Review", parent, vec![]).unwrap();
    for task in [&a, &b] {
        store.set_record_execution_mode(task.id, task.execution_mode, ExecutionMode::Manual, "operator", "Test").unwrap();
    }
    store.relate_records(b.id, a.id, RecordRelationshipKind::BuildsOn).unwrap();
    let history = store.list_record_updates(parent).unwrap();
    let checks = store.list_record_checks(a.id).unwrap();
    assert!(store.start_record(parent).unwrap_err().message.contains("root execution configuration"));
    assert_eq!(store.show_record(parent).unwrap().status, RecordStatus::Open);
    assert_eq!(store.list_record_updates(parent).unwrap(), history);
    assert_eq!(store.list_record_checks(a.id).unwrap(), checks);
    store.configure_task_stack(a.id, &root()).unwrap();
    store.start_record(parent).unwrap();
}

#[test]
fn incompatible_immutable_checks_block_standalone_authorization_and_preparation() {
    let store = fixture();
    // A new Task intentionally retains a template without Publication.
    store.replace_check_template("alpha", &TemplateReplacement {
        expected_version: 1, actor: "operator".into(), message: "Non-stacked work".into(),
        criteria: vec![CheckCriterion { name: "Implementation".into(), description: "Deliver".into(), profile_revision: None }],
    }).unwrap();
    let id = task(&store);
    store.configure_task_stack(id, &root()).unwrap();
    let checks = store.list_record_checks(id).unwrap();
    let history = store.list_record_updates(id).unwrap();
    let hosting = Hosting::default();
    hosting.branches.lock().unwrap().insert("feature/payments".into(), "a".repeat(40));
    let app = app(&store, &hosting);
    assert!(store.validate_task_execution_plan(id).is_err());
    assert!(app.prepare_task_stack(id).is_err());
    assert!(app.start_record(id).is_err());
    assert!(store.task_base_commit(id).unwrap().is_none());
    assert_eq!(store.list_record_checks(id).unwrap(), checks);
    assert_eq!(store.list_record_updates(id).unwrap(), history);
}
