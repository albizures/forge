use forge_rpc::{StackDestination, StackSource, TaskStackAction, TaskStackRequest};

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
