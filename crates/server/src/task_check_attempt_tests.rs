#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn check_bound_rpc_context_and_outcome_are_same_owner_and_normal_dispatch_stays_closed() {
    use forge_app::{ManagedWorktreeRepository, WorkspaceDispatchRepository, WorkspaceRepository};
    let (fixture, _, _) = assignment_test_daemon_scoped(true);
    let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
    for assignment in ["owner-assignment", "target-assignment"] {
        store
            .close_agent_assignment(assignment, forge_app::AgentAssignmentClosureReason::Retired)
            .unwrap();
    }
    let workspace = store.workspace("test").unwrap();
    let skill = workspace.root.join(".agents/skills/work");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: work\ndescription: work\n---\nWork",
    )
    .unwrap();
    std::fs::write(
        skill.join("forge-agent.md"),
        "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\ntools: [read]\n---\nWork",
    )
    .unwrap();
    let policy = forge_app::agent_profiles::ResourceCatalog::discover(
        &workspace.root.join("operator"),
        &workspace.root,
    )
    .unwrap()
    .resolve(&workspace.root, ".agents/skills/work/forge-agent.md")
    .unwrap();
    let profile = store
        .register_agent_profile("test", RecordType::Task, &policy)
        .unwrap();
    store
        .replace_check_template(
            "alpha",
            &forge_app::check_templates::TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Pin".into(),
                criteria: vec![forge_app::check_templates::CheckCriterion {
                    name: "Implementation".into(),
                    description: "Deliver".into(),
                    profile_revision: Some(profile.revision),
                }],
            },
        )
        .unwrap();
    store
        .select_workspace_profile("test", RecordType::Task, Some(profile.revision))
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
            Some(&forge_app::DispatchWorkspace {
                id: "provider".into(),
            }),
            generation,
        )
        .unwrap();
    store
        .set_project_task_concurrency_limit("alpha", Some(10))
        .unwrap();
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let check = store.list_record_checks(task.id).unwrap().remove(0);
    let checkout = store.reserve_worktree_claim(task.id).unwrap();
    let repository = workspace.root.join("alpha");
    std::fs::create_dir_all(&repository).unwrap();
    super::git(&repository, &["init"]).unwrap();
    super::git(&repository, &["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "--allow-empty", "-m", "Input"]).unwrap();
    super::git(&repository, &["worktree", "add", "-b", "task-check", checkout.path.to_str().unwrap()]).unwrap();
    store.activate_worktree_claim(task.id).unwrap();
    let evidence = forge_app::outcomes::Evidence::Repository {
        repository: workspace.root.join("alpha").display().to_string(),
        commit: super::git(&checkout.path, &["rev-parse", "HEAD"]).unwrap(),
    };
    store
        .claim_task_check_acceptance(
            task.id,
            "bound-check",
            &forge_app::DispatchPreflightSelection {
                task_revision: profile.revision,
                grilling_revision: 1,
                binding_id: "provider".into(),
                provider_generation: generation,
            },
            &evidence,
            check.id,
            profile.revision,
        )
        .unwrap();
    let root = fixture.database_path.with_extension("assignment-secrets");
    std::fs::write(root.join("bound-check"), "check-secret").unwrap();
    let mut daemon = Daemon::new(
        Application::with_managed_worktrees_and_agent_dispatch(
            Box::new(SqliteProjectRepository::open(&fixture.database_path).unwrap()),
            Box::new(SqliteProjectRepository::open(&fixture.database_path).unwrap()),
            Box::new(super::GitWorktreeAdapter),
            Box::new(SqliteProjectRepository::open(&fixture.database_path).unwrap()),
            Box::new(super::ServerAgentDispatchAdapterFactory {
                herdr: super::HerdrAdapterFactory::from_configuration_store(
                    fixture.database_path.clone(),
                ),
                daemon_url: "http://127.0.0.1:45003".into(),
                capability_secret_root: root.clone(),
                cli_override: None,
            }),
            Box::new(super::GitInspector),
        )
        .with_workspace_repository(Box::new(
            SqliteProjectRepository::open(&fixture.database_path).unwrap(),
        )),
    );
    daemon.capability_secret_root = Some(root);
    let credentials = serde_json::json!({"assignmentId":"bound-check", "secret":"check-secret"});
    let boot = assignment_rpc(daemon.clone(), "assignment.bootstrap", credentials.clone()).await;
    assert_eq!(
        boot["result"]["context"]["bound"]["checkAttempt"]["check_id"], check.id,
        "{boot}"
    );
    // Implementation may commit clean output B rather than re-report input A.
    std::fs::write(checkout.path.join("deliverable.txt"), "Implemented\n").unwrap();
    super::git(&checkout.path, &["add", "deliverable.txt"]).unwrap();
    super::git(&checkout.path, &["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "Output"]).unwrap();
    let output = forge_app::outcomes::Evidence::Repository {
        repository: repository.display().to_string(),
        commit: super::git(&checkout.path, &["rev-parse", "HEAD"]).unwrap(),
    };
    assert_ne!(evidence, output);
    let outcome = forge_app::outcomes::OutcomeInput {
        attempt_id: "bound-check".into(),
        generation: 1,
        evidence: output,
        verdict: forge_app::checks::Verdict::Pass,
        actor: "spoofed".into(),
        message: "Delivered".into(),
        disposition: forge_app::outcomes::OutcomeDisposition::Complete,
        findings: vec![],
        verifications: vec![],
    };
    let mut request = credentials;
    request["outcome"] = serde_json::to_value(outcome).unwrap();
    let mut unauthorized = request.clone();
    unauthorized["secret"] = "wrong".into();
    assert!(
        assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", unauthorized)
            .await
            .get("error")
            .is_some()
    );
    assert!(
        store.list_record_checks(task.id).unwrap()[0]
            .latest_assessment
            .is_none()
    );
    let mut foreign = request.clone();
    foreign["outcome"]["attempt_id"] = "owner-assignment".into();
    assert!(
        assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", foreign)
            .await
            .get("error")
            .is_some()
    );
    let accepted =
        assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", request.clone()).await;
    assert_eq!(
        accepted["result"]["attempt"]["actor"], "bound-check",
        "{accepted}"
    );
    assert_eq!(
        accepted["result"]["check"]["latest_assessment"]["assignment_id"], "bound-check",
        "{accepted}"
    );
    assert_eq!(
        assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", request.clone()).await["result"],
        accepted["result"]
    );
    request["outcome"]["message"] = "Conflicting replay".into();
    assert!(
        assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", request)
            .await
            .get("error")
            .is_some()
    );
    // Typed public RPC confirms normal dispatch remains closed even while a bound Assignment exists.
    let normal = AgentDispatchApiServer::dispatch_record(
        &daemon,
        forge_rpc::SelectedRecordRequest {
            selection: forge_rpc::RecordSelection {
                workspace: Some("test".into()),
                ..Default::default()
            },
            record: forge_rpc::DispatchRecordRequest { record_id: task.id },
        },
    )
    .await;
    assert!(
        normal
            .unwrap_err()
            .to_string()
            .contains("task-dispatch-unsupported-contract")
    );
    assert_eq!(
        store.show_record(task.id).unwrap().status,
        forge_app::RecordStatus::InProgress
    );
}
