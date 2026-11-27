use forge_rpc::RecordRelationshipResponse;

#[test]
fn application_inspection_emits_canonical_waits_for_and_preserves_unrelated_kinds() {
    use forge_app::{
        ExecutionMode, Record, RecordRelationship, RecordRelationshipKind, RecordStatus, RecordType,
    };
    let edge = |kind, ordering_only, to_id| RecordRelationship {
        kind,
        ordering_only,
        from_id: 42,
        to_id,
        parent_id: 0,
        child_id: 0,
    };
    let record = Record {
        planning_context: vec![],
        workspace_id: "isolated".into(),
        id: 42,
        record_type: RecordType::Task,
        body: "wait without code inheritance".into(),
        project_slugs: vec![],
        execution_mode: ExecutionMode::Manual,
        status: RecordStatus::Open,
        completion_resolution: None,
        relationships: vec![
            edge(RecordRelationshipKind::WaitsFor, true, 17),
            edge(RecordRelationshipKind::DependsOn, true, 18),
            edge(RecordRelationshipKind::BuildsOn, false, 19),
            edge(RecordRelationshipKind::GeneratedBy, false, 20),
        ],
    };
    let assigned = forge_rpc::AssignmentLiveRecord::from(record.clone());
    let response = forge_rpc::RecordResponse::from(record);
    let serialized = serde_json::to_value(&response).unwrap();
    let assigned = serde_json::to_value(assigned).unwrap();
    assert_eq!(assigned["relationships"], serialized["relationships"]);
    assert!(
        assigned["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .all(|edge| edge.get("ordering_only").is_none())
    );
    for (index, kind) in ["waits-for", "waits-for", "builds-on", "generated-by"]
        .iter()
        .enumerate()
    {
        assert_eq!(serialized["relationships"][index]["kind"], *kind);
    }
    assert_eq!(
        serde_json::from_value::<forge_rpc::RecordResponse>(serialized).unwrap(),
        response
    );
}

#[test]
fn prerequisite_inspection_round_trips_kind_without_a_competing_flag() {
    for kind in ["waits-for", "builds-on"] {
        let wire = serde_json::json!({
            "kind": kind, "from_id": 42, "to_id": 17, "parent_id": 0, "child_id": 0,
        });
        let relationship: RecordRelationshipResponse =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(relationship).unwrap(), wire);
    }
}
