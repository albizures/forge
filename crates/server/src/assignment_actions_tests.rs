async fn assignment_rpc(
    daemon: Daemon,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    let request = axum::http::Request::builder().method("POST").uri("/rpc").header("content-type", "application/json").body(axum::body::Body::from(serde_json::json!({"jsonrpc":"2.0", "id":1, "method":method.replace('.', "_"), "params":[params]}).to_string())).unwrap();
    let response = router(daemon).oneshot(request).await.unwrap();
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn production_assignment_routes_authenticate_and_reject_spoofed_fields() {
    let (fixture, assignment, neighbor) = assignment_test_daemon();
    let root = fixture.database_path.with_extension("rpc-secrets");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(&assignment), "actual-secret\n").unwrap();
    let mut daemon = fixture.daemon();
    daemon.capability_secret_root = Some(root.clone());
    let request = |secret: &str| serde_json::json!({"assignmentId": assignment, "secret":secret, "target":neighbor});
    let good = assignment_rpc(
        daemon.clone(),
        "assignment.record",
        request("actual-secret"),
    )
    .await;
    assert_eq!(good["result"]["record"]["id"], neighbor, "{good}");
    for secret in ["wrong", &assignment, ""] {
        let denied = assignment_rpc(daemon.clone(), "assignment.record", request(secret)).await;
        assert!(denied.get("error").is_some(), "{denied}");
        assert!(!denied.to_string().contains("actual-secret"));
    }
    let mut spoof = request("actual-secret");
    spoof["assignmentId"] = "target-assignment".into();
    assert!(
        assignment_rpc(daemon.clone(), "assignment.record", spoof)
            .await
            .get("error")
            .is_some()
    );
    let own_spoof = serde_json::json!({"assignmentId": assignment, "secret":"actual-secret", "target":neighbor, "body":"Forbidden foreign comment"});
    assert!(
        assignment_rpc(daemon.clone(), "assignment.comment", own_spoof)
            .await
            .get("error")
            .is_some()
    );
    let good = assignment_rpc(daemon.clone(), "assignment.comment", serde_json::json!({"assignmentId":assignment,"secret":"actual-secret","body":"Own RPC comment"})).await;
    assert_eq!(
        good["result"]["comment"]["body"], "Own RPC comment",
        "{good}"
    );
    let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
    store.start_record(neighbor).unwrap();
    check_tests::pass_checks(&store, neighbor);
    store.done_record(neighbor, Some("satisfied")).unwrap();
    let owner = store.agent_assignment(&assignment).unwrap().record_id;
    let legacy_own = assignment_rpc(daemon.clone(), "agentDispatch.taskStart", serde_json::json!({"assignment_id":assignment,"capability_id":assignment,"correlation_id":"actual-secret","task_id":owner})).await;
    assert!(
        legacy_own.get("error").is_some(),
        "an Assignment ID is not a credential: {legacy_own}"
    );
    let legacy = assignment_rpc(daemon, "agentDispatch.taskStart", serde_json::json!({"assignment_id":assignment,"capability_id":assignment,"correlation_id":"actual-secret","task_id":neighbor})).await;
    assert!(legacy.get("error").is_some());
    std::fs::remove_dir_all(root).unwrap();
}

fn verify_assignment_graph(
    session: &forge_app::assignment::AuthenticatedAssignment<'_>,
    readable: Vec<u64>,
    denied: Vec<u64>,
) {
    for target in readable {
        assert_eq!(session.record(target).unwrap().id, target);
        assert!(session.comments(target, None, 10).is_ok());
        assert!(session.history(target, None, 10).is_ok());
    }
    for target in denied {
        assert_eq!(
            session.record(target).unwrap_err().code,
            "assignment-capability-denied",
            "target {target}"
        );
        assert!(session.comments(target, None, 10).is_err());
        assert!(session.history(target, None, 10).is_err());
    }
}

