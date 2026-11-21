// T16: mixed-backend rollout/rollback through the production composition.
// Included into `workspace_dispatch_tests`, so it shares its Herdr `Provider` and `Fixture`.
use forge_app::durable_execution::launch_control::{DisableRequest, State as LaunchState};

fn durable_profile(fixture: &Fixture, workspace: &str) -> forge_app::AgentProfile {
    let root = fixture.root.join(workspace);
    std::fs::create_dir_all(root.join(".agents/profiles")).unwrap();
    std::fs::write(
        root.join(".agents/profiles/durable.md"),
        "---\nversion: 1\nruntime: pi-durable\nmodel: openai/gpt-4.1\nthinkingLevel: off\ntools: [bash]\n---\nWork",
    )
    .unwrap();
    let policy = forge_app::agent_profiles::ResourceCatalog::discover(
        &fixture.root.join("operator"),
        &root,
    )
    .unwrap()
    .resolve(&root, ".agents/profiles/durable.md")
    .unwrap();
    let profile = fixture
        .store()
        .register_agent_profile(workspace, RecordType::Task, &policy)
        .unwrap();
    assert_eq!(profile.provider, "pi-durable");
    profile
}

fn pin_template(fixture: &Fixture, project: &str, revision: u32) {
    fixture
        .store()
        .replace_check_template(
            project,
            &forge_app::check_templates::TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Pin durable".into(),
                criteria: vec![forge_app::check_templates::CheckCriterion {
                    name: "Implementation".into(),
                    description: "Deliver".into(),
                    profile_revision: Some(revision),
                }],
            },
        )
        .unwrap();
}

fn durable_task(fixture: &Fixture, workspace: &str) -> forge_app::Record {
    let root = fixture.root.join(workspace);
    git(&root, &["init"]).unwrap();
    git(
        &root,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.test",
            "commit",
            "--allow-empty",
            "-m",
            "base",
        ],
    )
    .unwrap();
    let base = git(&root, &["rev-parse", "HEAD"]).unwrap();
    let store = fixture.store();
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "implement".into(),
            project_slugs: vec![format!("{workspace}/api")],
            generated_by_ids: vec![],
        })
        .unwrap();
    store.set_task_base_commit(task.id, base.trim()).unwrap();
    task
}

fn daemon(fixture: &Fixture, app: Application) -> Daemon {
    let mut daemon = Daemon::new(app);
    daemon.durable_effect_resolution = Some(Arc::new(
        durable_authorization::operator::OperatorService::new(&fixture.database, None).unwrap(),
    ));
    daemon
}

/// The durable Task is refused (withheld, then rolled back) without any Herdr fallback,
/// allocation, Check-pin or selection rewrite, while Herdr Grilling keeps working.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn withheld_and_rolled_back_durable_launches_never_fall_back_to_herdr() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    fixture.configure(&app, &provider, "one");
    let durable = durable_profile(&fixture, "one");
    pin_template(&fixture, "one/api", durable.revision);
    // Registration selects the new revision; the durable backend is reached only through
    // the Check pin, so the Workspace keeps Herdr for its Grilling/legacy readiness.
    app.workspace_dispatch_config_set("one", "task-profile", "1", None)
        .unwrap();
    let store = fixture.store();
    store
        .set_project_task_concurrency_limit("one/api", Some(10))
        .unwrap();
    let task = durable_task(&fixture, "one");
    let checks = store.list_record_checks(task.id).unwrap();
    assert_eq!(checks[0].profile_revision, Some(durable.revision));
    let selection = store.workspace_dispatch_configuration("one").unwrap();

    let refused = |app: &Application, phase: &str| {
        provider.reset();
        // The pinned durable profile fails preflight; no Herdr substitute is selected.
        assert_eq!(
            app.dispatch_record(task.id).unwrap_err().code,
            "invalid-check-profile",
            "{phase}: explicit dispatch"
        );
        assert!(app.validate_record_check_profiles(task.id).is_err(), "{phase}");
        app.reconcile_agent_dispatch(phase, 1_000).unwrap();
        // No fallback: Herdr is never asked to provision the durable-pinned Task.
        provider.no_provisioning();
        assert!(store.list_agent_assignments(task.id).unwrap().is_empty(), "{phase}");
        assert!(
            forge_app::ManagedWorktreeRepository::worktree_claim(&store, task.id)
                .unwrap()
                .is_none(),
            "{phase}"
        );
        assert_eq!(store.list_record_checks(task.id).unwrap(), checks, "{phase}");
        assert_eq!(
            store.workspace_dispatch_configuration("one").unwrap(),
            selection,
            "{phase}"
        );
        assert_eq!(
            store.show_record(task.id).unwrap().status,
            forge_app::RecordStatus::Open,
            "{phase}"
        );
    };

    // Default: eligibility is withheld in production composition.
    let operator = daemon(&fixture, fixture.app());
    let status = operator.durable_launch_status().await.unwrap();
    assert_eq!(status.state, LaunchState::Withheld);
    assert!(!status.new_launches_admitted && status.rollback.is_none());
    refused(&app, "withheld");

    // Rollback is recorded through the operator RPC and is idempotent.
    let request = DisableRequest {
        operation_id: "rollback-1".into(),
        reason: "incident".into(),
    };
    let disabled = operator.durable_launch_disable(request.clone()).await.unwrap();
    assert_eq!(disabled.state, LaunchState::Disabled);
    assert!(!disabled.replayed && !disabled.new_launches_admitted);
    let again = operator
        .durable_launch_disable(DisableRequest {
            operation_id: "rollback-2".into(),
            reason: "other".into(),
        })
        .await
        .unwrap();
    assert!(again.replayed);
    assert_eq!(again.rollback, disabled.rollback);
    assert!(
        operator
            .durable_launch_disable(DisableRequest {
                operation_id: "bad id".into(),
                reason: "x".into(),
            })
            .await
            .is_err()
    );
    refused(&app, "rolled-back");

    // Store/daemon restart keeps the rollback.
    drop(app);
    drop(operator);
    let restarted = fixture.app();
    refused(&restarted, "restarted");
    let status = daemon(&fixture, fixture.app())
        .durable_launch_status()
        .await
        .unwrap();
    assert_eq!(status.state, LaunchState::Disabled);
    assert_eq!(status.rollback, disabled.rollback);

    // Herdr Grilling keeps its own prerequisites and still launches through Herdr.
    let grilling = fixture.grilling("one");
    let (assignment, duplicate) = restarted.dispatch_record(grilling.id).unwrap();
    assert!(!duplicate);
    assert_eq!(assignment.profile.provider, "herdr");
    assert!(
        provider
            .state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|call| call["method"] == "agent.start")
    );
}

