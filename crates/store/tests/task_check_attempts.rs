use forge_app::{
    AgentDispatchRepository, DispatchPreflightSelection, DispatchWorkspace,
    ManagedWorktreeRepository, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordType, WorkspaceDispatchRepository, WorkspaceRepository,
};
use forge_app::{
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
    outcomes::Evidence,
};
use forge_store::SqliteProjectRepository;
#[path = "support/task_attempt_adapters.rs"]
mod adapters;
use adapters::{Local, Provider};

#[test]
#[allow(clippy::too_many_lines)]
fn acceptance_claim_snapshots_first_unpassed_check_and_pinned_profile_atomically() {
    let root = std::env::temp_dir().join(format!(
        "forge-task-attempt-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join(".agents/skills/work")).unwrap();
    std::fs::write(
        root.join(".agents/skills/work/SKILL.md"),
        "---\nname: work\ndescription: work\n---\nWork",
    )
    .unwrap();
    std::fs::write(
        root.join(".agents/skills/work/forge-agent.md"),
        "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\ntools: [read]\n---\nWork",
    )
    .unwrap();
    let path = root.join("store.sqlite");
    let store = SqliteProjectRepository::open(&path).unwrap();
    store.create_workspace("test", "Test", &root).unwrap();
    store
        .register(&Project {
            reference: "test/api".into(),
            workspace_id: "test".into(),
            slug: "api".into(),
            project_root: root.clone(),
            origin_url: "https://example.test/api.git".into(),
        })
        .unwrap();
    let policy = ResourceCatalog::discover(&root.join("operator"), &root)
        .unwrap()
        .resolve(&root, ".agents/skills/work/forge-agent.md")
        .unwrap();
    let pinned = store
        .register_agent_profile("test", RecordType::Task, &policy)
        .unwrap();
    let drift = store
        .register_agent_profile("test", RecordType::Task, &policy)
        .unwrap();
    store
        .select_workspace_profile("test", RecordType::Task, Some(drift.revision))
        .unwrap();
    store
        .select_workspace_profile("test", RecordType::Grilling, Some(1))
        .unwrap();
    let generation = store
        .workspace_dispatch_configuration("test")
        .unwrap()
        .provider_generation;
    store
        .retain_verified_workspace_binding(
            "test",
            Some(&DispatchWorkspace {
                id: "provider".into(),
            }),
            generation,
        )
        .unwrap();
    store
        .replace_check_template(
            "test/api",
            &TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Pin".into(),
                criteria: ["Implementation", "Review"]
                    .map(|name| CheckCriterion {
                        name: name.into(),
                        description: name.into(),
                        profile_revision: Some(pinned.revision),
                    })
                    .into(),
            },
        )
        .unwrap();
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            project_slugs: vec!["test/api".into()],
            body: "Deliver work".into(),
            generated_by_ids: vec![],
        })
        .unwrap();
    let checks = store.list_record_checks(task.id).unwrap();
    store.reserve_worktree_claim(task.id).unwrap();
    store.activate_worktree_claim(task.id).unwrap();
    let selection = DispatchPreflightSelection {
        task_revision: drift.revision,
        grilling_revision: 1,
        binding_id: "provider".into(),
        provider_generation: generation,
    };
    let input = Evidence::Repository {
        repository: root.display().to_string(),
        commit: "a".repeat(40),
    };
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let claims: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let selection = selection.clone();
            let input = input.clone();
            let check = checks[0].id;
            let revision = pinned.revision;
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let store = SqliteProjectRepository::open(&path).unwrap();
                barrier.wait();
                store
                    .claim_task_check_acceptance(
                        task.id,
                        "attempt-1",
                        &selection,
                        &input,
                        check,
                        revision,
                    )
                    .unwrap()
            })
        })
        .collect();
    let claims: Vec<_> = claims
        .into_iter()
        .map(|claim| claim.join().unwrap())
        .collect();
    assert_eq!(claims.iter().filter(|(_, existing)| !existing).count(), 1);
    assert_eq!(claims[0].0, claims[1].0);
    let assignment = claims[0].0.clone();
    assert_eq!(assignment.profile.revision, pinned.revision);
    let attempt = store
        .assignment_check_attempt("attempt-1")
        .unwrap()
        .unwrap();
    assert_eq!(attempt.id, "attempt-1");
    assert_eq!(attempt.check_id, checks[0].id);
    assert_eq!(attempt.generation, 1);
    assert_eq!(attempt.evidence, input);
    assert_eq!(attempt.actor, "attempt-1");
    assert_eq!(
        store
            .claim_task_check_acceptance(
                task.id,
                "attempt-2",
                &selection,
                &input,
                checks[0].id,
                pinned.revision
            )
            .unwrap(),
        (assignment, true)
    );
    drop(store);
    let store = SqliteProjectRepository::open(&path).unwrap();
    assert_eq!(
        store.assignment_check_attempt("attempt-1").unwrap(),
        Some(attempt)
    );
    assert_eq!(store.list_agent_assignments(task.id).unwrap().len(), 1);
    store.acknowledge_agent_assignment("attempt-1").unwrap();
    store.start_owned_record(task.id, "attempt-1").unwrap();
    store.mark_assignment_context_read("attempt-1").unwrap();
    let outcome = forge_app::outcomes::OutcomeInput {
        attempt_id: "attempt-1".into(),
        generation: 1,
        evidence: input,
        verdict: forge_app::checks::Verdict::Fail,
        actor: "attempt-1".into(),
        message: "Implementation failed".into(),
        disposition: forge_app::outcomes::OutcomeDisposition::Complete,
        findings: vec![],
        verifications: vec![],
    };
    assert_eq!(
        store
            .submit_record_outcome(task.id, &outcome)
            .unwrap_err()
            .code,
        "assignment-outcome-required"
    );
    assert_eq!(
        store
            .submit_assignment_outcome(task.id, "other", &outcome)
            .unwrap_err()
            .code,
        "unauthorized-attempt"
    );
    let accepted = store
        .submit_assignment_outcome(task.id, "attempt-1", &outcome)
        .unwrap();
    assert_eq!(
        accepted.check.current_verdict,
        Some(forge_app::checks::Verdict::Fail)
    );
    assert_eq!(
        store
            .submit_assignment_outcome(task.id, "attempt-1", &outcome)
            .unwrap(),
        accepted
    );
    assert_eq!(
        store.list_record_checks(task.id).unwrap()[1].current_verdict,
        None
    );
    store
        .close_agent_assignment(
            "attempt-1",
            forge_app::AgentAssignmentClosureReason::Retired,
        )
        .unwrap();
    // Failure leaves Implementation selected; neither retry identity nor live selections can skip it.
    assert_eq!(
        store
            .claim_task_check_acceptance(
                task.id,
                "skip-review",
                &selection,
                &outcome.evidence,
                checks[1].id,
                pinned.revision
            )
            .unwrap_err()
            .code,
        "check-selection-stale"
    );
    let unavailable = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let launches = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = forge_app::Application::with_managed_worktrees_and_agent_dispatch(
        Box::new(SqliteProjectRepository::open(&path).unwrap()),
        Box::new(SqliteProjectRepository::open(&path).unwrap()),
        Box::new(Local),
        Box::new(SqliteProjectRepository::open(&path).unwrap()),
        Box::new(Provider {
            unavailable: unavailable.clone(),
            launches: launches.clone(),
        }),
        Box::new(Local),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(&path).unwrap()));
    assert_eq!(
        app.dispatch_record(task.id).unwrap_err().code,
        "task-dispatch-unsupported-contract"
    );
    assert_eq!(
        app.execute_task_check_acceptance(task.id).unwrap_err().code,
        "invalid-check-profile"
    );
    assert!(launches.lock().unwrap().is_empty());
    unavailable.store(false, std::sync::atomic::Ordering::SeqCst);
    let (retry, existing) = app.execute_task_check_acceptance(task.id).unwrap();
    assert!(!existing);
    assert_eq!(retry.profile.revision, pinned.revision);
    assert_eq!(*launches.lock().unwrap(), vec![pinned.revision]);
    assert_eq!(
        app.execute_task_check_acceptance(task.id).unwrap(),
        (retry.clone(), true)
    );
    let secrets = root.join("secrets");
    std::fs::create_dir_all(&secrets).unwrap();
    std::fs::write(secrets.join(&retry.id), "actual-secret").unwrap();
    assert!(app.assignment(&secrets, &retry.id, "wrong").is_err());
    let session = app
        .assignment(&secrets, &retry.id, "actual-secret")
        .unwrap();
    session.acknowledge().unwrap();
    session.start().unwrap();
    let context = session.context().unwrap();
    assert_eq!(
        context.bound.check_attempt.as_ref().unwrap().check_id,
        checks[0].id
    );
    assert_eq!(context.bound.profile_revision, pinned.revision);
    let mut pass = outcome.clone();
    pass.attempt_id = retry.id.clone();
    pass.actor = "spoofed actor is replaced".into();
    pass.verdict = forge_app::checks::Verdict::Pass;
    pass.message = "Implemented".into();
    assert_eq!(
        session.submit_outcome(&outcome).unwrap_err().code,
        "unauthorized-attempt"
    );
    let mut forged = pass.clone();
    forged.evidence = Evidence::Artifact {
        uri: "urn:bypass".into(),
        sha256: "b".repeat(64),
    };
    assert_eq!(
        session.submit_outcome(&forged).unwrap_err().code,
        "evidence-mismatch"
    );
    let accepted = session.submit_outcome(&pass).unwrap();
    assert_eq!(accepted.attempt.actor, retry.id);
    assert_eq!(session.submit_outcome(&pass).unwrap(), accepted);
    assert_eq!(
        session
            .check_invalidate(
                checks[0].id,
                &forge_app::checks::InvalidationInput {
                    expected_version: accepted.check.version,
                    assessment_id: accepted.check.latest_assessment.as_ref().unwrap().id,
                    actor: "ignored".into(),
                    message: "Agent must not select handback policy".into(),
                    revision: None,
                    dirty: None,
                }
            )
            .unwrap_err()
            .code,
        "attempt-required"
    );
    store
        .close_agent_assignment(&retry.id, forge_app::AgentAssignmentClosureReason::Retired)
        .unwrap();
    let (review, _) = app.execute_task_check_acceptance(task.id).unwrap();
    assert_eq!(
        store
            .assignment_check_attempt(&review.id)
            .unwrap()
            .unwrap()
            .check_id,
        checks[1].id
    );
    assert_eq!(
        *launches.lock().unwrap(),
        vec![pinned.revision, pinned.revision]
    );
    assert_eq!(
        app.show_record(task.id).unwrap().status,
        forge_app::RecordStatus::InProgress
    );
    assert_eq!(
        app.dispatch_record(task.id).unwrap_err().code,
        "task-dispatch-unsupported-contract"
    );
    for (index, revision) in [None, Some(999_999), Some(1)].into_iter().enumerate() {
        store
            .replace_check_template(
                "test/api",
                &TemplateReplacement {
                    expected_version: (index + 1) as u64,
                    actor: "operator".into(),
                    message: "Unusable pin".into(),
                    criteria: vec![CheckCriterion {
                        name: "Implementation".into(),
                        description: "Deliver".into(),
                        profile_revision: revision,
                    }],
                },
            )
            .unwrap();
        let invalid = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                project_slugs: vec!["test/api".into()],
                body: "Blocked profile".into(),
                generated_by_ids: vec![],
            })
            .unwrap();
        assert_eq!(
            app.execute_task_check_acceptance(invalid.id)
                .unwrap_err()
                .code,
            "invalid-check-profile"
        );
        assert!(store.list_agent_assignments(invalid.id).unwrap().is_empty());
    }
    assert_eq!(
        *launches.lock().unwrap(),
        vec![pinned.revision, pinned.revision]
    );
    let manual = store
        .records_in_workspace("test")
        .unwrap()
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            project_slugs: vec![],
            body: "Manual only".into(),
            generated_by_ids: vec![],
        })
        .unwrap();
    assert_eq!(
        app.execute_task_check_acceptance(manual.id)
            .unwrap_err()
            .code,
        "manual-dispatch-forbidden"
    );
    let spec = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Spec,
            project_slugs: vec!["test/api".into()],
            body: "Not executable".into(),
            generated_by_ids: vec![],
        })
        .unwrap();
    assert_eq!(
        app.execute_task_check_acceptance(spec.id).unwrap_err().code,
        "unsupported-record-type"
    );
    app.reconcile_agent_dispatch("acceptance-test", 100_000)
        .unwrap();
    assert_eq!(
        *launches.lock().unwrap(),
        vec![pinned.revision, pinned.revision]
    );
    assert_eq!(store.agent_assignment(&review.id).unwrap(), review);
    std::fs::remove_dir_all(root).unwrap();
}
