// Tests at the production application/provider seam. All resources are isolated fixtures.
fn registered_policy(fixture: &Fixture, kind: RecordType) -> (forge_app::AgentProfile, PathBuf) {
    register_behavior(fixture, kind, "FROZEN-BEHAVIOR-SENTINEL")
}

fn register_behavior(
    fixture: &Fixture,
    kind: RecordType,
    behavior: &str,
) -> (forge_app::AgentProfile, PathBuf) {
    let skill = fixture.root.join("one/.agents/skills/entry");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: entry\ndescription: entry\n---\nOn demand",
    )
    .unwrap();
    std::fs::create_dir_all(fixture.root.join("one/.agents/profiles")).unwrap();
    std::fs::write(fixture.root.join("one/.agents/profiles/entry.md"), format!("---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\ntools: []\nskills: [{{name: entry}}]\n---\n{behavior}")).unwrap();
    let profile = fixture
        .app()
        .register_agent_profile(
            "one",
            &fixture.root.join("two"),
            Path::new(".agents/profiles/entry.md"),
            kind,
        )
        .unwrap();
    fixture
        .store()
        .select_workspace_profile("one", kind, Some(profile.revision))
        .unwrap();
    (profile, skill)
}

#[tokio::test]
async fn selected_mode_rpc_uses_configured_profile_validation_without_launching() {
    use forge_app::ExecutionMode::{Dispatched, Manual};
    let fixture = Fixture::new();
    let (profile, _) = registered_policy(&fixture, RecordType::Task);
    let store = fixture.store();
    store.replace_check_template("one/api", &forge_app::check_templates::TemplateReplacement {
        expected_version: 0, actor: "operator".into(), message: "Pin profile".into(),
        criteria: vec![forge_app::check_templates::CheckCriterion { name: "Implementation".into(), description: "Deliver work".into(), profile_revision: Some(profile.revision) }],
    }).unwrap();
    let task = store.open_typed_record(&NewTypedRecord { record_type: RecordType::Task, body: "Manual and dispatched policy".into(), project_slugs: vec!["one/api".into()], generated_by_ids: vec![] }).unwrap();
    let daemon = Daemon::new(fixture.app());
    let request = |workspace: &str, expected, mode| forge_rpc::SelectedRecordRequest {
        selection: forge_rpc::RecordSelection { workspace: Some(workspace.into()), cwd: None },
        record: forge_rpc::SetExecutionModeRequest { id: task.id, expected, mode, actor: "human".into(), message: "Explicit mode selection".into() },
    };
    assert!(RecordApiServer::execution_mode_set(&daemon, request("two", Dispatched, Manual)).await.is_err());
    assert_eq!(store.show_record(task.id).unwrap().execution_mode, Dispatched);
    RecordApiServer::execution_mode_set(&daemon, request("one", Dispatched, Manual)).await.unwrap();
    let changed = RecordApiServer::execution_mode_set(&daemon, request("one", Manual, Dispatched)).await.unwrap();
    assert_eq!(changed.execution_mode, Dispatched);
    assert_eq!(daemon.application.dispatch_record(task.id).unwrap_err().code, "workspace-dispatch-blocked");
    assert!(store.list_agent_assignments(task.id).unwrap().is_empty());
    assert!(store.worktree_claim(task.id).unwrap().is_none());
}

fn launch_count(provider: &Provider) -> usize {
    provider
        .state
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|call| call["method"] == "agent.start")
        .count()
}

