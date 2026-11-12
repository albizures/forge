use forge_app::{Application, Project, ProjectRepository, RecordRepository, WorkspaceRepository};
use forge_rpc::{RecordApiClient, RecordIdRequest, RecordSelection, SelectedRecordRequest};
use forge_server::{Daemon, GitInspector, router};
use forge_store::SqliteProjectRepository;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forge-cli-record-workspaces-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn json(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(&String::from_utf8(output.stdout).unwrap()).unwrap()
}
fn rejected(output: Output) {
    assert!(
        !output.status.success(),
        "unexpected success: {}",
        String::from_utf8(output.stdout).unwrap()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn record_commands_and_rpc_are_confined_by_explicit_or_client_cwd_selection() {
    let fixture = Fixture::new();
    let database = fixture.0.join("forge.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    for workspace in ["one", "two"] {
        let root = fixture.0.join(workspace);
        std::fs::create_dir_all(&root).unwrap();
        store.create_workspace(workspace, workspace, &root).unwrap();
        for slug in ["api", "web"] {
            let root = root.join(slug);
            std::fs::create_dir_all(&root).unwrap();
            store
                .register(&Project {
                    reference: format!("{workspace}/{slug}"),
                    workspace_id: workspace.into(),
                    slug: slug.into(),
                    project_root: root.canonicalize().unwrap(),
                    origin_url: "https://example.test/project.git".into(),
                })
                .unwrap();
        }
    }
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
    let run = |args: &[&str], cwd: &Path| {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(args)
            .args(["--server-url", &endpoint])
            .current_dir(cwd)
            .output()
            .unwrap()
    };
    let outside = &fixture.0;
    let one = fixture.0.join("one");
    let two = fixture.0.join("two");
    let map = json(run(
        &[
            "wayfinder",
            "open",
            "--body",
            "local map",
            "--workspace",
            "one",
            "--json",
        ],
        outside,
    ));
    assert_eq!(map["workspace_id"], "one");
    assert_eq!(map["project_scope"]["project_slugs"], serde_json::json!([]));
    let map_id = map["id"].to_string();
    let initiative = json(run(
        &["initiative", "open", "--body", "local initiative", "--json"],
        &one,
    ));
    let initiative_id = initiative["id"].to_string();
    json(run(
        &[
            "record",
            "relate",
            &map_id,
            "--kind",
            "initiative",
            "--to",
            &initiative_id,
            "--json",
        ],
        &one,
    ));
    let foreign_initiative = json(run(
        &[
            "initiative",
            "open",
            "--body",
            "foreign initiative",
            "--json",
        ],
        &two,
    ));
    rejected(run(
        &[
            "record",
            "relate",
            &map_id,
            "--kind",
            "initiative",
            "--to",
            &foreign_initiative["id"].to_string(),
            "--workspace",
            "one",
        ],
        outside,
    ));
    let map = json(run(&["record", "show", &map_id, "--json"], &one));
    assert_eq!(map["relationships"].as_array().unwrap().len(), 1);
    let task = json(run(
        &[
            "task",
            "under-wayfinder",
            "--wayfinder",
            &map_id,
            "--body",
            "local task",
            "--workspace",
            "one",
            "--json",
        ],
        outside,
    ));
    let task_id = task["id"].to_string();
    let grilling = json(run(
        &[
            "grilling",
            "open",
            "--wayfinder",
            &map_id,
            "--body",
            "local decision",
            "--json",
        ],
        &one,
    ));
    let grilling_id = grilling["id"].to_string();
    json(run(
        &[
            "record",
            "relate",
            &task_id,
            "--kind",
            "depends-on",
            "--to",
            &grilling_id,
            "--json",
        ],
        &one,
    ));
    let foreign_task = json(run(
        &["task", "open", "--body", "foreign task", "--json"],
        &two,
    ));
    let foreign_task_id = foreign_task["id"].to_string();
    for kind in ["depends-on", "generated-by"] {
        rejected(run(
            &[
                "record",
                "relate",
                &task_id,
                "--kind",
                kind,
                "--to",
                &foreign_task_id,
                "--workspace",
                "one",
            ],
            outside,
        ));
    }
    rejected(run(
        &[
            "task",
            "under-wayfinder",
            "--wayfinder",
            &map_id,
            "--body",
            "wrong parent",
            "--workspace",
            "two",
        ],
        outside,
    ));
    rejected(run(
        &[
            "grilling",
            "open",
            "--wayfinder",
            &map_id,
            "--body",
            "wrong parent",
            "--workspace",
            "two",
        ],
        outside,
    ));
    let projected = json(run(
        &[
            "wayfinder",
            "open",
            "--body",
            "multi project",
            "--project",
            "one/api",
            "--project",
            "one/web",
            "--json",
        ],
        outside,
    ));
    assert_eq!(
        projected["project_scope"]["project_slugs"],
        serde_json::json!(["one/api", "one/web"])
    );
    rejected(run(
        &[
            "wayfinder",
            "open",
            "--body",
            "mixed",
            "--project",
            "one/api",
            "--project",
            "two/api",
        ],
        outside,
    ));
    rejected(run(
        &[
            "spec",
            "open",
            "--body",
            "conflict",
            "--workspace",
            "two",
            "--project",
            "one/api",
        ],
        outside,
    ));
    let spec = json(run(
        &[
            "spec",
            "open",
            "--body",
            "qualified selection",
            "--project",
            "one/api",
            "--json",
        ],
        outside,
    ));
    assert_eq!(spec["workspace_id"], "one");
    rejected(run(
        &[
            "task",
            "under-spec",
            "--spec",
            &spec["id"].to_string(),
            "--body",
            "wrong",
            "--workspace",
            "two",
        ],
        outside,
    ));
    let list_before = json(run(
        &["record", "list", "--workspace", "one", "--json"],
        outside,
    ));
    rejected(run(
        &[
            "record",
            "open",
            "--body",
            "wrong generator",
            "--no-projects",
            "--workspace",
            "one",
            "--generated-by",
            &foreign_task_id,
        ],
        outside,
    ));
    assert_eq!(
        json(run(
            &["record", "list", "--workspace", "one", "--json"],
            outside
        )),
        list_before
    );
    for args in [
        vec!["record", "show", &task_id],
        vec!["record", "edit", &task_id, "--body", "bad"],
        vec!["record", "start", &task_id],
        vec!["record", "done", &task_id],
        vec!["record", "comment", &task_id, "--message", "bad"],
        vec!["record", "comments", &task_id],
        vec!["record", "updates", &task_id],
    ] {
        rejected(run(&args, &two));
    }
    rejected(run(&["record", "show", &task_id], outside));
    rejected(run(&["record", "list"], outside));
    rejected(run(&["ready"], outside));
    rejected(run(&["task", "open", "--body", "unselected"], outside));
    let selected = json(run(
        &["record", "show", &task_id, "--workspace", "one", "--json"],
        &two,
    ));
    assert_eq!(selected["workspace_id"], "one");
    let ready = json(run(
        &[
            "ready",
            "--workspace",
            "one",
            "--planning",
            "--include-hitl",
            "--blocked",
            "--json",
        ],
        outside,
    ));
    assert!(
        ready["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["record"]["workspace_id"] == "one")
    );
    assert!(
        ready["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["record"]["id"] == task["id"]
                && entry["blocked_by"] == serde_json::json!([grilling["id"]]))
    );
    let two_list = json(run(&["record", "list", "--json"], &two));
    assert_eq!(two_list["records"].as_array().unwrap().len(), 2);
    json(run(
        &["record", "comment", &task_id, "--message", "kept", "--json"],
        &one,
    ));
    let comments = json(run(&["record", "comments", &task_id, "--json"], &one));
    let comment_id = comments["comments"][0]["id"].to_string();
    rejected(run(
        &["record", "comment", "delete", &task_id, &comment_id],
        &two,
    ));
    json(run(
        &[
            "record",
            "unrelate",
            &task_id,
            "--kind",
            "depends-on",
            "--to",
            &grilling_id,
            "--json",
        ],
        &one,
    ));
    json(run(&["record", "start", &task_id, "--json"], &one));
    let checks = json(run(&["record", "check", "list", &task_id, "--json"], &one));
    rejected(run(&["record", "check", "list", &task_id, "--json"], &two));
    for check in checks.as_array().unwrap() {
        rejected(run(
            &[
                "record",
                "check",
                "show",
                &task_id,
                &check["id"].to_string(),
                "--json",
            ],
            &two,
        ));
        rejected(run(
            &[
                "record",
                "check",
                "assess",
                &task_id,
                &check["id"].to_string(),
                "--expected-version",
                &check["version"].to_string(),
                "--verdict",
                "pass",
                "--actor",
                "foreign operator",
                "--message",
                "Forbidden",
                "--json",
            ],
            &two,
        ));
        json(run(
            &[
                "record",
                "check",
                "assess",
                &task_id,
                &check["id"].to_string(),
                "--expected-version",
                &check["version"].to_string(),
                "--verdict",
                "pass",
                "--actor",
                "test operator",
                "--message",
                "Fixture deliverables verified",
                "--json",
            ],
            &one,
        ));
    }
    json(run(
        &[
            "record",
            "done",
            &task_id,
            "--resolution",
            "verified",
            "--json",
        ],
        &one,
    ));

    // The typed RPC contract also rejects missing/relative client context and explicit conflicts.
    let client = jsonrpsee::http_client::HttpClientBuilder::default()
        .build(&endpoint)
        .unwrap();
    for selection in [
        RecordSelection::default(),
        RecordSelection {
            workspace: None,
            cwd: Some("relative".into()),
        },
        RecordSelection {
            workspace: Some("two".into()),
            cwd: None,
        },
    ] {
        assert!(
            client
                .show(SelectedRecordRequest {
                    selection,
                    record: RecordIdRequest {
                        id: task["id"].as_u64().unwrap()
                    }
                })
                .await
                .is_err()
        );
    }
    let before = json(run(
        &["record", "list", "--workspace", "one", "--json"],
        outside,
    ));
    assert!(
        client
            .open_spec(SelectedRecordRequest::selected(
                "one",
                forge_rpc::OpenSpecRequest {
                    body: "API conflict".into(),
                    project_slug: "two/api".into(),
                    generated_by_ids: vec![],
                }
            ))
            .await
            .is_err()
    );
    for projects in [
        vec!["one/api".into(), "two/api".into()],
        vec!["two/api".into()],
    ] {
        assert!(
            client
                .open_initiative(SelectedRecordRequest::selected(
                    "one",
                    forge_rpc::OpenRecordRequest {
                        body: "API mixed ownership".into(),
                        project_scope: forge_rpc::ProjectScopeRequest {
                            project_slugs: projects
                        },
                        generated_by_ids: vec![],
                    }
                ))
                .await
                .is_err()
        );
    }
    assert_eq!(
        json(run(
            &["record", "list", "--workspace", "one", "--json"],
            outside
        )),
        before
    );
    let selected = client
        .show(SelectedRecordRequest::selected(
            "one",
            RecordIdRequest {
                id: task["id"].as_u64().unwrap(),
            },
        ))
        .await
        .unwrap();
    assert_eq!(selected.workspace_id, "one");
    server.abort();
    let reopened = SqliteProjectRepository::open(&database).unwrap();
    assert_eq!(
        reopened
            .show_record(task["id"].as_u64().unwrap())
            .unwrap()
            .workspace_id,
        "one"
    );
}
