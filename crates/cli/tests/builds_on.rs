//! Full public declaration/inspection/removal contract for derived Stack topology.
use forge_app::{
    Application, NewTypedRecord, Project, ProjectRepository, RecordRepository, RecordType,
    WorkspaceRepository,
};
use forge_server::{Daemon, GitInspector, router};
use forge_store::SqliteProjectRepository;
use std::process::Command;

// Keep the isolated server fixture and one public round-trip scenario together.
#[allow(clippy::too_many_lines)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn builds_on_cli_declares_prerequisite_and_inspected_predecessor_in_one_write() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let database = root.join("forge.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("one", "One", &root).unwrap();
    store
        .register(&Project {
            reference: "one/api".into(),
            workspace_id: "one".into(),
            slug: "api".into(),
            project_root: root.clone(),
            origin_url: "https://github.com/acme/widgets.git".into(),
        })
        .unwrap();
    let selected = store.in_record_workspace("one").unwrap();
    let tasks: Vec<_> = (0..3)
        .map(|_| {
            selected
                .open_typed_record(&NewTypedRecord {
                    record_type: RecordType::Task,
                    body: "Planned work".into(),
                    project_slugs: vec!["one/api".into()],
                    generated_by_ids: vec![],
                })
                .unwrap()
                .id
                .to_string()
        })
        .collect();
    let app = Application::new(
        Box::new(SqliteProjectRepository::open(&database).unwrap()),
        Box::new(GitInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(&database).unwrap()));
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
            .args(["--server-url", &endpoint, "--workspace", "one"])
            .current_dir(&root)
            .output()
            .unwrap()
    };
    let json = |args: &[&str]| {
        let output = run(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    // Missing execution Checks/configuration do not prevent assembling an Open plan.
    json(&[
        "record",
        "relate",
        &tasks[1],
        "--kind",
        "builds-on",
        "--to",
        &tasks[0],
        "--json",
    ]);
    json(&[
        "task",
        "stack",
        "root",
        &tasks[0],
        "--repository",
        "acme/widgets",
        "--remote",
        "origin",
        "--base",
        "release/2.x",
        "--json",
    ]);
    let stack = json(&["task", "stack", "show", &tasks[1], "--json"]);
    assert_eq!(stack["predecessor"], tasks[0].parse::<u64>().unwrap());
    assert_eq!(stack["destination"]["branch"], "release/2.x");
    let record = json(&["record", "show", &tasks[1], "--json"]);
    assert_eq!(record["relationships"][0]["kind"], "builds-on");
    let human = run(&["record", "show", &tasks[1]]);
    assert!(human.status.success());
    assert!(
        String::from_utf8(human.stdout)
            .unwrap()
            .contains("builds-on")
    );
    for args in [
        vec![
            "record",
            "relate",
            &tasks[2],
            "--kind",
            "builds-on",
            "--to",
            &tasks[0],
        ],
        vec![
            "record",
            "relate",
            &tasks[1],
            "--kind",
            "waits-for",
            "--to",
            &tasks[0],
        ],
        vec![
            "task",
            "stack",
            "root",
            &tasks[1],
            "--repository",
            "acme/widgets",
            "--remote",
            "origin",
            "--base",
            "main",
        ],
        vec!["record", "start", &tasks[1]],
    ] {
        assert!(!run(&args).status.success());
    }
    json(&[
        "record",
        "unrelate",
        &tasks[1],
        "--kind",
        "builds-on",
        "--to",
        &tasks[0],
        "--json",
    ]);
    assert!(json(&["task", "stack", "show", &tasks[1], "--json"]).is_null());
    json(&[
        "record",
        "relate",
        &tasks[1],
        "--kind",
        "waits-for",
        "--to",
        &tasks[0],
        "--json",
    ]);
    assert!(json(&["task", "stack", "show", &tasks[1], "--json"]).is_null());
    server.abort();
}
