use forge_rpc::RecordRelationshipResponse;

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
