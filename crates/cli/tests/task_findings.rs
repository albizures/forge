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
async fn operator_findings_retain_explanations_and_block_completion_over_rpc() {
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
    let task = json(run(&["task", "open", "--body", "Deliver work", "--json"]))["id"].to_string();
    json(run(&["record", "start", &task, "--json"]));
    let check = json(run(&["task", "check", "list", &task, "--json"]))[0]["id"].to_string();
    let base = [
        "task",
        "finding",
        "raise",
        &task,
        "--severity",
        "critical",
        "--actor",
        "reviewer",
    ];
    for invalid in [
        vec![],
        vec!["--message", " "],
        vec!["--message", "why", "--message-file", "-"],
        vec!["--message", "why", "--revision", "short"],
        vec!["--message", "why", "--actor", " "],
    ] {
        assert!(!run(&[base.as_slice(), &invalid].concat()).status.success());
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(base)
        .args([
            "--check",
            &check,
            "--message-file",
            "-",
            "--dirty",
            "false",
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
        .write_all(b"Input validation missing")
        .unwrap();
    let raised = json(child.wait_with_output().unwrap());
    assert_eq!(raised["version"], 1);
    assert_eq!(raised["blocking"], true);
    assert_eq!(raised["raised"]["message"], "Input validation missing");
    assert_eq!(raised["raised"]["dirty"], false);
    let id = raised["id"].to_string();
    let blocked = run(&["record", "done", &task]);
    assert!(!blocked.status.success());
    let error = String::from_utf8_lossy(&blocked.stderr);
    assert!(error.contains("Check IDs:") && error.contains("Finding IDs:"));
    json(run(&[
        "task",
        "check",
        "assess",
        &task,
        &check,
        "--verdict",
        "pass",
        "--expected-version",
        "1",
        "--actor",
        "operator",
        "--message",
        "Verified implementation",
        "--json",
    ]));
    assert!(!run(&["record", "done", &task]).status.success());
    let list = json(run(&[
        "task", "finding", "list", &task, "--limit", "1", "--json",
    ]));
    assert_eq!(list["findings"][0], raised);
    let resolve = [
        "task",
        "finding",
        "resolve",
        &task,
        &id,
        "--expected-version",
        "1",
        "--actor",
        "verifier",
    ];
    assert!(!run(&resolve).status.success());
    let message = root.path().join("verification.md");
    std::fs::write(&message, "Verified validation and regression tests").unwrap();
    let resolved = json(run(&[
        resolve.as_slice(),
        &[
            "--message-file",
            message.to_str().unwrap(),
            "--revision",
            &"b".repeat(40),
            "--json",
        ],
    ]
    .concat()));
    assert_eq!(resolved["state"], "resolved");
    assert_eq!(resolved["version"], 2);
    assert_eq!(resolved["raised"], raised["raised"]);
    let stale = run(&[resolve.as_slice(), &["--message", "Stale"]].concat());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("stale-version"));
    let page = json(run(&[
        "task", "finding", "show", &task, &id, "--limit", "1", "--json",
    ]));
    assert_eq!(page["activities"].as_array().unwrap().len(), 1);
    let cursor = page["next_cursor"].to_string();
    let next = json(run(&[
        "task",
        "finding",
        "show",
        &task,
        &id,
        "--after",
        &cursor,
        "--limit",
        "1",
        "--compare-revision",
        &"b".repeat(40),
        "--json",
    ]));
    assert_eq!(
        next["activities"][0]["message"],
        "Verified validation and regression tests"
    );
    // Projectless evidence comparison stays unknown, even for identical SHAs.
    assert_eq!(next["activities"][0]["comparison"], "unknown");
    let text = run(&["task", "finding", "show", &task, &id]);
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(
        text.contains("Input validation missing")
            && text.contains("Verified validation and regression tests")
    );
    let updates = run(&["record", "updates", &task]);
    assert!(String::from_utf8_lossy(&updates.stdout).contains("Finding Activity"));
    json(run(&["record", "done", &task, "--json"]));
    let frozen = run(&[base.as_slice(), &["--message", "Too late"]].concat());
    assert!(String::from_utf8_lossy(&frozen.stderr).contains("record-done"));
    assert!(
        !run(&["--workspace", "missing", "task", "finding", "list", &task])
            .status
            .success()
    );
    server.abort();
}
