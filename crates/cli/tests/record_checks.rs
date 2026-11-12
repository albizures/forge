use forge_app::{Application, WorkspaceRepository};
use forge_server::{Daemon, GitInspector, router};
use forge_store::SqliteProjectRepository;
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};
fn json(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(&String::from_utf8(output.stdout).unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn operators_submit_bound_manual_outcomes_with_evidence_freshness_and_durable_replay() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("forge.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("test", "Test", root.path()).unwrap();
    let app = Application::new(
        Box::new(SqliteProjectRepository::open(&database).unwrap()),
        Box::new(GitInspector),
    )
    .with_workspace_repository(Box::new(store));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/rpc", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router(Daemon::new(app)))
            .await
            .unwrap();
    });
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(args)
            .args(["--server-url", &endpoint])
            .current_dir(root.path())
            .output()
            .unwrap()
    };
    let task = json(run(&[
        "task",
        "open",
        "--body",
        "Deliver the report",
        "--json",
    ]))["id"]
        .to_string();
    let checks = json(run(&["record", "check", "list", &task, "--json"]));
    let check = checks[0]["id"].as_u64().unwrap();
    assert_eq!(checks[0]["name"], "Implementation");
    json(run(&["record", "start", &task, "--json"]));
    assert!(!run(&["record", "done", &task]).status.success());
    let evidence =
        serde_json::json!({"kind":"artifact", "uri":"urn:report:v1", "sha256":"a".repeat(64)});
    let path = root.path().join("input.json");
    let submit = |operation: &str, input: &serde_json::Value| {
        std::fs::write(&path, input.to_string()).unwrap();
        run(&[
            "record",
            "check",
            operation,
            &task,
            "--input-file",
            path.to_str().unwrap(),
            "--json",
        ])
    };
    let begin = serde_json::json!({"id":"manual-1", "check_id":check, "generation":1, "evidence":evidence, "actor":"operator"});
    let bound = json(submit("begin", &begin));
    assert_eq!(bound["body"], "Deliver the report");
    assert_eq!(json(submit("begin", &begin)), bound);
    let mut invalid = begin.clone();
    invalid["id"] = "competing".into();
    assert!(!submit("begin", &invalid).status.success());
    let outcome = serde_json::json!({"attempt_id":"manual-1", "generation":1, "evidence":evidence, "verdict":"pass", "actor":"operator", "message":"Verified report"});
    for (field, value) in [
        ("actor", serde_json::json!("other")),
        ("generation", serde_json::json!(2)),
        ("message", serde_json::json!(" ")),
    ] {
        let mut invalid = outcome.clone();
        invalid[field] = value;
        assert!(!submit("outcome", &invalid).status.success());
    }
    let mut unknown = outcome.clone();
    unknown["findings"] = serde_json::json!([]);
    assert!(
        !submit("outcome", &unknown).status.success(),
        "unsupported partial outcome fields must not be ignored"
    );
    // JSON stdin is supported, not only filesystem input.
    let mut child = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args([
            "record",
            "check",
            "outcome",
            &task,
            "--input-file",
            "-",
            "--json",
            "--server-url",
            &endpoint,
        ])
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(outcome.to_string().as_bytes())
        .unwrap();
    let accepted = json(child.wait_with_output().unwrap());
    assert_eq!(accepted["check"]["current_verdict"], "pass");
    assert_eq!(
        accepted["check"]["latest_assessment"]["attempt_id"],
        "manual-1"
    );
    assert_eq!(accepted["check"]["latest_assessment"]["evidence"], evidence);
    assert_eq!(json(submit("outcome", &outcome)), accepted);
    let mut conflict = outcome.clone();
    conflict["message"] = "changed".into();
    assert!(
        String::from_utf8_lossy(&submit("outcome", &conflict).stderr).contains("outcome-conflict")
    );
    let summary = json(run(&["record", "show", &task, "--json"]));
    assert_ne!(summary["status"], "done");
    let invalidate = serde_json::json!({"generation":1, "evidence":evidence, "actor":"operator", "message":"Same artifact, explicit fresh verification"});
    assert_eq!(
        json(submit("invalidate-work", &invalidate))["generation"],
        2
    );
    assert!(!run(&["record", "done", &task]).status.success());
    assert_eq!(
        json(submit("outcome", &outcome)),
        accepted,
        "historical exact replay is not a new pass"
    );
    let work = json(run(&["record", "check", "work", &task, "--json"]));
    assert_eq!(work["generation"], 2);
    let current = json(run(&["record", "check", "list", &task, "--json"]));
    assert!(current[0]["current_verdict"].is_null());
    let mut begin2 = begin.clone();
    begin2["id"] = "manual-2".into();
    begin2["generation"] = 2.into();
    json(submit("begin", &begin2));
    let mut outcome2 = outcome.clone();
    outcome2["attempt_id"] = "manual-2".into();
    outcome2["generation"] = 2.into();
    json(submit("outcome", &outcome2));
    json(run(&["record", "done", &task, "--json"]));
    assert_eq!(
        json(submit("outcome", &outcome)),
        accepted,
        "closed-Task replay remains immutable"
    );
    assert!(!submit("invalidate-work", &serde_json::json!({"generation":2, "evidence":evidence, "actor":"operator", "message":"Frozen"})).status.success());
    let page = json(run(&[
        "record",
        "check",
        "show",
        &task,
        &check.to_string(),
        "--json",
    ]));
    assert_eq!(page["activities"].as_array().unwrap().len(), 3);
    assert_eq!(page["assessments"].as_array().unwrap().len(), 2);
    server.abort();
}