fn claim_attempt(fixture: &Fixture, kind: RecordType) -> forge_app::AgentAssignment {
    let store = fixture.store();
    let record = store
        .open_typed_record(&NewTypedRecord {
            record_type: kind,
            body: "Restart regression".into(),
            project_slugs: vec!["one/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    if kind == RecordType::Task {
        let claim = store.reserve_worktree_claim(record.id).unwrap();
        std::fs::create_dir_all(claim.path).unwrap();
        store.activate_worktree_claim(record.id).unwrap();
    }
    store
        .claim_record_agent_assignment(record.id, "recovery-attempt")
        .unwrap()
        .0
}

// Seed already-launched historical ownership through the controlled provider fixture,
// not through the now-disabled production Task dispatch entry point.
fn seed_existing_launch(fixture: &Fixture, assignment: &forge_app::AgentAssignment) {
    let store = fixture.store();
    let workspace = if assignment.profile.record_type == RecordType::Task {
        forge_app::ManagedWorktreeRepository::worktree_claim(&store, assignment.record_id)
            .unwrap().unwrap().path
    } else {
        fixture.root.join("one")
    };
    let adapter = super::super::ServerAgentDispatchAdapterFactory {
        herdr: super::super::HerdrAdapterFactory::from_configuration_store(fixture.database.clone()),
        daemon_url: "http://127.0.0.1:45003/rpc".into(),
        capability_secret_root: fixture.root.join("assignment-capabilities"),
        cli_override: Some(std::env::current_exe().unwrap()),
    };
    let adapter = forge_app::AgentDispatchAdapterFactory::adapter_for(&adapter, &assignment.profile).unwrap();
    let _ = adapter.provision(&forge_app::AgentDispatchContext {
        assignment: assignment.clone(),
        project_root: fixture.root.join("one"),
        agent_workspace: Some(workspace),
        dispatch_workspace: store.assignment_dispatch_workspace(&assignment.id).unwrap(),
        rendered_initial_prompt: "Historical launch fixture".into(),
        bootstrap_prompt: None,
        capability_delivery: None,
        resources: store.list_agent_assignment_resources(&assignment.id).unwrap(),
    }, &store);
}

#[test]
fn launched_registered_assignment_recovers_and_retires_without_sources() {
    for kind in [RecordType::Task, RecordType::Grilling] {
        verify_source_independent_retirement(kind);
    }
}

fn verify_source_independent_retirement(kind: RecordType) {
    let fixture = Fixture::new();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    let app = fixture.app();
    fixture.configure(&app, &provider, "one");
    let (profile, source) = registered_policy(&fixture, kind);
    let original = claim_attempt(&fixture, kind);
    seed_existing_launch(&fixture, &original);
    let record = fixture.store().show_record(original.record_id).unwrap();
    let secrets = fixture.root.join("assignment-capabilities");
    let secret = std::fs::read_to_string(secrets.join(&original.id)).unwrap();
    let first = app
        .assignment(&secrets, &original.id, secret.trim())
        .unwrap()
        .bootstrap()
        .unwrap();
    assert_eq!(first.context.bound.profile_revision, profile.revision);
    let retained = fixture
        .store()
        .list_agent_assignment_resources(&original.id)
        .unwrap();
    // A new selected revision cannot redirect the old context, provider or behavior.
    let (replacement, _) = register_behavior(&fixture, kind, "REPLACEMENT-BEHAVIOR-SENTINEL");
    assert_ne!(replacement.revision, profile.revision);
    std::fs::remove_dir_all(source).unwrap();
    assert!(
        profile
            .launch_policy
            .as_ref()
            .unwrap()
            .validate_direct_inputs()
            .is_err()
    );
    drop(app);

    let app = fixture.app();
    let session = app
        .assignment(&secrets, &original.id, secret.trim())
        .unwrap();
    assert_eq!(
        session.bootstrap().unwrap().context.bound,
        first.context.bound
    );
    assert_eq!(
        session.bootstrap().unwrap().behavior,
        "FROZEN-BEHAVIOR-SENTINEL"
    );
    session
        .comment("Post-restart source-independent action")
        .unwrap();
    assert_eq!(session.context().unwrap().bound, first.context.bound);
    app.reconcile_agent_dispatch("recovery", 40_000).unwrap();
    assert_eq!(
        fixture
            .store()
            .list_agent_assignment_resources(&original.id)
            .unwrap(),
        retained
    );
    if kind == RecordType::Task {
        assert_eq!(app.dispatch_record(record.id).unwrap_err().code,
            "assignment-recovery-blocked");
    } else {
        assert_eq!(app.dispatch_record(record.id).unwrap().0.id, original.id);
    }
    assert_eq!(launch_count(&provider), 1);

    app.retire_agent_assignment(&original.id).unwrap();
    assert!(session.context().is_err());
    let info = app.agent_assignment_info(&original.id).unwrap();
    assert!(info.assignment.closed_at_millis.is_some());
    assert_eq!(info.assignment.profile, original.profile);
    assert!(info.dispatch_hold.is_some());
    assert!(
        !info.capacity_reserved(),
        "completed provisioning and verified cleanup leave no unknown allocation"
    );
    assert!(info.resources.iter().all(|r| r.retired_at_millis.is_some()));
    drop(app);
    let reopened = fixture.app().agent_assignment_info(&original.id).unwrap();
    assert_eq!(reopened.assignment.profile, original.profile);
    assert!(reopened.dispatch_hold.is_some());
    assert!(reopened.unknown_resource_resolution.is_none());
    let diagnostics = format!("{reopened:?}");
    for forbidden in [
        secret.trim(),
        "FROZEN-BEHAVIOR-SENTINEL",
        "REPLACEMENT-BEHAVIOR-SENTINEL",
        secrets.to_str().unwrap(),
    ] {
        assert!(!diagnostics.contains(forbidden));
    }
    assert_eq!(launch_count(&provider), 1);
}

#[test]
fn persisted_crash_windows_never_replay_provisioning_or_launch() {
    for kind in [RecordType::Task, RecordType::Grilling] {
        for window in [
            "claimed",
            "provision-intent",
            "allocated",
            "launch-intent",
            "response-lost",
            "ack-response-lost",
            "response",
            "ack-response",
        ] {
            verify_crash_window(kind, window);
        }
    }
}

fn verify_crash_window(kind: RecordType, window: &str) {
    let fixture = Fixture::new();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    let app = fixture.app();
    fixture.configure(&app, &provider, "one");
    let (profile, _) = registered_policy(&fixture, kind);
    let original = claim_attempt(&fixture, kind);
    let store = fixture.store();
    match window {
        "claimed" => (),
        "provision-intent" | "allocated" | "launch-intent" => {
            assert!(store.begin_agent_provision(&original.id).unwrap());
            if window != "provision-intent" {
                for (resource_kind, resource_id) in [("tab", "tab"), ("pane", "pane")] {
                    store
                        .add_agent_assignment_resource(&forge_app::NewAgentAssignmentResource {
                            assignment_id: original.id.clone(),
                            resource_kind: resource_kind.into(),
                            resource_id: resource_id.into(),
                        })
                        .unwrap();
                }
            }
            if window == "launch-intent" {
                assert!(store.begin_agent_launch(&original.id, 1_000).unwrap());
            }
        }
        _ => {
            provider.state.lock().unwrap().lose_launch_response = window.contains("lost");
            seed_existing_launch(&fixture, &original);
            assert_eq!(launch_count(&provider), 1, "{kind:?}/{window}");
            if window.starts_with("ack") {
                let root = fixture.root.join("assignment-capabilities");
                let secret = std::fs::read_to_string(root.join(&original.id)).unwrap();
                app.assignment(&root, &original.id, secret.trim())
                    .unwrap()
                    .bootstrap()
                    .unwrap();
            }
        }
    }
    let _ = register_behavior(&fixture, kind, "REPLACEMENT-BEHAVIOR-SENTINEL");
    let before = store.agent_assignment_info(&original.id).unwrap();
    let resources = before.resources.clone();
    drop(store);
    drop(app);
    let app = fixture.app();
    for (daemon, now) in [("restart-one", 40_000), ("restart-two", 80_000)] {
        app.reconcile_agent_dispatch(daemon, now).unwrap();
        let recovered = app.agent_assignment_info(&original.id).unwrap();
        assert_eq!(
            recovered.assignment.profile.revision, profile.revision,
            "{window}"
        );
        assert_eq!(recovered.assignment.profile, original.profile);
        assert_eq!(recovered.assignment.record_id, original.record_id);
        assert!(recovered.capacity_reserved());
        if window != "claimed" {
            assert_eq!(recovered.resources, resources, "{window}");
        }
    }
    let expected = usize::from(
        matches!(window, "response-lost" | "ack-response-lost" | "response" | "ack-response")
            || (window == "claimed" && kind == RecordType::Grilling)
    );
    if kind == RecordType::Task && !window.starts_with("ack") {
        assert_eq!(app.agent_assignment_info(&original.id).unwrap(), before);
    }
    assert_eq!(launch_count(&provider), expected, "{kind:?}/{window}");
    assert_eq!(
        provider
            .state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|call| call["method"] == "tab.create")
            .count(),
        expected,
        "provisioning must not replay: {kind:?}/{window}"
    );
    assert_eq!(
        fixture
            .store()
            .list_agent_assignments(original.record_id)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fixture
            .store()
            .assignment_dispatch_workspace(&original.id)
            .unwrap()
            .unwrap()
            .id,
        "herdr-one"
    );
    if window != "claimed" {
        assert!(!fixture.store().begin_agent_provision(&original.id).unwrap());
    }
}

#[test]
#[allow(clippy::too_many_lines)] // One ordered recovery scenario across durable reopen boundaries.
fn late_bootstrap_absence_repair_and_resume_do_not_authorize_replacement() {
    let fixture = Fixture::new();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    let app = fixture.app();
    fixture.configure(&app, &provider, "one");
    let (_, source) = registered_policy(&fixture, RecordType::Task);
    let original = claim_attempt(&fixture, RecordType::Task);
    provider.state.lock().unwrap().lose_launch_response = true;
    seed_existing_launch(&fixture, &original);
    let root = fixture.root.join("assignment-capabilities");
    let secret = std::fs::read_to_string(root.join(&original.id)).unwrap();
    std::fs::remove_dir_all(source).unwrap();
    fixture
        .store()
        .hold_dispatch(original.record_id, "Independent operator hold")
        .unwrap();
    let deadline = fixture
        .store()
        .agent_launch_intent(&original.id)
        .unwrap()
        .unwrap()
        .acknowledgement_deadline_millis;
    drop(app);

    let app = fixture.app();
    app.reconcile_agent_dispatch("expired", deadline + 1)
        .unwrap();
    let session = app.assignment(&root, &original.id, secret.trim()).unwrap();
    session.bootstrap().unwrap();
    assert_eq!(
        fixture
            .store()
            .dispatch_hold(original.record_id)
            .unwrap()
            .unwrap()
            .reason,
        "Independent operator hold"
    );
    assert_eq!(
        fixture
            .store()
            .agent_assignment(&original.id)
            .unwrap()
            .state,
        forge_app::AgentAssignmentState::Working
    );
    // No retained agent identity: missing resources and late acknowledgement cannot
    // turn a lost response into trustworthy process-exit evidence.
    {
        let mut state = provider.state.lock().unwrap();
        state.agents.clear();
        state.tab_closed = true;
    }
    app.reconcile_agent_dispatch("late", deadline + 40_000)
        .unwrap();
    assert!(
        app.agent_assignment_info(&original.id)
            .unwrap()
            .capacity_reserved()
    );
    assert!(app.reopen_task(original.record_id).is_err());
    assert_eq!(launch_count(&provider), 1);
    app.retire_agent_assignment(&original.id).unwrap();
    assert!(session.context().is_err());
    assert!(
        session.acknowledge().is_err(),
        "late credentials cannot resurrect a closed attempt"
    );
    app.reopen_task(original.record_id).unwrap();
    let (replacement, _) = registered_policy(&fixture, RecordType::Task);
    app.resume_task_dispatch(original.record_id).unwrap();
    assert!(
        fixture
            .store()
            .dispatch_hold(original.record_id)
            .unwrap()
            .is_none()
    );
    let info = app.agent_assignment_info(&original.id).unwrap();
    assert!(info.capacity_reserved());
    assert!(
        fixture
            .store()
            .claim_record_agent_assignment(original.record_id, "unsafe-replacement")
            .is_err()
    );
    assert!(app.dispatch_record(original.record_id).is_err());
    assert_eq!(launch_count(&provider), 1);
    drop(app);

    let app = fixture.app();
    assert!(
        app.agent_assignment_info(&original.id)
            .unwrap()
            .capacity_reserved()
    );
    app.resolve_unknown_agent_resources(&original.id, true, true)
        .unwrap();
    let new = fixture
        .store()
        .claim_record_agent_assignment(original.record_id, "safe-replacement")
        .unwrap()
        .0;
    assert_ne!(new.id, original.id);
    assert_eq!(new.profile.revision, replacement.revision);
    assert_eq!(
        fixture
            .store()
            .agent_assignment(&original.id)
            .unwrap()
            .profile,
        original.profile
    );
    assert_eq!(
        launch_count(&provider),
        1,
        "claiming a safely eligible replacement is not launch replay"
    );
}
