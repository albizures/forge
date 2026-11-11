use forge_app::{Application, Project, ProjectRepository, WorkspaceRepository};
use forge_rpc::{
    CheckTemplateSetRequest, CheckTemplateShowRequest, RecordApiClient, SelectedRecordRequest,
    TemplateReplacement,
};
use forge_server::{Daemon, GitInspector, router};
use forge_store::SqliteProjectRepository;
use jsonrpsee::http_client::HttpClientBuilder;
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
async fn project_templates_round_trip_through_cli_and_typed_rpc_with_workspace_confinement() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("forge.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    for workspace in ["one", "two"] {
        let path = root.path().join(workspace);
        std::fs::create_dir_all(&path).unwrap();
        for args in [
            vec!["init", "--quiet"],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.test",
                "commit",
                "--allow-empty",
                "--quiet",
                "-m",
                "Initial",
            ],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&path)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        store.create_workspace(workspace, workspace, &path).unwrap();
        store
            .register(&Project {
                reference: format!("{workspace}/api"),
                workspace_id: workspace.into(),
                slug: "api".into(),
                project_root: path.canonicalize().unwrap(),
                origin_url: "https://example.test/repo.git".into(),
            })
            .unwrap();
    }
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
    let cwd = root.path().join("one");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(args)
            .args(["--server-url", &endpoint])
            .current_dir(&cwd)
            .output()
            .unwrap()
    };
    let initial = json(run(&["project", "check-template", "show", "api", "--json"]));
    assert_eq!(initial["version"], 0);
    assert_eq!(initial["criteria"], serde_json::json!([]));
    let task_before = json(run(&[
        "task",
        "open",
        "--project",
        "api",
        "--body",
        "Old task",
        "--json",
    ]));
    let file = root.path().join("criteria.json");
    std::fs::write(&file, r#"[{"name":" Review ","description":" Review the deliverables "},{"name":"Tests","description":"Relevant checks pass"}]"#).unwrap();
    let file = file.to_str().unwrap();
    let base = [
        "project",
        "check-template",
        "set",
        "api",
        "--file",
        file,
        "--actor",
        "operator",
        "--expected-version",
        "0",
    ];
    for invalid in [
        vec![],
        vec!["--message", " "],
        vec!["--message", "why", "--message-file", "-"],
    ] {
        assert!(
            !run(&[base.as_slice(), invalid.as_slice()].concat())
                .status
                .success()
        );
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(base)
        .args(["--message-file", "-", "--json", "--server-url", &endpoint])
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"Require independent review and verification")
        .unwrap();
    let template = json(child.wait_with_output().unwrap());
    assert_eq!(template["version"], 1);
    assert_eq!(template["criteria"][0]["name"], "Review");
    let stale = run(&[base.as_slice(), &["--message", "stale"]].concat());
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("stale-version"));
    let text = run(&["project", "check-template", "show", "api"]);
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("version 1") && text.contains("operator") && text.contains("Review"));
    let old_checks = json(run(&[
        "task",
        "check",
        "list",
        &task_before["id"].to_string(),
        "--json",
    ]));
    assert_eq!(old_checks.as_array().unwrap().len(), 1);
    let task = json(run(&[
        "task",
        "open",
        "--project",
        "api",
        "--body",
        "New task",
        "--json",
    ]));
    let checks = json(run(&[
        "task",
        "check",
        "list",
        &task["id"].to_string(),
        "--json",
    ]));
    assert_eq!(checks.as_array().unwrap().len(), 3);
    assert_eq!(checks[1]["name"], "Review");
    assert_eq!(checks[2]["name"], "Tests");
    assert!(
        !run(&[
            "project",
            "check-template",
            "show",
            "one/api",
            "--workspace",
            "two"
        ])
        .status
        .success()
    );
    assert_eq!(
        json(run(&[
            "project",
            "check-template",
            "show",
            "two/api",
            "--json"
        ]))["version"],
        0
    );
    let client = HttpClientBuilder::default().build(&endpoint).unwrap();
    let page = client
        .project_check_template_show(SelectedRecordRequest::selected(
            "one",
            CheckTemplateShowRequest {
                project: "api".into(),
                after: None,
                limit: 1,
            },
        ))
        .await
        .unwrap();
    assert_eq!(page.history.len(), 1);
    assert_eq!(
        page.history[0].message,
        "Require independent review and verification"
    );
    assert_eq!(page.history[0].criteria, page.criteria);
    assert!(
        client
            .project_check_template_set(SelectedRecordRequest::selected(
                "two",
                CheckTemplateSetRequest {
                    project: "one/api".into(),
                    replacement: TemplateReplacement {
                        expected_version: 1,
                        criteria: vec![],
                        actor: "operator".into(),
                        message: "Forbidden".into()
                    }
                }
            ))
            .await
            .is_err()
    );
    let cleared = client
        .project_check_template_set(SelectedRecordRequest::selected(
            "one",
            CheckTemplateSetRequest {
                project: "api".into(),
                replacement: TemplateReplacement {
                    expected_version: 1,
                    criteria: vec![],
                    actor: "operator".into(),
                    message: "Clear extras".into(),
                },
            },
        ))
        .await
        .unwrap();
    assert_eq!(cleared.version, 2);
    let first = client
        .project_check_template_show(SelectedRecordRequest::selected(
            "one",
            CheckTemplateShowRequest {
                project: "one/api".into(),
                after: None,
                limit: 1,
            },
        ))
        .await
        .unwrap();
    let next = client
        .project_check_template_show(SelectedRecordRequest::selected(
            "one",
            CheckTemplateShowRequest {
                project: "api".into(),
                after: first.next_cursor,
                limit: 1,
            },
        ))
        .await
        .unwrap();
    assert_eq!(next.history[0].message, "Clear extras");
    assert!(next.history[0].criteria.is_empty());
    assert!(next.next_cursor.is_none());
    let message = root.path().join("why.txt");
    std::fs::write(&message, "Restore criteria").unwrap();
    assert_eq!(
        json(run(&[
            "project",
            "check-template",
            "set",
            "api",
            "--file",
            file,
            "--actor",
            "operator",
            "--expected-version",
            "2",
            "--message-file",
            message.to_str().unwrap(),
            "--json"
        ]))["version"],
        3
    );
    std::fs::write(
        file,
        r#"[{"name":"Implementation","description":"duplicate baseline"}]"#,
    )
    .unwrap();
    assert!(
        !run(&[
            "project",
            "check-template",
            "set",
            "api",
            "--file",
            file,
            "--actor",
            "operator",
            "--expected-version",
            "3",
            "--message",
            "Invalid"
        ])
        .status
        .success()
    );
    assert_eq!(
        json(run(&["project", "check-template", "show", "api", "--json"]))["history"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    server.abort();
}
