use forge_cli::{render_record, render_record_list};
use forge_rpc::{ListRecordsResponse, RecordResponse, TaskActivityState};

fn record() -> RecordResponse {
    serde_json::from_value(serde_json::json!({
        "workspace_id": "test", "id": 7, "record_type": "task", "body": "Inspect",
        "execution_mode": "dispatched", "project_scope": {"project_slugs": ["api"]},
        "status": "in_progress", "completion_resolution": null, "relationships": [],
        "checks": [{"id": 1, "record_id": 7, "name": "Review", "description": "Review output",
            "position": 0, "profile_revision": 1, "version": 1, "latest_assessment": null,
            "current_verdict": null, "project": "api"}],
        "activity": {"state": "closed", "assignment_id": "retired-attempt",
            "historical_assignment_state": "working", "closed_at_millis": 42,
            "acknowledged_at_millis": 1, "retained_resources": 0,
            "dispatch_hold": "Human input required", "stack": null}
    }))
    .unwrap()
}

#[test]
fn task_inspection_closed_worker_hold_and_pending_checks_render_together() {
    let record = record();
    let text = render_record(&record);
    for expected in [
        "in_progress",
        "no active assignment",
        "Human input required",
        "Checks remaining: 1/1",
        "Review [pending]",
        "assignment-info retired-attempt",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(!text.contains("working (persisted)"));
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["activity"]["state"], "closed");
    assert_eq!(json["activity"]["historical_assignment_state"], "working");
    assert_eq!(
        serde_json::from_value::<RecordResponse>(json).unwrap(),
        record
    );
    assert_eq!(
        render_record_list(&ListRecordsResponse {
            records: vec![record]
        }),
        text
    );
}

#[test]
fn task_inspection_unknown_retained_and_unacknowledged_are_not_absent() {
    let mut record = record();
    for (state, expected) in [
        (TaskActivityState::Absent, "no assignment"),
        (TaskActivityState::Provisioning, "provisioning (persisted)"),
        (TaskActivityState::Working, "working (persisted)"),
        (TaskActivityState::Unknown, "agent absence not established"),
        (
            TaskActivityState::RetainedResources,
            "agent absence not established",
        ),
    ] {
        record.activity.as_mut().unwrap().state = state;
        assert!(render_record(&record).contains(expected));
    }
    // Additive contract: old responses still decode without an activity member.
    let mut json = serde_json::to_value(&record).unwrap();
    json.as_object_mut().unwrap().remove("activity");
    assert!(
        serde_json::from_value::<RecordResponse>(json)
            .unwrap()
            .activity
            .is_none()
    );
}
