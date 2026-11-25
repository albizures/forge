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
                    "continuation":null, "recovery":null
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
fn default_bootstrap_renders_continuation_handoff_when_present() {
    let mut value = response("PINNED TASK BODY");
    value.context.bound.check_attempt.as_mut().unwrap().continuation = Some(
        serde_json::from_value(serde_json::json!({
            "attempt_id": "previous", "evidence": {"kind":"artifact", "uri":"artifact://checkpoint", "sha256":"b".repeat(64)},
            "handoff": {"stopping_reason":"STOP", "completed_work":"DONE", "verification":"VERIFIED", "remaining_work":"REMAINING"}
        }))
        .unwrap(),
    );
    let text = render_assignment_bootstrap(&value).unwrap();
    assert!(text.contains("### Accepted Continuation checkpoint"));
    assert!(text.contains("REMAINING"));
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
