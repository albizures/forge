#[tokio::test]
async fn check_bound_rpc_context_outcome_and_normal_dispatch_share_same_owner() {
    check_bound_outcome_inspection(0).await;
}

#[tokio::test]
async fn task_inspection_after_accepted_human_input_and_clean_retirement() {
    check_bound_outcome_inspection(1).await;
}

#[tokio::test]
async fn implementation_continuation_rpc_retains_clean_checkpoint_and_handoff() {
    check_bound_outcome_inspection(2).await;
}

#[allow(clippy::too_many_lines)]
async fn check_bound_outcome_inspection(kind: u8) {
    use forge_app::{ManagedWorktreeRepository, WorkspaceDispatchRepository, WorkspaceRepository};
    let human_input = kind == 1;
    let continuation = kind == 2;
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
        verdict: if human_input { forge_app::checks::Verdict::Fail } else { forge_app::checks::Verdict::Pass },
        actor: "spoofed".into(),
        message: "Delivered".into(),
        disposition: if human_input { forge_app::outcomes::OutcomeDisposition::HumanInput } else { forge_app::outcomes::OutcomeDisposition::Complete },
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
        assert_eq!(rejected["error"]["data"], "interrupted-evidence-changed", "{rejected}");
        let message = rejected["error"]["message"].as_str().unwrap();
        assert!(message.contains("Continuation"), "{message}");
        assert!(message.contains("operator recovery"), "{message}");
        assert!(message.contains("interrupted-work recovery is not available"), "{message}");
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
    if continuation {
        request["outcome"]["verdict"] = "fail".into();
        request["outcome"]["disposition"] = serde_json::json!({"continuation": {
            "stopping_reason": "Context budget nearly exhausted",
            "completed_work": "Implemented checkpoint",
            "verification": "Focused tests passed; full gates unrun due to constraint",
            "remaining_work": "Finish and run full gates"
        }});
        for (field, value) in [("secret", "wrong"), ("attempt_id", "owner-assignment")] {
            let mut denied = request.clone();
            if field == "secret" { denied[field] = value.into(); } else { denied["outcome"][field] = value.into(); }
            assert!(assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", denied).await.get("error").is_some());
            assert_eq!(store.record_work_state(task.id).unwrap(), before_work);
        }
        for field in ["stopping_reason", "completed_work", "verification", "remaining_work"] {
            let mut missing = request.clone();
            missing["outcome"]["disposition"]["continuation"].as_object_mut().unwrap().remove(field);
            assert!(assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", missing).await.get("error").is_some());
            assert_eq!(store.record_work_state(task.id).unwrap(), before_work);
        }
        std::fs::write(checkout.path.join("untracked.txt"), "Retain dirty work").unwrap();
        let rejected = assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", request.clone()).await;
        assert_eq!(rejected["error"]["data"], "invalid-continuation", "{rejected}");
        assert_eq!(store.record_work_state(task.id).unwrap(), before_work);
        assert!(store.dispatch_hold(task.id).unwrap().is_none());
        std::fs::remove_file(checkout.path.join("untracked.txt")).unwrap();
    }
    let accepted =
        assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", request.clone()).await;
    assert_eq!(
        accepted["result"]["attempt"]["actor"], "bound-check",
        "{accepted}"
    );
    if continuation {
        assert!(accepted["result"]["check"]["latest_assessment"].is_null(), "{accepted}");
        let work = store.record_work_state(task.id).unwrap();
        assert_eq!(work.input, Some(serde_json::from_value(request["outcome"]["evidence"].clone()).unwrap()));
        assert!(work.output.is_none());
        assert_eq!(work.progress.continuations[0].total, 1);
        assert_eq!(work.progress.continuations[0].consecutive_unchanged, 0);
        assert_eq!(work.progress.continuations[0].checkpoint.handoff.remaining_work, "Finish and run full gates");
        assert_eq!(store.worktree_claim(task.id).unwrap(), before_claim);
        let reset = forge_rpc::ResetContinuationsRequest {
            record: task.id, check: work.progress.continuations[0].check_id,
            actor: "operator".into(), message: "Investigated checkpoint".into(),
        };
        let denied = assignment_rpc(daemon.clone(), "assignment.resetContinuations", serde_json::json!({
            "assignment_id": "bound-check", "secret": "check-secret", "record": task.id, "check": reset.check,
            "actor": "operator", "message": "impersonation"
        })).await;
        assert!(denied.get("error").is_some(), "{denied}");
        assert_eq!(store.record_work_state(task.id).unwrap(), work);
        let mut invalid = reset.clone();
        invalid.actor = " ".into();
        let selection = forge_rpc::RecordSelection { workspace: Some("test".into()), ..Default::default() };
        assert!(forge_rpc::RecordApiServer::reset_continuations(&daemon, forge_rpc::SelectedRecordRequest { selection: selection.clone(), record: invalid }).await.is_err());
        let state = forge_rpc::RecordApiServer::reset_continuations(&daemon, forge_rpc::SelectedRecordRequest { selection: selection.clone(), record: reset }).await.unwrap();
        assert_eq!(state.progress.continuations[0].total, 0);
        assert_eq!(state.progress.continuations[0].resets.len(), 1);
        let inspected = forge_rpc::RecordApiServer::record_work_state(&daemon, forge_rpc::SelectedRecordRequest { selection, record: forge_rpc::RecordWorkStateRequest { record: task.id } }).await.unwrap();
        assert_eq!(inspected, state);
        assert_eq!(store.worktree_claim(task.id).unwrap(), before_claim);
    } else if human_input {
        assert!(accepted["result"]["check"]["latest_assessment"].is_null());
        // Persist the clean retirement facts; no external provider is needed by inspection.
        for resource in store.list_agent_assignment_resources("bound-check").unwrap() {
            store.retire_agent_assignment_resource("bound-check", &resource.resource_kind, &resource.resource_id).unwrap();
        }
        store.close_agent_assignment("bound-check", forge_app::AgentAssignmentClosureReason::Retired).unwrap();
        let before = store.agent_assignment_info("bound-check").unwrap();
        assert_eq!(before.assignment.state, forge_app::AgentAssignmentState::Working);
        let selection = forge_rpc::RecordSelection { workspace: Some("test".into()), ..Default::default() };
        let shown = forge_rpc::RecordApiServer::show(&daemon, forge_rpc::SelectedRecordRequest {
            selection: selection.clone(), record: forge_rpc::RecordIdRequest { id: task.id },
        }).await.unwrap();
        let listed = forge_rpc::RecordApiServer::list(&daemon, forge_rpc::SelectedRecordRequest {
            selection,
            record: forge_rpc::ListRecordsRequest { project_slug: None, status: None, record_type: Some("task".into()) },
        }).await.unwrap();
        assert_eq!(listed.records.iter().find(|r| r.id == task.id), Some(&shown));
        assert_eq!(shown.status, "in_progress");
        let activity = shown.activity.as_ref().unwrap();
        assert_eq!(activity.state, forge_rpc::TaskActivityState::Closed);
        assert_eq!(activity.retained_resources, 0);
        assert!(activity.dispatch_hold.as_ref().unwrap().contains("Human input required"));
        assert!(shown.checks.as_ref().unwrap().iter().all(|c| c.verdict().is_none()));
        let json = serde_json::to_string(&shown).unwrap();
        for secret in ["check-secret", "prompt_template", "launch_policy", "resource_id"] {
            assert!(!json.contains(secret), "{json}");
        }
        assert_eq!(store.agent_assignment_info("bound-check").unwrap(), before);
    } else {
        assert_eq!(
            accepted["result"]["check"]["latest_assessment"]["assignment_id"], "bound-check",
            "{accepted}"
        );
    }
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
    assert_eq!(
        store.show_record(task.id).unwrap().status,
        forge_app::RecordStatus::InProgress
    );
}
