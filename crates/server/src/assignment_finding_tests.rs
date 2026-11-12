#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn assignment_finding_rpc_retains_cycles_and_rejects_overrides_atomically() {
    let (fixture, assignment, neighbor) = assignment_test_daemon();
    let daemon = fixture.daemon();
    let credentials =
        serde_json::json!({"assignmentId":assignment,"secret":"assignment-test-secret"});
    let mut raise = credentials.clone();
    raise["severity"] = "critical".into();
    raise["message"] = "Review issue".into();
    let raised = assignment_rpc(daemon.clone(), "assignment.findingRaise", raise.clone()).await;
    assert_eq!(raised["result"]["state"], "open", "{raised}");
    let id = raised["result"]["id"].as_u64().unwrap();
    assert_eq!(raised["result"]["raised"]["assignment_id"], assignment);
    let mut mutation = credentials.clone();
    mutation["finding"] = id.into();
    mutation["message"] = "Review action".into();
    for (version, change, state) in [
        (
            1,
            serde_json::json!({"action":"respond","response":"fixed"}),
            "awaiting-review",
        ),
        (
            2,
            serde_json::json!({"action":"respond","response":"rejected"}),
            "awaiting-review",
        ),
        (3, serde_json::json!({"action":"resolve"}), "resolved"),
        (4, serde_json::json!({"action":"note"}), "resolved"),
        (5, serde_json::json!({"action":"reopen"}), "open"),
        (
            6,
            serde_json::json!({"action":"amend","severity":"minor","blocking":false}),
            "open",
        ),
        (7, serde_json::json!({"action":"dismiss"}), "dismissed"),
    ] {
        mutation["expectedVersion"] = version.into();
        mutation["change"] = change;
        let result =
            assignment_rpc(daemon.clone(), "assignment.findingMutate", mutation.clone()).await;
        assert_eq!(result["result"]["state"], state, "{result}");
        assert_eq!(result["result"]["version"], version + 1);
        assert_eq!(result["result"]["latest_activity"]["actor"], assignment);
        assert_eq!(
            result["result"]["latest_activity"]["assignment_id"],
            assignment
        );
    }
    let stale = assignment_rpc(daemon.clone(), "assignment.findingMutate", mutation.clone()).await;
    assert_eq!(stale["error"]["data"], "stale-version", "{stale}");
    mutation["expectedVersion"] = 8.into();
    for field in [
        "actor",
        "task",
        "target",
        "assignment_id",
        "mutation",
        "unexpected",
    ] {
        let mut spoof = mutation.clone();
        spoof[field] = "override".into();
        let result = assignment_rpc(daemon.clone(), "assignment.findingMutate", spoof).await;
        assert!(result.get("error").is_some(), "{field}: {result}");
    }
    let mut malformed = mutation.clone();
    malformed["change"] = serde_json::json!({"action":"note","actor":"override"});
    assert!(
        assignment_rpc(daemon.clone(), "assignment.findingMutate", malformed)
            .await
            .get("error")
            .is_some()
    );
    let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
    let owner = store.agent_assignment(&assignment).unwrap().record_id;
    let foreign_check = store.list_record_checks(neighbor).unwrap()[0].id;
    for check in [foreign_check, u64::MAX] {
        raise["checkId"] = check.into();
        let result = assignment_rpc(daemon.clone(), "assignment.findingRaise", raise.clone()).await;
        assert_eq!(
            result["error"]["data"], "invalid-finding-origin",
            "{result}"
        );
    }
    let mut show = credentials.clone();
    show["finding"] = id.into();
    show["limit"] = 3.into();
    let first = assignment_rpc(daemon.clone(), "assignment.findingShow", show.clone()).await;
    assert_eq!(first["result"]["activities"].as_array().unwrap().len(), 3);
    assert_eq!(first["result"]["finding"]["version"], 8);
    show["after"] = first["result"]["next_cursor"].clone();
    let next = assignment_rpc(daemon.clone(), "assignment.findingShow", show).await;
    assert_eq!(next["result"]["activities"][0]["action"], "resolve");
    let mut list = credentials;
    list["limit"] = 1.into();
    let result = assignment_rpc(daemon.clone(), "assignment.findingList", list.clone()).await;
    assert_eq!(result["result"]["findings"].as_array().unwrap().len(), 1);
    list["limit"] = 101.into();
    assert_eq!(
        assignment_rpc(daemon.clone(), "assignment.findingList", list).await["error"]["data"],
        "invalid-page"
    );
    let page = store.show_record_finding(owner, id, None, 100).unwrap();
    assert_eq!(page.activities.len(), 8);
    assert!(
        page.activities
            .iter()
            .all(|a| a.assignment_id.as_deref() == Some(assignment.as_str()))
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn assignment_finding_security_done_and_completion_gates_match_operator() {
    use forge_app::findings::{
        FindingMutation, FindingMutationInput, FindingRaiseInput, FindingResolveInput,
        FindingResponse, FindingState, Severity,
    };
    let (fixture, assignment, prerequisite) = assignment_test_daemon();
    let root = fixture
        .database_path
        .with_extension("finding-lifecycle-secrets");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(&assignment), "actual-secret").unwrap();
    let daemon = fixture.daemon();
    let session = daemon
        .application
        .assignment(&root, &assignment, "actual-secret")
        .unwrap();
    let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
    let owner = store.agent_assignment(&assignment).unwrap().record_id;
    let raise = FindingRaiseInput {
        severity: Severity::Minor,
        blocking: None,
        check_id: None,
        assessment_id: None,
        actor: "spoof".into(),
        message: "Issue".into(),
        revision: Some("a".repeat(40)),
        dirty: Some(false),
    };
    let minor = session.finding_raise(&raise).unwrap();
    let critical = session
        .finding_raise(&FindingRaiseInput {
            severity: Severity::Critical,
            ..raise.clone()
        })
        .unwrap();
    let foreign = store.raise_record_finding(prerequisite, &raise).unwrap();
    let change = |version, change| FindingMutationInput {
        change,
        explanation: FindingResolveInput {
            expected_version: version,
            actor: "spoof".into(),
            message: "Reviewed".into(),
            revision: Some("b".repeat(40)),
            dirty: None,
        },
    };
    let before = store.list_record_updates(owner).unwrap();
    for foreign_id in [foreign.id, u64::MAX] {
        assert_eq!(
            session
                .finding_show(foreign_id, None, 1, None)
                .unwrap_err()
                .code,
            "finding-not-found"
        );
        assert_eq!(
            session
                .finding_mutate(foreign_id, &change(1, FindingMutation::Note))
                .unwrap_err()
                .code,
            "finding-not-found"
        );
    }
    let assessment = store.list_record_checks(prerequisite).unwrap()[0]
        .latest_assessment
        .as_ref()
        .unwrap()
        .id;
    for origin in [assessment, u64::MAX] {
        assert_eq!(
            session
                .finding_raise(&FindingRaiseInput {
                    assessment_id: Some(origin),
                    ..raise.clone()
                })
                .unwrap_err()
                .code,
            "invalid-finding-origin"
        );
    }
    assert_eq!(
        session
            .finding_raise(&FindingRaiseInput {
                message: " ".into(),
                ..raise.clone()
            })
            .unwrap_err()
            .code,
        "invalid-finding"
    );
    assert_eq!(
        session
            .finding_raise(&FindingRaiseInput {
                revision: Some("short".into()),
                ..raise.clone()
            })
            .unwrap_err()
            .code,
        "invalid-revision"
    );
    assert_eq!(
        session
            .finding_mutate(minor.id, &change(99, FindingMutation::Note))
            .unwrap_err()
            .code,
        "stale-version"
    );
    assert_eq!(store.list_record_updates(owner).unwrap(), before);
    let minor = session
        .finding_mutate(
            minor.id,
            &change(
                1,
                FindingMutation::Respond {
                    response: FindingResponse::Rejected,
                },
            ),
        )
        .unwrap();
    session
        .finding_mutate(
            critical.id,
            &change(
                1,
                FindingMutation::Respond {
                    response: FindingResponse::Fixed,
                },
            ),
        )
        .unwrap();
    store.start_owned_record(prerequisite, "target-assignment").unwrap();
    store.done_record(prerequisite, Some("verified")).unwrap();
    session.acknowledge().unwrap();
    session.start().unwrap();
    let operator = store.done_record(owner, Some("done")).unwrap_err();
    let assigned = session.done(Some("done"), None).unwrap_err();
    assert_eq!(operator.code, "completion-blocked");
    assert_eq!(operator.code, assigned.code);
    assert_eq!(operator.message, assigned.message);
    assert!(
        operator
            .message
            .contains(&format!("Finding IDs: [{}]", critical.id))
    );
    check_tests::pass_checks(&store, owner);
    assert_eq!(
        session.done(Some("done"), None).unwrap_err().code,
        "completion-blocked"
    );
    session
        .finding_mutate(critical.id, &change(2, FindingMutation::Resolve))
        .unwrap();
    session.done(Some("done"), None).unwrap();
    assert_eq!(
        session.finding_raise(&raise).unwrap_err().code,
        "record-done"
    );
    assert_eq!(
        session
            .finding_mutate(critical.id, &change(3, FindingMutation::Reopen))
            .unwrap_err()
            .code,
        "record-done"
    );
    assert_eq!(
        session
            .finding_mutate(
                minor.id,
                &change(
                    minor.version,
                    FindingMutation::Amend {
                        severity: None,
                        blocking: Some(true)
                    }
                )
            )
            .unwrap_err()
            .code,
        "record-done"
    );
    let closed = session
        .finding_mutate(minor.id, &change(minor.version, FindingMutation::Dismiss))
        .unwrap();
    assert_eq!(closed.state, FindingState::Dismissed);
    let reopened = session
        .finding_mutate(minor.id, &change(closed.version, FindingMutation::Reopen))
        .unwrap();
    let responded = session
        .finding_mutate(
            minor.id,
            &change(
                reopened.version,
                FindingMutation::Respond {
                    response: FindingResponse::Fixed,
                },
            ),
        )
        .unwrap();
    let resolved = session
        .finding_mutate(
            minor.id,
            &change(responded.version, FindingMutation::Resolve),
        )
        .unwrap();
    session
        .finding_mutate(minor.id, &change(resolved.version, FindingMutation::Note))
        .unwrap();
    assert_eq!(
        store.show_record(owner).unwrap().status,
        forge_app::RecordStatus::Done
    );
    // A Done Task does not revive revoked delivery or closed Assignments, even for notes/reads.
    std::fs::write(root.join(&assignment), "revoked-and-replaced").unwrap();
    assert_eq!(
        session.finding_list(None, 1, None).unwrap_err().code,
        "assignment-capability-denied"
    );
    assert_eq!(
        session
            .finding_mutate(minor.id, &change(7, FindingMutation::Note))
            .unwrap_err()
            .code,
        "assignment-capability-denied"
    );
    std::fs::write(root.join(&assignment), "actual-secret").unwrap();
    store
        .close_agent_assignment(
            &assignment,
            forge_app::AgentAssignmentClosureReason::Retired,
        )
        .unwrap();
    assert_eq!(
        session
            .finding_show(minor.id, None, 1, None)
            .unwrap_err()
            .code,
        "assignment-capability-denied"
    );
    assert_eq!(
        session.finding_raise(&raise).unwrap_err().code,
        "assignment-capability-denied"
    );
    assert_eq!(
        session
            .finding_mutate(minor.id, &change(7, FindingMutation::Note))
            .unwrap_err()
            .code,
        "assignment-capability-denied"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn assignment_findings_derive_attribution_and_retain_provenance() {
    use forge_app::findings::{FindingRaiseInput, Severity};
    let (fixture, assignment, _) = assignment_test_daemon();
    let root = fixture.database_path.with_extension("finding-secrets");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(&assignment), "actual-secret\n").unwrap();
    let daemon = fixture.daemon();
    let session = daemon
        .application
        .assignment(&root, &assignment, "actual-secret")
        .unwrap();
    let finding = session
        .finding_raise(&FindingRaiseInput {
            severity: Severity::Minor,
            blocking: None,
            check_id: None,
            assessment_id: None,
            actor: "spoofed".into(),
            message: "Review issue".into(),
            revision: None,
            dirty: Some(false),
        })
        .unwrap();
    assert_eq!(finding.raised.actor, assignment);
    assert_eq!(
        finding.raised.assignment_id.as_deref(),
        Some(assignment.as_str())
    );
    let page = session.finding_show(finding.id, None, 1, None).unwrap();
    assert_eq!(page.finding, finding);
    assert_eq!(page.activities, vec![finding.raised]);
    assert_eq!(
        session.finding_list(None, 1, None).unwrap().findings.len(),
        1
    );
    std::fs::remove_dir_all(root).unwrap();
}
