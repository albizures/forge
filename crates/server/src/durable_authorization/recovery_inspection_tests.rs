use super::decode;
use forge_app::durable_execution::Launch;
use forge_rpc::durable_execution::recovery_inspection::PendingStatus;
use serde_json::{Value, json};

fn launch() -> Launch {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../../runner/test/wire-v1.json")).unwrap();
    serde_json::from_value(fixture["operation"]["launch"].clone()).unwrap()
}
fn response() -> Value {
    json!({"protocol": 1, "launch": launch(), "candidate": {
        "root": 1, "admission_id": "original", "status": "queued",
    }})
}

#[test]
fn recovery_reply_requires_exact_shape_binding_pending_status_and_safe_root() {
    let launch = launch();
    for (status, expected) in [
        ("queued", PendingStatus::Queued),
        ("placed", PendingStatus::Placed),
    ] {
        let mut value = response();
        value["candidate"]["status"] = status.into();
        let input = decode(&serde_json::to_vec(&value).unwrap(), &launch, "original").unwrap();
        assert_eq!(input.root, 1);
        assert_eq!(input.admission_id, "original");
        assert_eq!(input.status, expected);
    }
    let mut invalid = vec![json!(null), json!([])];
    for key in response()["launch"].as_object().unwrap().keys() {
        let mut value = response();
        value["launch"][key] = if matches!(key.as_str(), "profile_revision" | "runtime_schema") {
            json!(99)
        } else {
            json!("foreign")
        };
        invalid.push(value);
    }
    for status in [
        json!("done"),
        json!("unanswered"),
        json!("continue"),
        json!("Queued"),
        json!(null),
        json!(1),
    ] {
        let mut value = response();
        value["candidate"]["status"] = status;
        invalid.push(value);
    }
    for root in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!(9_007_199_254_740_992_u64),
        json!("1"),
        json!(null),
    ] {
        let mut value = response();
        value["candidate"]["root"] = root;
        invalid.push(value);
    }
    for path in [
        vec!["protocol"],
        vec!["launch"],
        vec!["candidate"],
        vec!["candidate", "admission_id"],
        vec!["candidate", "root"],
        vec!["candidate", "status"],
    ] {
        let mut value = response();
        if path.len() == 1 {
            value.as_object_mut().unwrap().remove(path[0]);
        } else {
            value[path[0]].as_object_mut().unwrap().remove(path[1]);
        }
        invalid.push(value);
    }
    for (field, value) in [("protocol", json!(2)), ("extra", json!("PRIVATE-CANARY"))] {
        let mut changed = response();
        changed[field] = value;
        invalid.push(changed);
    }
    for (field, value) in [
        ("admission_id", json!("foreign")),
        ("admission_id", json!(1)),
        ("command", json!("PRIVATE-CANARY")),
    ] {
        let mut changed = response();
        changed["candidate"][field] = value;
        invalid.push(changed);
    }
    for value in invalid {
        let error = decode(&serde_json::to_vec(&value).unwrap(), &launch, "original").unwrap_err();
        assert_eq!(error.code, "durable-recovery-inspection-unavailable");
        assert!(!error.message.contains("PRIVATE-CANARY"));
    }
    for bytes in [b"PRIVATE-CANARY invalid JSON".as_slice(), b"{} trailing"] {
        assert!(decode(bytes, &launch, "original").is_err());
    }
    let mut empty = response();
    empty["candidate"]["admission_id"] = json!("");
    assert!(decode(&serde_json::to_vec(&empty).unwrap(), &launch, "").is_err());
}
