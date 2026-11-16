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
    let response = forge_rpc::RecordResponse::from(Record {
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
            edge(RecordRelationshipKind::DependsOn, false, 19),
            edge(RecordRelationshipKind::GeneratedBy, false, 20),
        ],
    });
    let serialized = serde_json::to_value(&response).unwrap();
    for (index, kind) in ["waits-for", "waits-for", "depends-on", "generated-by"]
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
fn dependency_inspection_preserves_explicit_intent_and_defaults_legacy_responses() {
    let legacy = serde_json::json!({
        "kind": "depends-on", "from_id": 42, "to_id": 17, "parent_id": 0, "child_id": 0,
    });
    let mut relationship: RecordRelationshipResponse = serde_json::from_value(legacy).unwrap();
    assert!(!relationship.ordering_only);
    relationship.ordering_only = true;
    let serialized = serde_json::to_value(&relationship).unwrap();
    assert_eq!(serialized["kind"], "depends-on");
    assert_eq!(serialized["ordering_only"], true);
    assert_eq!(
        serde_json::from_value::<RecordRelationshipResponse>(serialized).unwrap(),
        relationship
    );
}
