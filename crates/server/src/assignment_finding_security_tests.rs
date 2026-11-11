#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn assignment_finding_rpc_confines_reads_origins_and_all_mutations() {
    use forge_app::{
        WorkspaceRepository,
        findings::{FindingRaiseInput, Severity},
    };
    let (fixture, assignment, _) = assignment_test_daemon_scoped(true);
    let daemon = fixture.daemon();
    let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
    let owner = store.agent_assignment(&assignment).unwrap().record_id;
    let input = FindingRaiseInput {
        severity: Severity::Minor,
        blocking: None,
        check_id: None,
        assessment_id: None,
        actor: "operator".into(),
        message: "Review".into(),
        revision: None,
        dirty: None,
    };
    let finding = store.raise_task_finding(owner, &input).unwrap();
    let foreign_root = fixture.database_path.with_extension("foreign-root");
    std::fs::create_dir_all(&foreign_root).unwrap();
    store
        .create_workspace("foreign", "Foreign", &foreign_root)
        .unwrap();
    let foreign_store = store.records_in_workspace("foreign").unwrap();
    let foreign_task = foreign_store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            task_kind: None,
            body: "Foreign Task".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    let foreign_finding = foreign_store
        .raise_task_finding(foreign_task.id, &input)
        .unwrap();
    let foreign_check = foreign_store.list_task_checks(foreign_task.id).unwrap()[0].id;
    let foreign_assessment = foreign_store
        .assess_task_check(
            foreign_task.id,
            foreign_check,
            &forge_app::checks::AssessmentInput {
                expected_version: 1,
                actor: "foreign".into(),
                verdict: forge_app::checks::Verdict::Pass,
                message: "Verified".into(),
                revision: None,
                dirty: None,
            },
        )
        .unwrap()
        .latest_assessment
        .unwrap()
        .id;
    let credentials =
        serde_json::json!({"assignmentId":assignment,"secret":"assignment-test-secret"});
    let mut requests = Vec::new();
    for (method, fields) in [
        ("assignment.findingList", serde_json::json!({"limit":1})),
        (
            "assignment.findingShow",
            serde_json::json!({"finding":finding.id,"limit":1}),
        ),
        (
            "assignment.findingRaise",
            serde_json::json!({"severity":"minor","message":"Review"}),
        ),
        (
            "assignment.findingMutate",
            serde_json::json!({"finding":finding.id,"expectedVersion":1,"message":"Review","change":{"action":"note"}}),
        ),
    ] {
        let mut request = credentials.clone();
        request
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        for field in ["task", "target", "actor", "workspace", "unexpected"] {
            let mut spoof = request.clone();
            spoof[field] = "override".into();
            let response = assignment_rpc(daemon.clone(), method, spoof).await;
            assert_eq!(response["error"]["code"], -32602, "{method}: {response}");
        }
        let mut invalid = request.clone();
        invalid["secret"] = "wrong-secret".into();
        let response = assignment_rpc(daemon.clone(), method, invalid).await;
        assert_eq!(
            response["error"]["data"], "assignment-capability-denied",
            "{response}"
        );
        assert!(!response.to_string().contains("assignment-test-secret"));
        requests.push((method, request));
    }
    for id in [foreign_finding.id, u64::MAX] {
        for (method, request) in [&requests[1], &requests[3]] {
            let mut request = request.clone();
            request["finding"] = id.into();
            let response = assignment_rpc(daemon.clone(), method, request).await;
            assert_eq!(response["error"]["data"], "finding-not-found", "{response}");
        }
    }
    for (field, origin) in [
        ("checkId", foreign_check),
        ("assessmentId", foreign_assessment),
    ] {
        for id in [origin, u64::MAX] {
            let mut request = requests[2].1.clone();
            request[field] = id.into();
            let response = assignment_rpc(daemon.clone(), "assignment.findingRaise", request).await;
            assert_eq!(
                response["error"]["data"], "invalid-finding-origin",
                "{response}"
            );
        }
    }
    // Inject failure at the public operation's final log boundary; inspect only public state.
    let before = store.list_record_updates(owner).unwrap();
    let connection = rusqlite::Connection::open(&fixture.database_path).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_assignment_finding_log BEFORE INSERT ON record_updates WHEN NEW.kind = 'finding-activity' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").unwrap();
    for (method, request) in [&requests[2], &requests[3]] {
        assert!(
            assignment_rpc(daemon.clone(), method, request.clone())
                .await
                .get("error")
                .is_some()
        );
    }
    assert_eq!(
        store
            .list_task_findings(owner, None, 100)
            .unwrap()
            .findings
            .len(),
        1
    );
    assert_eq!(
        store
            .show_task_finding(owner, finding.id, None, 100)
            .unwrap()
            .activities
            .len(),
        1
    );
    assert_eq!(store.list_record_updates(owner).unwrap(), before);
    connection
        .execute_batch("DROP TRIGGER fail_assignment_finding_log;")
        .unwrap();
    let secret = fixture
        .database_path
        .with_extension("assignment-secrets")
        .join(&assignment);
    std::fs::remove_file(&secret).unwrap();
    for (method, request) in &requests {
        assert_eq!(
            assignment_rpc(daemon.clone(), method, request.clone()).await["error"]["data"],
            "assignment-capability-denied"
        );
    }
    std::fs::write(&secret, "assignment-test-secret").unwrap();
    store
        .close_agent_assignment(
            &assignment,
            forge_app::AgentAssignmentClosureReason::Retired,
        )
        .unwrap();
    for (method, request) in &requests {
        assert_eq!(
            assignment_rpc(daemon.clone(), method, request.clone()).await["error"]["data"],
            "assignment-capability-denied"
        );
    }
    std::fs::remove_dir_all(foreign_root).unwrap();
}
