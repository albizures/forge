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
fn default_bootstrap_deduplicates_equal_bodies_without_losing_attempt_identity() {
    let text = render_assignment_bootstrap(&response("PINNED TASK BODY")).unwrap();
    assert_eq!(text.matches("PINNED TASK BODY").count(), 1);
    for expected in [
        "\"id\": \"attempt\"",
        "\"generation\": 2",
        "\"check_id\": 117",
        "artifact://input",
        &"a".repeat(64),
        "\"continuation\": null",
        "\"recovery\": null",
        "Freshness: freshness-token",
        "Profile revision: 17",
        "Execution directory: /worktree",
        "Project: workspace/project — /project",
        "#36 (spec, in_progress): Parent",
    ] {
        assert!(text.contains(expected), "missing {expected}");
    }
    assert!(!text.contains("\"body\":"));
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
