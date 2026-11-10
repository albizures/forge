fn context_test_application(database: &Path, secrets: &Path) -> Application {
    Application::with_agent_dispatch(
        Box::new(SqliteProjectRepository::open(database).unwrap()),
        Box::new(SqliteProjectRepository::open(database).unwrap()),
        Box::new(super::ServerAgentDispatchAdapterFactory {
            herdr: super::HerdrAdapterFactory::from_configuration_store(database.to_owned()),
            daemon_url: "http://127.0.0.1:1".into(),
            capability_secret_root: secrets.to_owned(),
            cli_override: None,
        }),
        Box::new(TestInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(database).unwrap()))
}

fn context_policy_fixture(catalog: &Path) -> forge_app::agent_profiles::ResolvedLaunchPolicy {
    std::fs::create_dir_all(catalog.join(".agents/skills/entry")).unwrap();
    std::fs::create_dir_all(catalog.join(".pi")).unwrap();
    std::fs::write(
        catalog.join(".agents/skills/entry/SKILL.md"),
        "---\nname: entry\ndescription: entry\n---\nOn demand",
    )
    .unwrap();
    std::fs::write(catalog.join(".pi/mcp.json"), r#"{"mcpServers":{"local":{"command":"never-connect","env":{"PASSWORD":"MCP-CREDENTIAL-SENTINEL"}}}}"#).unwrap();
    std::fs::write(catalog.join(".agents/skills/entry/forge-agent.md"), "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\ntools: []\nskills: [{name: entry}]\nmcpServers: [{name: local}]\n---\nBEHAVIOR-SENTINEL").unwrap();
    forge_app::agent_profiles::ResourceCatalog::discover(catalog, catalog)
        .unwrap()
        .resolve(catalog, ".agents/skills/entry/forge-agent.md")
        .unwrap()
}

fn claim_context_fixture(
    store: &SqliteProjectRepository,
    kind: RecordType,
    revision: u32,
) -> (forge_app::Record, Option<PathBuf>) {
    use forge_app::{ManagedWorktreeRepository, WorkspaceDispatchRepository};
    let own = store
        .open_typed_record(&NewTypedRecord {
            record_type: kind,
            task_kind: None,
            body: "Assigned title\nAssigned body".into(),
            project_slugs: if kind == RecordType::Task {
                vec!["alpha".into()]
            } else {
                vec!["alpha".into(), "beta".into()]
            },
            generated_by_ids: vec![],
        })
        .unwrap();
    let worktree = if kind == RecordType::Task {
        let claim = store.reserve_worktree_claim(own.id).unwrap();
        store.activate_worktree_claim(own.id).unwrap();
        Some(claim.path)
    } else {
        None
    };
    // Durable launch fixture only: verification evidence, never runtime/MCP calls.
    store
        .set_project_task_concurrency_limit("alpha", Some(10))
        .unwrap();
    for selected_kind in [RecordType::Task, RecordType::Grilling] {
        store
            .select_workspace_profile(
                "test",
                selected_kind,
                Some(if kind == selected_kind { revision } else { 1 }),
            )
            .unwrap();
    }
    let generation = store
        .workspace_dispatch_configuration("test")
        .unwrap()
        .provider_generation;
    store
        .retain_verified_workspace_binding(
            "test",
            Some(&forge_app::DispatchWorkspace {
                id: "fixture-provider-workspace".into(),
            }),
            generation,
        )
        .unwrap();
    store
        .claim_record_agent_assignment(own.id, "context-owner")
        .unwrap();
    store
        .hold_dispatch(own.id, "Existing operator hold")
        .unwrap();
    (own, worktree)
}

fn context_credentials() -> serde_json::Value {
    serde_json::json!({"assignmentId":"context-owner", "secret":"actual-secret"})
}

fn assert_context_redacted(text: &str, behavior_allowed: bool) {
    for forbidden in [
        "actual-secret",
        "MCP-CREDENTIAL-SENTINEL",
        "PRIVATE-DELIVERY-CATALOG",
        "origin_url",
    ] {
        assert!(!text.contains(forbidden), "must not expose {forbidden}");
    }
    if !behavior_allowed {
        assert!(!text.contains("BEHAVIOR-SENTINEL"));
    }
}

async fn verify_source_independent_context_routes(
    daemon: &Daemon,
    own_id: u64,
    bound: &forge_app::assignment::BoundIdentity,
) {
    let context = assignment_rpc(daemon.clone(), "assignment.context", context_credentials()).await;
    assert_eq!(
        context["result"]["bound"],
        serde_json::to_value(bound).unwrap(),
        "{context}"
    );
    assert_eq!(context["result"]["record"]["title"], "Assigned title");
    assert_context_redacted(&context.to_string(), false);
    let bootstrap = assignment_rpc(
        daemon.clone(),
        "assignment.bootstrap",
        context_credentials(),
    )
    .await;
    assert_eq!(
        bootstrap["result"]["behavior"], "BEHAVIOR-SENTINEL",
        "{bootstrap}"
    );
    assert_context_redacted(&bootstrap.to_string(), true);
    let started = assignment_rpc(daemon.clone(), "assignment.start", context_credentials()).await;
    assert_eq!(started["result"]["record"]["status"], "in_progress");
    let mut request = context_credentials();
    request["body"] = "Source-independent comment".into();
    let comment = assignment_rpc(daemon.clone(), "assignment.comment", request).await;
    assert_eq!(
        comment["result"]["comment"]["body"],
        "Source-independent comment"
    );
    for method in ["assignment.comments", "assignment.history"] {
        let page = assignment_rpc(daemon.clone(), method, serde_json::json!({"assignmentId":"context-owner", "secret":"actual-secret", "target":own_id,"limit":1})).await;
        assert_eq!(
            page["result"]["items"].as_array().unwrap().len(),
            1,
            "{page}"
        );
        assert_eq!(page["result"]["schemaVersion"], 1);
    }
}

async fn verify_context_route_authentication(daemon: &Daemon, own_id: u64) {
    for method in [
        "assignment.bootstrap",
        "assignment.context",
        "assignment.start",
        "assignment.done",
        "assignment.comment",
        "assignment.record",
        "assignment.comments",
        "assignment.history",
    ] {
        let mut request = context_credentials();
        request["secret"] = "wrong".into();
        match method {
            "assignment.record" => request["target"] = own_id.into(),
            "assignment.comments" | "assignment.history" => {
                request["target"] = own_id.into();
                request["limit"] = 10.into();
            }
            "assignment.comment" => request["body"] = "Forbidden".into(),
            _ => (),
        }
        let result = assignment_rpc(daemon.clone(), method, request).await;
        assert!(result.get("error").is_some(), "{method}: {result}");
        assert_context_redacted(&result.to_string(), false);
    }
}

async fn verify_source_independent_subscription_delivery(daemon: &Daemon, own_id: u64) {
    let created = assignment_rpc(
        daemon.clone(),
        "agentSubscription.create",
        serde_json::json!({"assignment_id":"context-owner", "capability_id":"actual-secret", "correlation_id":"not-authority", "target_record_ids":[own_id], "kind_filters":[]}),
    ).await;
    let id = created["result"]["id"].as_u64().unwrap_or_else(|| panic!("{created}"));
    let manage = serde_json::json!({"assignment_id":"context-owner", "capability_id":"actual-secret", "correlation_id":"not-authority", "subscription_id":id});
    let renewed = assignment_rpc(daemon.clone(), "agentSubscription.renew", manage.clone()).await;
    assert!(renewed.get("result").is_some(), "{renewed}");
    let endpoint = format!("/v1/agent-assignments/context-owner/record-update-subscriptions/{id}/events");
    let request = axum::http::Request::builder()
        .uri(endpoint)
        .header("authorization", "Bearer actual-secret")
        .extension(axum::extract::ConnectInfo("127.0.0.1:40000".parse::<std::net::SocketAddr>().unwrap()))
        .body(axum::body::Body::empty()).unwrap();
    let response = router(daemon.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    stream.next().await.unwrap().unwrap(); // connection control frame
    let comment = assignment_rpc(daemon.clone(), "assignment.comment", serde_json::json!({"assignmentId":"context-owner", "secret":"actual-secret", "body":"Notification after source deletion"})).await;
    assert!(comment.get("result").is_some(), "{comment}");
    let notification = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
        .await.unwrap().unwrap().unwrap();
    let text = String::from_utf8(notification.to_vec()).unwrap();
    assert!(text.contains("record-update"), "{text}");
    assert_context_redacted(&text, false);
    let context = assignment_rpc(daemon.clone(), "assignment.context", context_credentials()).await;
    assert_eq!(context["result"]["record"]["id"], own_id);
    let deleted = assignment_rpc(daemon.clone(), "agentSubscription.delete", manage).await;
    assert!(deleted.get("result").is_some(), "{deleted}");
    assert!(tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await.unwrap().is_none());
}

async fn verify_context_completion(daemon: &Daemon, kind: RecordType) {
    let mut done = context_credentials();
    done["resolution"] = "implemented".into();
    if kind == RecordType::Grilling {
        assert!(
            assignment_rpc(daemon.clone(), "assignment.done", done.clone())
                .await
                .get("error")
                .is_some()
        );
        done["human"] = serde_json::json!({"confirmed":false, "confirmation":"Not confirmed", "agreedOutcome":"Use flags"});
        assert!(
            assignment_rpc(daemon.clone(), "assignment.done", done.clone())
                .await
                .get("error")
                .is_some()
        );
        done["human"] = serde_json::json!({"confirmed":true, "confirmation":"Human explicitly confirmed", "agreedOutcome":"Use flags"});
    }
    let completed = assignment_rpc(daemon.clone(), "assignment.done", done).await;
    assert_eq!(
        completed["result"]["record"]["status"], "done",
        "{completed}"
    );
}

#[tokio::test]
async fn persisted_task_and_multi_project_grilling_context_survives_source_deletion_and_restart() {
    use forge_app::{ManagedWorktreeRepository, WorkspaceDispatchRepository};
    for kind in [RecordType::Task, RecordType::Grilling] {
        let (fixture, _, _, _, _) = subscription_test_daemon();
        let root = fixture.database_path.with_extension("context-secrets");
        let catalog = root.join("PRIVATE-DELIVERY-CATALOG");
        let policy = context_policy_fixture(&catalog);
        let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
        for id in ["owner-assignment", "target-assignment"] {
            store
                .close_agent_assignment(id, forge_app::AgentAssignmentClosureReason::Retired)
                .unwrap();
        }
        super::test_projects::register(&store, "beta", None);
        let profile = store.register_agent_profile("test", kind, &policy).unwrap();
        let (own, worktree) = claim_context_fixture(&store, kind, profile.revision);
        std::fs::write(root.join("context-owner"), "actual-secret\n").unwrap();
        let app = context_test_application(&fixture.database_path, &root);
        let session = app
            .assignment(&root, "context-owner", "actual-secret")
            .unwrap();
        let first = session.bootstrap().unwrap();
        assert_eq!(first.behavior, "BEHAVIOR-SENTINEL");
        assert_eq!(
            first.context.projects.len(),
            if kind == RecordType::Task { 1 } else { 2 }
        );
        assert_eq!(
            first.context.bound.execution_directory,
            worktree
                .clone()
                .unwrap_or_else(|| first.context.workspace.root.clone())
        );
        assert_ne!(
            first.context.bound.execution_directory,
            first.context.projects[0].project_root
        );
        let bound = first.context.bound.clone();
        let mut replacement_policy = policy.clone();
        replacement_policy.behavior = "Replacement behavior".into();
        let replacement = store
            .register_agent_profile("test", kind, &replacement_policy)
            .unwrap();
        store
            .select_workspace_profile("test", kind, Some(replacement.revision))
            .unwrap();
        std::fs::remove_dir_all(&catalog).unwrap();
        assert!(policy.validate_direct_inputs().is_err());
        if worktree.is_some() {
            store.require_worktree_claim_cleanup(own.id).unwrap();
        }
        drop(app);
        let mut daemon = Daemon::new(context_test_application(&fixture.database_path, &root));
        daemon.capability_secret_root = Some(root.clone());
        verify_source_independent_context_routes(&daemon, own.id, &bound).await;
        verify_context_route_authentication(&daemon, own.id).await;
        verify_source_independent_subscription_delivery(&daemon, own.id).await;
        verify_context_completion(&daemon, kind).await;
        assert!(
            store.dispatch_hold(own.id).unwrap().is_some(),
            "bootstrap/actions cannot clear holds"
        );
        assert_context_redacted(
            &format!(
                "{:?}",
                store
                    .list_assignment_authorization_audits("context-owner")
                    .unwrap()
            ),
            false,
        );
        store
            .close_agent_assignment(
                "context-owner",
                forge_app::AgentAssignmentClosureReason::Retired,
            )
            .unwrap();
        assert!(
            assignment_rpc(daemon, "assignment.context", context_credentials())
                .await
                .get("error")
                .is_some()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