/// An existing durable Assignment keeps its original backend and profile through
/// Workspace selection changes, rollback and restart; Herdr is never used for it.
#[tokio::test]
async fn configuration_change_and_rollback_do_not_redirect_existing_durable_assignment() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    fixture.configure(&app, &provider, "one");
    let durable = durable_profile(&fixture, "one");
    let store = fixture.store();
    let task = durable_task(&fixture, "one");
    let existing = store
        .create_agent_assignment(&forge_app::NewAgentAssignment {
            id: "existing-durable".into(),
            record_id: task.id,
            project_slug: "one/api".into(),
            profile_revision: durable.revision,
        })
        .unwrap();
    assert_eq!(existing.profile.provider, "pi-durable");

    // Mixed-backend configuration changes: the Workspace Task selection points at
    // Herdr revision 1 (unchanged) and durable launches are rolled back.
    app.workspace_dispatch_config_set("one", "task-profile", "1", None)
        .unwrap();
    let operator = daemon(&fixture, fixture.app());
    operator
        .durable_launch_disable(DisableRequest {
            operation_id: "rollback".into(),
            reason: "staged rollback".into(),
        })
        .await
        .unwrap();
    for (phase, app) in [("live", app), ("restarted", fixture.app())] {
        provider.reset();
        app.reconcile_agent_dispatch(phase, 2_000).unwrap();
        provider.no_provisioning();
        let current = store.agent_assignment("existing-durable").unwrap();
        assert_eq!(current.profile, existing.profile, "{phase}");
        assert_eq!(current.record_id, task.id, "{phase}");
        let assignments = store.list_agent_assignments(task.id).unwrap();
        assert_eq!(assignments.len(), 1, "{phase}: no duplicate/fallback Assignment");
        assert_eq!(assignments[0].id, "existing-durable");
        // Historical inspection remains available for the original Assignment.
        let info = app.agent_assignment_info("existing-durable").unwrap();
        assert_eq!(info.assignment.profile.provider, "pi-durable", "{phase}");
    }
}

/// Spec and dependency gates apply before any backend is consulted, so a durable pin
/// neither bypasses them nor is reported instead of them.
#[test]
fn durable_pinned_tasks_keep_spec_and_dependency_gates_before_backend_selection() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    fixture.configure(&app, &provider, "one");
    let durable = durable_profile(&fixture, "one");
    pin_template(&fixture, "one/api", durable.revision);
    app.workspace_dispatch_config_set("one", "task-profile", "1", None)
        .unwrap();
    let store = fixture.store();
    let parent = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Spec,
            body: "Planning".into(),
            project_slugs: vec!["one/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let first = store
        .open_task_under_spec("First", parent.id, vec![])
        .unwrap();
    let second = store
        .open_task_under_spec("Second", parent.id, vec![])
        .unwrap();
    store
        .relate_records(
            second.id,
            first.id,
            forge_app::RecordRelationshipKind::DependsOn,
        )
        .unwrap();
    assert_eq!(
        store.list_record_checks(first.id).unwrap()[0].profile_revision,
        Some(durable.revision)
    );
    provider.reset();
    for task in [first.id, second.id] {
        assert_eq!(
            app.dispatch_record(task).unwrap_err().code,
            "spec-execution-blocked"
        );
    }
    app.reconcile_agent_dispatch("spec-gate", 1).unwrap();
    provider.no_provisioning();
    for task in [first.id, second.id] {
        assert!(store.list_agent_assignments(task).unwrap().is_empty());
        assert!(
            forge_app::ManagedWorktreeRepository::worktree_claim(&store, task)
                .unwrap()
                .is_none()
        );
    }
}
