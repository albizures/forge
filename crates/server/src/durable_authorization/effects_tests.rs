use super::{decode, read_effects};
use forge_app::durable_execution::Launch;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "forge-effect-observer-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn effect_inspector_deadline_and_output_bound_fail_closed() {
    let fixture = Fixture::new();
    // Explicit trusted test programs stand in for a broken inspector. Shell
    // builtins keep each test in the directly spawned process, without children.
    for (index, script) in [
        "while :; do :; done",
        "while :; do printf 'SECRET-CANARY'; done",
        "printf 'SECRET-CANARY' >&2; exit 1",
    ]
    .iter()
    .enumerate()
    {
        let inspector = fixture.0.join(format!("inspector-{index}.sh"));
        std::fs::write(&inspector, script).unwrap();
        let error = read_effects(
            Path::new("/bin/sh"),
            &inspector,
            &fixture.0,
            b"{}",
            &launch(),
            Duration::from_secs(1),
            128,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "durable-effect-observation-unavailable");
        assert!(!error.message.contains("SECRET-CANARY"));
    }
}

fn launch() -> Launch {
    Launch {
        assignment_id: "assignment".into(),
        operation_id: "operation".into(),
        attempt_id: "attempt".into(),
        profile_revision: 1,
        runtime_id: "runtime".into(),
        incarnation_id: "incarnation".into(),
        conversation_id: "conversation".into(),
        store_id: "store".into(),
        forge_revision: "revision".into(),
        component_hash: "hash".into(),
        runtime_schema: 2,
    }
}

fn response() -> Value {
    json!({"protocol": 1, "launch": launch(), "effects": [{
        "task_id": "task", "conversation_id": "child", "call_id": "call"
    }]})
}

#[test]
fn effect_wire_requires_exact_binding_version_shape_and_bounded_identities() {
    let launch = launch();
    let effects = decode(&serde_json::to_vec(&response()).unwrap(), &launch).unwrap();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].conversation_id, "child");
    let mut invalid = vec![json!(null), json!([])];
    for key in [
        "assignment_id",
        "operation_id",
        "attempt_id",
        "runtime_id",
        "incarnation_id",
        "conversation_id",
        "store_id",
        "forge_revision",
        "component_hash",
    ] {
        let mut value = response();
        value["launch"][key] = "foreign".into();
        invalid.push(value);
    }
    for key in ["profile_revision", "runtime_schema"] {
        let mut value = response();
        value["launch"][key] = 99.into();
        invalid.push(value);
    }
    let mut extra = response();
    extra["unrecognized"] = true.into();
    invalid.push(extra);
    let mut extra = response();
    extra["effects"][0]["command"] = "SECRET-CANARY".into();
    invalid.push(extra);
    let mut version = response();
    version["protocol"] = 2.into();
    invalid.push(version);
    let mut missing = response();
    missing.as_object_mut().unwrap().remove("effects");
    invalid.push(missing);
    let mut oversized = response();
    oversized["effects"] = json!(vec![response()["effects"][0].clone(); 1001]);
    invalid.push(oversized);
    for identity in ["", " ", "line\nbreak", &"x".repeat(1025)] {
        let mut value = response();
        value["effects"][0]["call_id"] = identity.into();
        invalid.push(value);
    }
    for value in invalid {
        let error = decode(&serde_json::to_vec(&value).unwrap(), &launch).unwrap_err();
        assert_eq!(error.code, "durable-effect-observation-unavailable");
        assert!(!error.message.contains("SECRET-CANARY"));
    }
    assert!(decode(b"SECRET-CANARY invalid JSON", &launch).is_err());
    let mut empty = response();
    empty["effects"] = json!([]);
    assert!(
        decode(&serde_json::to_vec(&empty).unwrap(), &launch)
            .unwrap()
            .is_empty()
    );
}
