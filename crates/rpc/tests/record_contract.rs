use forge_rpc::{
    ListRecordsRequest, OpenTaskRequest, OpenTaskUnderSpecRequest, OpenTaskUnderWayfinderRequest,
    SelectedRecordRequest,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

#[test]
fn check_and_finding_payloads_use_record_ownership_without_aliases() {
    fn owner_contract<T: DeserializeOwned + Serialize>(valid: Value) {
        let response: T = serde_json::from_value(valid.clone()).unwrap();
        let serialized = serde_json::to_value(response).unwrap();
        assert_eq!(serialized["record_id"], 7);
        assert!(serialized.get("task_id").is_none());
        let mut invalid = valid;
        invalid["task_id"] = 7.into();
        assert!(serde_json::from_value::<T>(invalid.clone()).is_err());
        invalid.as_object_mut().unwrap().remove("record_id");
        assert!(serde_json::from_value::<T>(invalid).is_err());
    }
    owner_contract::<forge_rpc::RecordCheck>(json!({"id":1,"record_id":7,"name":"Implementation",
        "description":"Deliver all work","position":0,"version":1}));
    let activity = json!({"id":1,"finding_id":1,"action":"raise","actor":"reviewer",
        "message":"Review issue","created_at_millis":123,"comparison":"unknown"});
    owner_contract::<forge_rpc::Finding>(json!({"id":1,"record_id":7,"version":1,
        "severity":"major","blocking":true,"state":"open","raised":activity,"latest_activity":activity}));
}

#[test]
fn selected_requests_reject_duplicate_routing_and_owner_keys() {
    for input in [
        r#"{"workspace":"one","workspace":"two","record":7}"#,
        r#"{"workspace":"one","record":7,"record":8}"#,
        r#"{"workspace":"one","cwd":"/one","cwd":"/two","record":7}"#,
    ] {
        assert!(
            serde_json::from_str::<SelectedRecordRequest<forge_rpc::RecordCheckListRequest>>(input)
                .is_err()
        );
    }
}

fn rejects_obsolete_classification<T: DeserializeOwned + Serialize>(valid: &Value) {
    // The supported envelope must still round-trip with explicit Workspace routing.
    let request: SelectedRecordRequest<T> = serde_json::from_value(valid.clone()).unwrap();
    assert!(
        serde_json::to_value(request)
            .unwrap()
            .get("task_kind")
            .is_none()
    );
    for obsolete in ["task_kind", "kind"] {
        for value in [json!("research"), Value::Null] {
            let mut invalid = valid.clone();
            invalid[obsolete] = value;
            assert!(
                serde_json::from_value::<SelectedRecordRequest<T>>(invalid).is_err(),
                "obsolete {obsolete} must be rejected, not ignored"
            );
        }
    }
}

fn rejects_obsolete_owner_selector<T: DeserializeOwned + Serialize>(mut valid: Value) {
    valid["workspace"] = "test".into();
    valid["record"] = 7.into();
    let request: SelectedRecordRequest<T> = serde_json::from_value(valid.clone()).unwrap();
    assert_eq!(serde_json::to_value(request).unwrap()["record"], 7);
    for obsolete in ["task", "task_id"] {
        let mut with_alias = valid.clone();
        with_alias[obsolete] = 7.into();
        assert!(serde_json::from_value::<SelectedRecordRequest<T>>(with_alias.clone()).is_err());
        with_alias.as_object_mut().unwrap().remove("record");
        assert!(serde_json::from_value::<SelectedRecordRequest<T>>(with_alias).is_err());
    }
}

#[test]
fn record_check_and_finding_selectors_have_no_task_aliases() {
    rejects_obsolete_owner_selector::<forge_rpc::RecordCheckListRequest>(json!({}));
    rejects_obsolete_owner_selector::<forge_rpc::RecordCheckShowRequest>(
        json!({"check":1,"limit":50}),
    );
    rejects_obsolete_owner_selector::<forge_rpc::RecordCheckAssessRequest>(
        json!({"check":1,"assessment":{
        "expected_version":1,"verdict":"pass","actor":"operator","message":"Verified"}}),
    );
    rejects_obsolete_owner_selector::<forge_rpc::RecordCheckInvalidateRequest>(
        json!({"check":1,"invalidation":{
        "expected_version":1,"assessment_id":1,"actor":"operator","message":"Withdrawn"}}),
    );
    rejects_obsolete_owner_selector::<forge_rpc::RecordFindingListRequest>(json!({"limit":50}));
    rejects_obsolete_owner_selector::<forge_rpc::RecordFindingShowRequest>(
        json!({"finding":1,"limit":50}),
    );
    rejects_obsolete_owner_selector::<forge_rpc::RecordFindingRaiseRequest>(json!({"finding":{
        "severity":"major","actor":"operator","message":"Missing validation"}}));
    rejects_obsolete_owner_selector::<forge_rpc::RecordFindingResolveRequest>(
        json!({"finding":1,"resolution":{
        "expected_version":1,"actor":"operator","message":"Verified"}}),
    );
    rejects_obsolete_owner_selector::<forge_rpc::RecordFindingMutationRequest>(
        json!({"finding":1,"mutation":{
        "expected_version":1,"actor":"operator","message":"Fixed","change":{"action":"respond","response":"fixed"}}}),
    );
}

#[test]
fn record_creation_and_discovery_reject_removed_classification_inputs() {
    rejects_obsolete_classification::<OpenTaskRequest>(&json!({
        "workspace":"test", "body":"Describe any deliverable", "generated_by_ids":[]
    }));
    rejects_obsolete_classification::<OpenTaskUnderSpecRequest>(&json!({
        "workspace":"test", "body":"Describe any deliverable", "spec_id":1, "generated_by_ids":[]
    }));
    rejects_obsolete_classification::<OpenTaskUnderWayfinderRequest>(&json!({
        "workspace":"test", "body":"Describe any deliverable", "wayfinder_id":1, "generated_by_ids":[]
    }));
    rejects_obsolete_classification::<ListRecordsRequest>(&json!({"workspace":"test"}));
}
