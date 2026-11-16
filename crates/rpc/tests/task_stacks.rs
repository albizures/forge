use forge_rpc::{StackDestination, StackSource, TaskStackAction, TaskStackRequest};

#[test]
fn assignment_unchanged_is_explicit_and_ordinary_reads_keep_the_old_wire_shape() {
    let old = serde_json::json!({"assignmentId": "bound", "secret": "test", "url": null});
    let mut request: forge_rpc::AssignmentStackRequest =
        serde_json::from_value(old.clone()).unwrap();
    assert!(!request.unchanged);
    assert_eq!(serde_json::to_value(&request).unwrap(), old);
    request.unchanged = true;
    let wire = serde_json::to_value(request).unwrap();
    assert_eq!(wire["unchanged"], true);
    assert!(
        serde_json::from_value::<forge_rpc::AssignmentStackRequest>(wire)
            .unwrap()
            .unchanged
    );
}

#[test]
fn old_stack_responses_default_to_no_unchanged_result() {
    let old = serde_json::json!({
        "record_id": 42, "predecessor": null,
        "destination": {"repository": "acme/widgets", "remote": "origin", "branch": "main"},
        "base_branch": "main", "base_commit": null, "publication": null
    });
    let stack: forge_rpc::TaskStack = serde_json::from_value(old).unwrap();
    assert!(stack.unchanged_result.is_none());
}

#[test]
fn stack_actions_round_trip_and_reject_unknown_fields() {
    for action in [
        TaskStackAction::Show,
        TaskStackAction::Configure {
            source: StackSource::Root {
                destination: StackDestination {
                    repository: "acme/widgets".into(),
                    remote: "origin".into(),
                    branch: "feature/payments".into(),
                },
            },
        },
        TaskStackAction::Configure {
            source: StackSource::Predecessor { record_id: 41 },
        },
        TaskStackAction::Prepare,
        TaskStackAction::PublishUnchanged,
        TaskStackAction::Publish {
            url: "https://github.com/acme/widgets/pull/123".into(),
        },
    ] {
        let wire = serde_json::to_value(TaskStackRequest {
            record_id: 49,
            action,
        })
        .unwrap();
        let request: TaskStackRequest = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), wire);
        let mut invalid = wire;
        invalid["unexpected"] = true.into();
        assert!(serde_json::from_value::<TaskStackRequest>(invalid).is_err());
    }
}