#[test]
fn assignment_read_graph_is_one_hop_for_tasks_and_grillings() {
    for kind in [RecordType::Task, RecordType::Grilling] {
        let (fixture, _, _) = assignment_test_daemon();
        let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
        let open = |kind, body: &str| {
            store
                .open_typed_record(&NewTypedRecord {
                    record_type: kind,
                    task_kind: None,
                    body: body.into(),
                    project_slugs: vec!["alpha".into()],
                    generated_by_ids: vec![],
                })
                .unwrap()
        };
        let parent = open(RecordType::Wayfinder, "Parent");
        let own = if kind == RecordType::Task {
            store
                .open_task_under_wayfinder("Own", parent.id, None, None, vec![])
                .unwrap()
        } else {
            store
                .open_grilling_under_wayfinder("Own", parent.id, None, vec![])
                .unwrap()
        };
        let sibling = store
            .open_task_under_wayfinder("Sibling", parent.id, None, None, vec![])
            .unwrap();
        let prerequisite = open(RecordType::Task, "Prerequisite");
        let dependent = open(RecordType::Task, "Dependent");
        let generator = open(RecordType::Task, "Generator");
        let generated = open(RecordType::Task, "Generated");
        let transitive = open(RecordType::Task, "Two hops");
        let shared_project = open(RecordType::Task, "Only shared Project");
        let initiative = open(RecordType::Initiative, "Initiative");
        store
            .relate_records(parent.id, initiative.id, RecordRelationshipKind::Initiative)
            .unwrap();
        for (from, to, edge) in [
            (own.id, prerequisite.id, RecordRelationshipKind::DependsOn),
            (dependent.id, own.id, RecordRelationshipKind::DependsOn),
            (own.id, generator.id, RecordRelationshipKind::GeneratedBy),
            (generated.id, own.id, RecordRelationshipKind::GeneratedBy),
            (
                prerequisite.id,
                transitive.id,
                RecordRelationshipKind::DependsOn,
            ),
        ] {
            if kind == RecordType::Grilling
                && from == own.id
                && edge == RecordRelationshipKind::DependsOn
            {
                continue;
            }
            store.relate_records(from, to, edge).unwrap();
        }
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "graph".into(),
                record_id: own.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        let root = fixture.database_path.with_extension("graph-secrets");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("graph"), "graph-secret").unwrap();
        let daemon = fixture.daemon();
        let session = daemon
            .application
            .assignment(&root, "graph", "graph-secret")
            .unwrap();
        let mut readable = vec![own.id, parent.id, dependent.id, generator.id, generated.id];
        if kind == RecordType::Task {
            readable.push(prerequisite.id);
        }
        verify_assignment_graph(
            &session,
            readable,
            vec![
                sibling.id,
                transitive.id,
                shared_project.id,
                initiative.id,
                u64::MAX,
            ],
        );
        store
            .unrelate_records(generated.id, own.id, RecordRelationshipKind::GeneratedBy)
            .unwrap();
        assert!(session.record(generated.id).is_err());
        std::fs::write(root.join("graph"), "revoked-and-replaced").unwrap();
        assert!(
            session.record(own.id).is_err(),
            "delivery revocation is checked on each operation"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn assignment_application_authenticates_real_secret_and_rechecks_graph() {
    let (fixture, assignment, neighbor) = assignment_test_daemon();
    let root = fixture.database_path.with_extension("actions-secrets");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(&assignment), "actual-secret\n").unwrap();
    let daemon = fixture.daemon();
    assert!(
        daemon
            .application
            .assignment(&root, &assignment, &assignment)
            .is_err()
    );
    assert!(
        daemon
            .application
            .assignment(&root, "../owner-assignment", "actual-secret")
            .is_err()
    );
    let session = daemon
        .application
        .assignment(&root, &assignment, "actual-secret")
        .unwrap();
    assert_eq!(session.record(neighbor).unwrap().id, neighbor);
    let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
    let owner = store.agent_assignment(&assignment).unwrap().record_id;
    store
        .unrelate_records(owner, neighbor, RecordRelationshipKind::DependsOn)
        .unwrap();
    assert!(session.record(neighbor).is_err());
    assert_eq!(session.record(owner).unwrap().id, owner);
    let spec = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Spec,
            task_kind: None,
            body: "Legacy planning record".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    store
        .create_agent_assignment(&NewAgentAssignment {
            id: "legacy-spec".into(),
            record_id: spec.id,
            project_slug: "alpha".into(),
            profile_revision: 1,
        })
        .unwrap();
    std::fs::write(root.join("legacy-spec"), "actual-secret").unwrap();
    assert!(
        daemon
            .application
            .assignment(&root, "legacy-spec", "actual-secret")
            .is_err(),
        "Task/Grilling authority cannot be synthesized from a legacy Spec/profile mismatch"
    );
    store
        .close_agent_assignment(
            &assignment,
            forge_app::AgentAssignmentClosureReason::Retired,
        )
        .unwrap();
    assert!(session.record(owner).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn authenticated_grilling_bootstrap_separates_bound_identity_and_live_context() {
    let (fixture, _, _) = assignment_test_daemon();
    let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
    let grilling = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Grilling,
            task_kind: None,
            body: "Decide".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    store
        .create_agent_assignment(&NewAgentAssignment {
            id: "grill".into(),
            record_id: grilling.id,
            project_slug: "alpha".into(),
            profile_revision: 1,
        })
        .unwrap();
    let root = fixture.database_path.with_extension("grilling-secrets");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("grill"), "actual-secret\n").unwrap();
    let app = Application::with_agent_dispatch(
        Box::new(SqliteProjectRepository::open(&fixture.database_path).unwrap()),
        Box::new(SqliteProjectRepository::open(&fixture.database_path).unwrap()),
        Box::new(super::ServerAgentDispatchAdapterFactory {
            herdr: super::HerdrAdapterFactory::from_configuration_store(
                fixture.database_path.clone(),
            ),
            daemon_url: "http://127.0.0.1:1".into(),
            capability_secret_root: root.clone(),
            cli_override: None,
        }),
        Box::new(TestInspector),
    )
    .with_workspace_repository(Box::new(
        SqliteProjectRepository::open(&fixture.database_path).unwrap(),
    ));
    let session = app.assignment(&root, "grill", "actual-secret").unwrap();
    assert!(session.behavior().is_err());
    let first = session.bootstrap().unwrap();
    assert_eq!(first.context.schema_version, 1);
    assert_eq!(
        first.context.record.status,
        forge_app::RecordStatus::InProgress
    );
    assert_eq!(first.context.projects.len(), 1);
    assert_eq!(
        first.context.bound.execution_directory,
        first.context.workspace.root
    );
    let retry = session.bootstrap().unwrap();
    assert_eq!(first.context.bound, retry.context.bound);
    store
        .edit_record(grilling.id, "Changed live decision")
        .unwrap();
    let live = session.context().unwrap();
    assert_eq!(live.bound, first.context.bound);
    assert_eq!(live.record.body, "Changed live decision");
    assert_ne!(live.freshness, first.context.freshness);
    assert!(session.done(Some("agreed"), None).is_err());
    let result = session
        .done(
            Some("agreed"),
            Some(&forge_app::assignment::HumanConfirmation {
                confirmed: true,
                confirmation: "Human explicitly said yes".into(),
                agreed_outcome: "Use flags only".into(),
            }),
        )
        .unwrap();
    assert!(
        result
            .completion_resolution
            .unwrap()
            .contains("Use flags only")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn authenticated_pages_are_complete_bounded_and_cursor_bound() {
    let (fixture, assignment, neighbor) = assignment_test_daemon();
    let root = fixture.database_path.with_extension("page-secrets");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(&assignment), "actual-secret\n").unwrap();
    let daemon = fixture.daemon();
    let session = daemon
        .application
        .assignment(&root, &assignment, "actual-secret")
        .unwrap();
    let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
    for n in 0..5 {
        store
            .add_record_comment(neighbor, &format!("comment {n}"), None)
            .unwrap();
    }
    let first = session.comments(neighbor, None, 2).unwrap();
    assert_eq!(first.items.len(), 2);
    let cursor = first.next_cursor.unwrap();
    assert!(session.comments(neighbor, Some("garbage"), 2).is_err());
    assert!(session.comments(neighbor, None, 0).is_err());
    assert!(session.comments(neighbor, None, 101).is_err());
    assert!(session.history(neighbor, Some(&cursor), 2).is_err());
    let tampered = format!("{}0", &cursor[..cursor.len() - 1]);
    let tampered = if tampered == cursor {
        format!("{}1", &cursor[..cursor.len() - 1])
    } else {
        tampered
    };
    assert!(session.comments(neighbor, Some(&tampered), 2).is_err());
    std::fs::write(root.join("target-assignment"), "actual-secret").unwrap();
    let other = daemon
        .application
        .assignment(&root, "target-assignment", "actual-secret")
        .unwrap();
    assert!(other.comments(neighbor, Some(&cursor), 2).is_err());
    let own = store.agent_assignment(&assignment).unwrap().record_id;
    assert!(session.comments(own, Some(&cursor), 2).is_err());
    store
        .add_record_comment(neighbor, "later, next traversal", None)
        .unwrap();
    let second = session.comments(neighbor, Some(&cursor), 2).unwrap();
    let last = session
        .comments(neighbor, second.next_cursor.as_deref(), 2)
        .unwrap();
    assert_eq!(last.items.len(), 1);
    assert!(last.next_cursor.is_none());
    assert_ne!(first.freshness, last.freshness);
    let mut history = Vec::new();
    let mut next = None;
    loop {
        let page = session.history(neighbor, next.as_deref(), 2).unwrap();
        assert!(page.items.len() <= 2);
        history.extend(page.items);
        next = page.next_cursor;
        if next.is_none() {
            break;
        }
    }
    let mut expected = store.list_record_updates(neighbor).unwrap();
    expected.reverse();
    assert_eq!(history, expected);
    store
        .unrelate_records(own, neighbor, RecordRelationshipKind::DependsOn)
        .unwrap();
    assert!(session.comments(neighbor, Some(&cursor), 2).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn authenticated_actions_require_acknowledgement_and_normal_lifecycle_validation() {
    let (fixture, assignment, prerequisite) = assignment_test_daemon();
    let root = fixture.database_path.with_extension("lifecycle-secrets");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(&assignment), "actual-secret\n").unwrap();
    let daemon = fixture.daemon();
    let session = daemon
        .application
        .assignment(&root, &assignment, "actual-secret")
        .unwrap();
    assert!(session.start().is_err());
    assert!(session.done(Some("too early"), None).is_err());
    session.acknowledge().unwrap();
    assert!(
        session.start().is_err(),
        "open prerequisite still blocks start"
    );
    let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
    store.start_record(prerequisite).unwrap();
    check_tests::pass_checks(&store, prerequisite);
    store.done_record(prerequisite, Some("satisfied")).unwrap();
    assert_eq!(
        session.start().unwrap().status,
        forge_app::RecordStatus::InProgress
    );
    assert_eq!(
        session.start().unwrap().status,
        forge_app::RecordStatus::InProgress
    );
    assert!(session.comment("  ").is_err());
    assert_eq!(session.comment("Own comment").unwrap().body, "Own comment");
    let owner = store.agent_assignment(&assignment).unwrap().record_id;
    store
        .hold_dispatch(owner, "Operator review required")
        .unwrap();
    assert!(
        session.bootstrap().is_err(),
        "missing trusted scope must fail bootstrap after start"
    );
    assert!(store.dispatch_hold(owner).unwrap().is_some());
    assert!(!store.assignment_context_read(&assignment).unwrap());
    assert!(
        session.behavior().is_err(),
        "start alone cannot release behavior before context"
    );
    assert_eq!(
        session.done(Some("  "), None).unwrap_err().code,
        "invalid-resolution"
    );
    assert_eq!(session.done(Some("implemented"), None).unwrap_err().code, "checks-not-passed");
    check_tests::pass_checks(&store, owner);
    let finding = store.raise_task_finding(owner, &forge_app::findings::FindingRaiseInput {
        severity: forge_app::findings::Severity::Critical, blocking: None,
        check_id: None, assessment_id: None, actor: "reviewer".into(),
        message: "Missing validation".into(), revision: None, dirty: None,
    }).unwrap();
    let blocked = session.done(Some("implemented"), None).unwrap_err();
    assert_eq!(blocked.code, "completion-blocked");
    assert!(blocked.message.contains(&format!("Finding IDs: [{}]", finding.id)));
    store.resolve_task_finding(owner, finding.id, &forge_app::findings::FindingResolveInput {
        expected_version: 1, actor: "verifier".into(), message: "Verified correction".into(),
        revision: None, dirty: None,
    }).unwrap();
    session.done(Some("implemented"), None).unwrap();
    assert!(session.start().is_err());
    std::fs::remove_dir_all(root).unwrap();
}
