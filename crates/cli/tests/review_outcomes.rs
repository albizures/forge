use forge_app::check_templates::{CheckCriterion, TemplateReplacement};
use forge_app::{
    Application, NewTypedRecord, Project, ProjectRepository, RecordRepository, RecordType,
    WorkspaceRepository,
};
use forge_server::{Daemon, GitInspector, router};
use forge_store::SqliteProjectRepository;
use serde_json::{Value, json};
use std::process::{Command, Output};

fn value(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(&String::from_utf8(output.stdout).unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn cli_publishes_atomic_handback_then_only_the_owning_review_verifies() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("forge.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("test", "Test", root.path()).unwrap();
    store
        .register(&Project {
            reference: "test/api".into(),
            workspace_id: "test".into(),
            slug: "api".into(),
            project_root: root.path().canonicalize().unwrap(),
            origin_url: "https://example.test/api.git".into(),
        })
        .unwrap();
    let selected = SqliteProjectRepository::open(&database)
        .unwrap()
        .in_record_workspace("test")
        .unwrap();
    selected
        .replace_check_template(
            "test/api",
            &TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Two reviews".into(),
                criteria: [
                    forge_app::check_templates::baseline(),
                    ["Review 1", "Review 2"]
                        .map(|name| CheckCriterion {
                            name: name.into(),
                            description: "Review report".into(),
                            profile_revision: None,
                        })
                        .to_vec(),
                ]
                .concat(),
            },
        )
        .unwrap();
    let task = selected
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver report".into(),
            project_slugs: vec!["test/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    selected
        .set_record_execution_mode(
            task.id,
            forge_app::ExecutionMode::Dispatched,
            forge_app::ExecutionMode::Manual,
            "fixture",
            "Manual review workflow",
        )
        .unwrap();
    selected.start_record(task.id).unwrap();
    let checks = selected.list_record_checks(task.id).unwrap();
    let task = task.id.to_string();
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
    let file = root.path().join("input.json");
    let submit = |operation: &str, input: Value| {
        std::fs::write(&file, input.to_string()).unwrap();
        run(&[
            "record",
            "check",
            operation,
            &task,
            "--input-file",
            file.to_str().unwrap(),
            "--json",
        ])
    };
    let evidence = json!({"kind":"artifact", "uri":"urn:report", "sha256":"a".repeat(64)});
    let begin = |index: usize, id: &str, generation: u64| {
        value(submit(
            "begin",
            json!({"id":id,"check_id":checks[index].id,"generation":generation,"evidence":evidence,"actor":"operator"}),
        ))
    };
    let outcome = |id: &str, generation: u64| json!({"attempt_id":id,"generation":generation,"evidence":evidence,"actor":"operator","verdict":"pass","message":"Verified"});
    for (index, id) in ["implementation", "review-1"].iter().enumerate() {
        begin(index, id, 1);
        value(submit("outcome", outcome(id, 1)));
    }
    begin(2, "handback", 1);
    let mut handback = outcome("handback", 1);
    handback["verdict"] = json!("fail");
    handback["disposition"] = json!("handback");
    handback["findings"] =
        json!([{"severity":"critical", "blocking":true, "message":"Fix report"}]);
    let mut forged_origin = handback.clone();
    forged_origin["findings"][0]["check_id"] = json!(checks[0].id);
    assert!(!submit("outcome", forged_origin).status.success());
    let accepted = value(submit("outcome", handback.clone()));
    assert_eq!(accepted["work"]["generation"], 2);
    assert_eq!(
        accepted["findings"][0]["verification_check_id"],
        checks[2].id
    );
    assert_eq!(value(submit("outcome", handback.clone())), accepted);
    let finding = accepted["findings"][0]["id"].to_string();
    let direct = run(&[
        "record",
        "finding",
        "resolve",
        &task,
        &finding,
        "--expected-version",
        "1",
        "--actor",
        "implementer",
        "--message",
        "I fixed it",
    ]);
    assert!(!direct.status.success());
    assert!(String::from_utf8_lossy(&direct.stderr).contains("verification-required"));
    for (index, id) in ["remediation", "rerun-1"].iter().enumerate() {
        begin(index, id, 2);
        value(submit("outcome", outcome(id, 2)));
    }
    begin(2, "rerun-2", 2);
    assert!(!submit("outcome", outcome("rerun-2", 2)).status.success());
    let mut verified = outcome("rerun-2", 2);
    verified["verifications"] = json!([{"finding_id":accepted["findings"][0]["id"],"expected_version":1,"message":"Fix verified"}]);
    let result = value(submit("outcome", verified));
    assert_eq!(result["findings"][0]["state"], "resolved");
    assert_eq!(result["blocking_findings"], json!([]));
    assert_eq!(value(submit("outcome", handback)), accepted);
    let final_checks = value(run(&["record", "check", "list", &task, "--json"]));
    assert!(
        final_checks
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["current_verdict"] == "pass")
    );
    let external = value(run(&[
        "record",
        "finding",
        "raise",
        &task,
        "--severity",
        "major",
        "--actor",
        "operator",
        "--message",
        "External review concern",
        "--json",
    ]));
    let external_id = external["id"].to_string();
    let assigned = value(run(&[
        "record",
        "finding",
        "assign-verification",
        &task,
        &external_id,
        "--check",
        &checks[2].id.to_string(),
        "--expected-version",
        "1",
        "--actor",
        "operator",
        "--message",
        "Review 2 owns verification",
        "--json",
    ]));
    assert_eq!(assigned["latest_activity"]["action"], "assign-verification");
    assert_eq!(assigned["check_id"], Value::Null);
    assert_eq!(assigned["verification_check_id"], checks[2].id);
    server.abort();
}
