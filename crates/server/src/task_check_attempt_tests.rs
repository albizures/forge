include!("retained_work_tests.rs");

#[tokio::test]
async fn check_bound_rpc_context_outcome_and_normal_dispatch_share_same_owner() {
    check_bound_outcome_inspection(0).await;
}

#[tokio::test]
async fn clean_retained_work_recovery_rpc_checks_git_scope_and_operator_authority() {
    check_bound_outcome_inspection(3).await;
}

#[tokio::test]
async fn dirty_retained_work_recovery_rpc_binds_staged_tracked_and_untracked_bytes() {
    check_bound_outcome_inspection(4).await;
}

#[allow(clippy::too_many_lines)]
async fn check_bound_outcome_inspection(kind: u8) {
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
    std::fs::create_dir_all(workspace.root.join(".agents/profiles")).unwrap();
    std::fs::write(
        workspace.root.join(".agents/profiles/work.md"),
        "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: [read]\n---\nWork",
    )
    .unwrap();
    let policy = forge_app::agent_profiles::ResourceCatalog::discover(
        &workspace.root.join("operator"),
        &workspace.root,
    )
    .unwrap()
    .resolve(&workspace.root, ".agents/profiles/work.md")
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
    let parent = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Spec,
            body: "Validated execution plan".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let task = store.open_task_under_spec("Deliver", parent.id, vec![]).unwrap();
    store.start_record(parent.id).unwrap();
    let check = store.list_record_checks(task.id).unwrap().remove(0);
    let checkout = store.reserve_worktree_claim(task.id).unwrap();
    let repository = workspace.root.join("alpha");
    std::fs::create_dir_all(&repository).unwrap();
    super::git(&repository, &["init"]).unwrap();
    super::git(&repository, &["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "--allow-empty", "-m", "Input"]).unwrap();
    let branch = format!("feat/forge-{}", task.id);
    super::git(&repository, &["worktree", "add", "-b", &branch, checkout.path.to_str().unwrap()]).unwrap();
    store.set_task_base_commit(task.id, &super::git(&checkout.path, &["rev-parse", "HEAD"]).unwrap()).unwrap();
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
            None,
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
    // The Implementation bootstrap is minimal; Check identity stays readable via context.
    assert!(boot["result"]["commandContract"].is_string(), "{boot}");
    assert!(boot["result"].get("context").is_none(), "{boot}");
    let context = assignment_rpc(daemon.clone(), "assignment.context", credentials.clone()).await;
    assert_eq!(
        context["result"]["bound"]["checkAttempt"]["check_id"], check.id,
        "{context}"
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
    // Normal typed RPC shares the existing Check-bound ownership without relaunching.
    let normal = AgentDispatchApiServer::dispatch_record(
        &daemon,
        forge_rpc::SelectedRecordRequest {
            selection: forge_rpc::RecordSelection {
                workspace: Some("test".into()),
                ..Default::default()
            },
            record: forge_rpc::DispatchRecordRequest { record_id: task.id },
        },
    ).await.unwrap();
    assert!(normal.already_assigned);
    assert_eq!(normal.assignment.id, "bound-check");
    // Reproduce the changed-commit interruption incident before accepting B normally.
    let before_work = store.record_work_state(task.id).unwrap();
    let before_checks = store.list_record_checks(task.id).unwrap();
    let before_owner = store.agent_assignment_info("bound-check").unwrap();
    let before_claim = store.worktree_claim(task.id).unwrap();
    let before_updates = store.list_record_updates(task.id).unwrap();
    let before_findings = store.list_record_findings(task.id, None, 100).unwrap();
    let mut interrupted = request.clone();
    interrupted["outcome"]["disposition"] = "interrupted".into();
    interrupted["outcome"]["verdict"] = "fail".into();
    for (field, value) in [("secret", "wrong"), ("attempt_id", "owner-assignment")] {
        let mut denied = interrupted.clone();
        if field == "secret" { denied[field] = value.into(); } else { denied["outcome"][field] = value.into(); }
        let response = assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", denied).await;
        assert!(response.get("error").is_some(), "{response}");
        assert_ne!(response["error"]["data"], "interrupted-evidence-changed", "{response}");
    }
    for dirty in [false, true] {
        if dirty {
            std::fs::write(checkout.path.join("deliverable.txt"), "Retained edits\n").unwrap();
            std::fs::write(checkout.path.join("untracked.txt"), "Retained new work\n").unwrap();
        }
        let rejected = assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", interrupted.clone()).await;
        assert_eq!(rejected["error"]["data"], "implementation-outcome-forbidden", "{rejected}");
        assert_eq!(store.record_work_state(task.id).unwrap(), before_work);
        assert_eq!(store.list_record_checks(task.id).unwrap(), before_checks);
        assert_eq!(store.agent_assignment_info("bound-check").unwrap(), before_owner);
        assert_eq!(store.worktree_claim(task.id).unwrap(), before_claim);
        assert_eq!(store.list_record_updates(task.id).unwrap(), before_updates);
        assert_eq!(store.list_record_findings(task.id, None, 100).unwrap(), before_findings);
        assert!(store.dispatch_hold(task.id).unwrap().is_none());
        assert_eq!(super::git(&checkout.path, &["rev-parse", "HEAD"]).unwrap(), request["outcome"]["evidence"]["commit"].as_str().unwrap());
        if dirty {
            assert_eq!(std::fs::read_to_string(checkout.path.join("deliverable.txt")).unwrap(), "Retained edits\n");
            assert_eq!(std::fs::read_to_string(checkout.path.join("untracked.txt")).unwrap(), "Retained new work\n");
        }
    }
    // Fixture-only restoration, not a product repair path.
    std::fs::write(checkout.path.join("deliverable.txt"), "Implemented\n").unwrap();
    std::fs::remove_file(checkout.path.join("untracked.txt")).unwrap();
    if kind >= 3 {
        verify_clean_recovery_rpc(&daemon, &store, task.id, &checkout.path, &request, kind == 4, &fixture.database_path).await;
        return;
    }
    let credentials_for_denials = serde_json::json!({"assignmentId":"bound-check", "secret":"check-secret"});
    let mut forbidden = request.clone();
    for (verdict, disposition) in [
        ("pass", serde_json::json!("complete")),
        ("fail", serde_json::json!("complete")),
        ("fail", serde_json::json!("handback")),
        ("fail", serde_json::json!("human-input")),
        ("fail", serde_json::json!("interrupted")),
        ("fail", serde_json::json!({"continuation": {
            "stopping_reason": "Context budget nearly exhausted",
            "completed_work": "Implemented checkpoint",
            "verification": "Focused tests passed",
            "remaining_work": "Finish and run full gates"
        }})),
    ] {
        forbidden["outcome"]["verdict"] = verdict.into();
        forbidden["outcome"]["disposition"] = disposition;
        let rejected = assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", forbidden.clone()).await;
        assert_eq!(rejected["error"]["data"], "implementation-outcome-forbidden", "{rejected}");
        assert_eq!(store.record_work_state(task.id).unwrap(), before_work);
        assert_eq!(store.list_record_checks(task.id).unwrap(), before_checks);
        assert_eq!(store.list_record_updates(task.id).unwrap(), before_updates);
    }
    // Verdict-adjacent commands are closed to the Implementation Agent as well.
    let with = |extra: serde_json::Value| {
        let mut value = credentials_for_denials.clone();
        value.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        value
    };
    for (method, params) in [
        ("assignment.checkAssess", with(serde_json::json!({"check": check.id, "expectedVersion": 0, "verdict": "pass", "message": "Done"}))),
        ("assignment.checkInvalidate", with(serde_json::json!({"check": check.id, "expectedVersion": 0, "assessmentId": 1, "message": "No"}))),
        ("assignment.findingRaise", with(serde_json::json!({"severity": "major", "blocking": true, "message": "Self-raised"}))),
        ("assignment.findingMutate", with(serde_json::json!({"finding": 1, "expectedVersion": 1, "change": {"action": "resolve"}, "message": "Self-resolved"}))),
    ] {
        let rejected = assignment_rpc(daemon.clone(), method, params).await;
        assert_eq!(rejected["error"]["data"], "implementation-outcome-forbidden", "{method}: {rejected}");
        assert_eq!(store.list_record_checks(task.id).unwrap(), before_checks);
        assert_eq!(store.list_record_updates(task.id).unwrap(), before_updates);
        assert_eq!(store.list_record_findings(task.id, None, 100).unwrap(), before_findings);
    }
    // The Review Request is the Implementation Agent's only way to finish; Forge derives it.
    let credentials = serde_json::json!({"assignmentId":"bound-check", "secret":"check-secret"});
    let accepted = assignment_rpc(daemon.clone(), "assignment.reviewRequest", credentials.clone()).await;
    assert_eq!(accepted["result"]["attempt"]["actor"], "bound-check", "{accepted}");
    assert_eq!(accepted["result"]["evidence"], request["outcome"]["evidence"], "{accepted}");
    assert_eq!(
        accepted["result"]["check"]["latest_assessment"]["assignment_id"], "bound-check",
        "{accepted}"
    );
    assert_eq!(
        assignment_rpc(daemon.clone(), "assignment.reviewRequest", credentials).await["result"],
        accepted["result"]
    );
    assert_eq!(
        store.show_record(task.id).unwrap().status,
        forge_app::RecordStatus::InProgress
    );
}
