//! Full public declaration/inspection/removal contract for derived Stack topology.
#[path = "support/manual_cli.rs"]
mod manual_cli;
use forge_app::{
    Application, NewTypedRecord, Project, ProjectRepository, RecordRepository, RecordType,
    WorkspaceRepository,
};
use forge_rpc::{
    RecordApiClient, RelateRecordRequest, SelectedRecordRequest, StackSource, TaskStackAction,
    TaskStackRequest,
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
    let standalone_id = tasks[2].parse().unwrap();
    let standalone_base = "a".repeat(40);
    selected
        .set_task_base_commit(standalone_id, &standalone_base)
        .unwrap();
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
    let help = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(["help", "task", "stack", "--all"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for command in ["root", "show", "prepare", "publish", "publish-unchanged"] {
        assert!(help.contains(&format!("=== forge task stack {command} ===")));
    }
    assert!(!help.contains("=== forge task stack after ==="));
    let client = jsonrpsee::http_client::HttpClientBuilder::default()
        .build(&endpoint)
        .unwrap();
    let from_id = tasks[1].parse().unwrap();
    let to_id = tasks[0].parse().unwrap();
    for kind in ["depends-on", "ordering-only-depends-on"] {
        for action in ["relate", "unrelate"] {
            let output = run(&[
                "record", action, &tasks[1], "--kind", kind, "--to", &tasks[0],
            ]);
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            let diagnostic = String::from_utf8(output.stderr).unwrap();
            assert!(
                diagnostic.contains("waits-for") && diagnostic.contains("builds-on"),
                "{diagnostic}"
            );
            let request = SelectedRecordRequest::selected(
                "one",
                RelateRecordRequest {
                    from_id,
                    to_id,
                    kind: kind.into(),
                },
            );
            let error = if action == "relate" {
                client.relate(request).await
            } else {
                client.unrelate(request).await
            }
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("completion only") && error.contains("builds-on"),
                "{error}"
            );
        }
    }
    let legacy = run(&[
        "task",
        "stack",
        "after",
        &tasks[1],
        "--predecessor",
        &tasks[0],
    ]);
    assert!(!legacy.status.success());
    assert!(
        String::from_utf8(legacy.stderr)
            .unwrap()
            .contains("--kind builds-on")
    );
    let error = client
        .task_stack(SelectedRecordRequest::selected(
            "one",
            TaskStackRequest {
                record_id: from_id,
                action: TaskStackAction::Configure {
                    source: StackSource::Predecessor { record_id: to_id },
                },
            },
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("record relate --kind builds-on"), "{error}");
    assert!(
        json(&["record", "show", &tasks[1], "--json"])["relationships"]
            .as_array()
            .unwrap()
            .is_empty()
    );
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
    // A standalone Task and its retained Check evidence must not acquire a root
    // or change when the Project's policy is configured or reset.
    json(&[
        "record",
        "execution-mode",
        &tasks[2],
        "manual",
        "--expected",
        "dispatched",
        "--actor",
        "human",
        "--message",
        "Manual delivery",
        "--json",
    ]);
    json(&["record", "start", &tasks[2], "--json"]);
    let checks = json(&["record", "check", "list", &tasks[2], "--json"]);
    let check = checks[0]["id"].as_u64().unwrap();
    manual_cli::pass(run, &tasks[2], check, &root);
    let check_id = check.to_string();
    let root_before = json(&["task", "stack", "show", &tasks[0], "--json"]);
    let standalone_before = run(&["task", "stack", "show", &tasks[2], "--json"]);
    let record_before = json(&["record", "show", &tasks[2], "--json"]);
    let checks_before = json(&["record", "check", "list", &tasks[2], "--json"]);
    let evidence_before = json(&["record", "check", "show", &tasks[2], &check_id, "--json"]);
    assert!(
        evidence_before
            .to_string()
            .contains("urn:fixture:deliverable")
    );
    for action in ["set", "reset"] {
        let mut args = vec![
            "project",
            "base-branch-policy",
            action,
            "--project",
            "one/api",
            "--json",
        ];
        if action == "set" {
            args.push("default-branch");
        }
        json(&args);
        assert_eq!(
            json(&["task", "stack", "show", &tasks[0], "--json"]),
            root_before
        );
        let standalone_after = run(&["task", "stack", "show", &tasks[2], "--json"]);
        assert_eq!(standalone_after.status, standalone_before.status);
        assert_eq!(standalone_after.stdout, standalone_before.stdout);
        assert_eq!(standalone_after.stderr, standalone_before.stderr);
        assert_eq!(
            json(&["record", "show", &tasks[2], "--json"]),
            record_before
        );
        assert_eq!(
            json(&["record", "check", "list", &tasks[2], "--json"]),
            checks_before
        );
        assert_eq!(
            json(&["record", "check", "show", &tasks[2], &check_id, "--json"]),
            evidence_before
        );
        assert_eq!(
            selected.task_base_commit(standalone_id).unwrap(),
            Some(standalone_base.clone())
        );
    }
    let stack = json(&["task", "stack", "show", &tasks[1], "--json"]);
    assert_eq!(stack["predecessor"], tasks[0].parse::<u64>().unwrap());
    assert_eq!(stack["destination"]["branch"], "release/2.x");
    let record = json(&["record", "show", &tasks[1], "--json"]);
    assert_eq!(record["relationships"][0]["kind"], "builds-on");
    assert!(record["relationships"][0].get("ordering_only").is_none());
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
