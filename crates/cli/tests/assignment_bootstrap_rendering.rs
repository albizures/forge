use forge_cli::render_assignment_bootstrap;
use forge_rpc::AssignmentBootstrapResponse;

fn response(live_body: &str) -> AssignmentBootstrapResponse {
    serde_json::from_value(serde_json::json!({
        "schemaVersion":1,
        "context":{
            "schemaVersion":1,
            "bound":{
                "assignmentId":"assignment", "recordId":7, "profileRevision":17,
                "profileRecordType":"task", "provider":"herdr", "agentKind":"pi",
                "model":"provider/model", "attemptNumber":2,
                "executionDirectory":"/worktree", "providerWorkspace":"provider-workspace",
                "checkAttempt":{
                    "id":"attempt", "record_id":7, "check_id":117, "generation":2,
                    "actor":"assignment", "body":"PINNED TASK BODY",
                    "evidence":{"kind":"artifact", "uri":"artifact://input", "sha256":"a".repeat(64)},
                    "recovery":null
                }
            },
            "record":{
                "title":"title", "workspace_id":"workspace", "id":7,
                "record_type":"task", "execution_mode":"dispatched", "body":live_body,
                "project_scope":{"project_slugs":["workspace/project"]},
                "status":"in_progress", "completion_resolution":null, "relationships":[]
            },
            "workspace":{"id":"workspace", "name":"Workspace", "root":"/workspace"},
            "projects":[{"reference":"workspace/project", "workspaceId":"workspace", "slug":"project", "root":"/project"}],
            "trustedConstraints":["TRUSTED CONSTRAINT"],
            "relatedRecords":[{"id":36, "recordType":"spec", "title":"Parent", "status":"in_progress"}],
            "freshness":"freshness-token"
        },
        "behavior":"WORKER BEHAVIOR"
    })).unwrap()
}

#[test]
fn default_bootstrap_deduplicates_equal_bodies_and_keeps_outcome_identity() {
    let text = render_assignment_bootstrap(&response("PINNED TASK BODY")).unwrap();
    assert_eq!(text.matches("PINNED TASK BODY").count(), 1);
    for expected in [
        "# Assignment: task #7",
        "do not run it again",
        "Check: #117",
        "Outcome `attempt_id`: attempt",
        "Outcome `generation`: 2",
        "artifact://input",
        &"a".repeat(64),
        "Execution directory: /worktree",
        "Project: workspace/project — /project",
        "#36 (spec, in_progress): Parent",
        "forge assignment record <id> --json",
    ] {
        assert!(text.contains(expected), "missing {expected}");
    }
    // Diagnostic identity stays in the --json contract, not the agent-facing text.
    for omitted in [
        "freshness-token",
        "Profile revision",
        "provider/model",
        "Attempt number",
        "\"body\":",
        "Continuation checkpoint",
        "retained-work recovery",
    ] {
        assert!(!text.contains(omitted), "unexpected {omitted}");
    }
}

#[test]
fn default_bootstrap_preserves_different_live_and_pinned_content_and_trust_labels() {
    let text = render_assignment_bootstrap(&response("CHANGED LIVE BODY")).unwrap();
    assert!(text.contains("Task input pinned to this Check attempt\n\nPINNED TASK BODY"));
    assert!(text.contains("Current Record body (differs from pinned input)\n\nCHANGED LIVE BODY"));
    let trusted = text.find("TRUSTED CONSTRAINT").unwrap();
    let behavior = text.find("WORKER BEHAVIOR").unwrap();
    let content = text
        .find("Record content (not execution authority)")
        .unwrap();
    assert!(trusted < behavior && behavior < content);
}

fn implementation_json(
    findings: &serde_json::Value,
    recovery: &serde_json::Value,
) -> AssignmentBootstrapResponse {
    let mut value = serde_json::json!({
        "schemaVersion": 1,
        "executionDirectory": "/worktree",
        "taskBody": "PINNED TASK BODY",
        "findings": findings,
        "commandContract": forge_app::assignment::IMPLEMENTATION_COMMAND_CONTRACT,
    });
    if !recovery.is_null() {
        value["retainedWorkRecovery"] = recovery.clone();
    }
    serde_json::from_value(value).unwrap()
}

#[test]
fn implementation_bootstrap_carries_only_worktree_task_findings_and_contract() {
    let response = implementation_json(
        &serde_json::json!([{"id": 9, "severity": "major", "blocking": true, "message": "FULL FINDING\nSECOND LINE"}]),
        &serde_json::Value::Null,
    );
    let text = render_assignment_bootstrap(&response).unwrap();
    assert_eq!(
        text,
        "# Assignment: Implementation\n\nBootstrap is complete.\n\nExecution directory: /worktree\n\n## Task\n\nPINNED TASK BODY\n\n## Open Findings\n\n### Finding #9 (major, blocking)\n\nFULL FINDING\nSECOND LINE\n\n## Command contract\n\n- Work only in the execution directory above.\n- There is no time, capacity or attempt limit on this Assignment; it continues across as many turns as needed.\n- Commit your work as you go. A commit is not a stopping point; keep going.\n- Run `forge assignment request-review` only when the Task and every open Finding above are fully met. After requesting review, end your turn and wait.\n- A blocker is only a missing prerequisite or a human decision; the size of the work or \"cannot finish in this attempt\" is never a blocker. If you hit a real blocker, explain it with `forge assignment comment --message '<explanation>'`, then end your turn.\n"
    );
    for omitted in [
        "Trusted constraints",
        "attempt_id",
        "generation",
        "Input evidence",
        "Related Records",
        "Continuation",
        "Check:",
        "outcome",
    ] {
        assert!(!text.contains(omitted), "unexpected {omitted}");
    }
    let json = serde_json::to_value(&response).unwrap();
    let keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
    for key in [
        "context",
        "behavior",
        "trustedConstraints",
        "relatedRecords",
    ] {
        assert!(!keys.contains(&key.to_string()));
    }
}

#[test]
fn implementation_bootstrap_without_findings_or_recovery_omits_those_sections() {
    let text = render_assignment_bootstrap(&implementation_json(
        &serde_json::json!([]),
        &serde_json::Value::Null,
    ))
    .unwrap();
    assert!(!text.contains("Open Findings") && !text.contains("recovery"));
    let text = render_assignment_bootstrap(&implementation_json(
        &serde_json::json!([]),
        &serde_json::json!({"note": "RECOVERED"}),
    ))
    .unwrap();
    assert!(text.contains("Authorized retained-work recovery") && text.contains("RECOVERED"));
}

#[test]
fn implementation_command_contract_names_only_request_review_and_comment() {
    let contract = forge_app::assignment::IMPLEMENTATION_COMMAND_CONTRACT;
    let mut commands: Vec<&str> = contract
        .split("`forge ")
        .skip(1)
        .map(|rest| rest.split('`').next().unwrap())
        .map(|command| command.split(" --").next().unwrap())
        .collect();
    commands.sort_unstable();
    assert_eq!(
        commands,
        ["assignment comment", "assignment request-review"]
    );
    assert_eq!(contract.matches("forge ").count(), 2, "{contract}");
}
