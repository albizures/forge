use forge_app::{
    Application, NewTypedRecord, Project, ProjectRepository, RecordRepository, RecordType,
    WorkspaceRepository,
};
use forge_server::{Daemon, GitInspector, router};
use forge_store::SqliteProjectRepository;
use jsonrpsee::core::client::ClientT;
use serde_json::{Value, json};
use std::process::Command;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn cli_explicit_mode_selection_is_scoped_recorded_and_never_implicit_in_start() {
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
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver report".into(),
            project_slugs: vec!["test/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let id = task.id.to_string();
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
    let start = run(&["record", "start", &id]);
    assert!(!start.status.success());
    assert!(String::from_utf8_lossy(&start.stderr).contains("dispatched-start-forbidden"));
    let shown: Value =
        serde_json::from_slice(&run(&["record", "show", &id, "--json"]).stdout).unwrap();
    assert_eq!(shown["execution_mode"], "dispatched");
    assert_eq!(shown["status"], "open");
    let changed = run(&[
        "record",
        "execution-mode",
        &id,
        "manual",
        "--expected",
        "dispatched",
        "--actor",
        "human",
        "--message",
        "Manual delivery",
        "--json",
    ]);
    assert!(
        changed.status.success(),
        "{}",
        String::from_utf8_lossy(&changed.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&changed.stdout).unwrap()["execution_mode"],
        "manual"
    );
    let start = run(&["record", "start", &id]);
    assert!(
        start.status.success(),
        "{}",
        String::from_utf8_lossy(&start.stderr)
    );
    assert!(String::from_utf8_lossy(&start.stdout).contains("manual"));
    let invalid = run(&[
        "record",
        "execution-mode",
        &id,
        "dispatched",
        "--expected",
        "manual",
        "--actor",
        "human",
        "--message",
        "Missing profile",
    ]);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("invalid-check-profile"));
    let updates: Value =
        serde_json::from_slice(&run(&["record", "updates", &id, "--json"]).stdout).unwrap();
    let update = updates["updates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["kind"] == "execution-mode-changed")
        .unwrap();
    assert_eq!(update["from_mode"], "dispatched");
    assert_eq!(update["to_mode"], "manual");
    assert_eq!(update["mode_actor"], "human");
    assert_eq!(update["mode_message"], "Manual delivery");
    let human = run(&["record", "updates", &id]);
    assert!(
        String::from_utf8_lossy(&human.stdout)
            .contains("dispatched -> manual by human: Manual delivery")
    );
    // The selected RPC boundary rejects invalid enum values and unknown inputs before writes.
    let client = jsonrpsee::http_client::HttpClientBuilder::default()
        .build(&endpoint)
        .unwrap();
    let unchanged: Value = client.request("record_executionModeSet", jsonrpsee::rpc_params![json!({"workspace":"test", "id":task.id, "expected":"manual", "mode":"manual", "actor":"human", "message":"No change"})]).await.unwrap();
    assert_eq!(unchanged["execution_mode"], "manual");
    for extra in [
        json!({"mode":"automatic"}),
        json!({"mode":"manual", "profile_revision":99}),
    ] {
        let mut request = json!({"workspace":"test", "id":task.id, "expected":"manual", "actor":"human", "message":"Invalid input"});
        request
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(
            client
                .request::<Value, _>("record_executionModeSet", jsonrpsee::rpc_params![request])
                .await
                .is_err()
        );
    }
    let restarted = SqliteProjectRepository::open(&database).unwrap();
    assert_eq!(
        restarted.show_record(task.id).unwrap().execution_mode,
        forge_app::ExecutionMode::Manual
    );
    assert_eq!(restarted.list_record_updates(task.id).unwrap().len(), 3);
    server.abort();
}
