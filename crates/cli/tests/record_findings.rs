#[path = "support/manual_cli.rs"]
mod manual_cli;
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
    let check = json(run(&["record", "check", "list", &task, "--json"]))[0]["id"].to_string();
    let base = [
        "record",
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
            // Unlinked operator Finding: linked reviewer verification is outcome-bound.
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
    manual_cli::pass(&run, &task, check.parse().unwrap(), root.path());
    assert!(!run(&["record", "done", &task]).status.success());
    let list = json(run(&[
        "record", "finding", "list", &task, "--limit", "1", "--json",
    ]));
    assert_eq!(list["findings"][0], raised);
    let resolve = [
        "record",
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
        "record", "finding", "show", &task, &id, "--limit", "1", "--json",
    ]));
    assert_eq!(page["activities"].as_array().unwrap().len(), 1);
    let cursor = page["next_cursor"].to_string();
    let next = json(run(&[
        "record",
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
    let text = run(&["record", "finding", "show", &task, &id]);
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(
        text.contains("Input validation missing")
            && text.contains("Verified validation and regression tests")
    );
    let updates = run(&["record", "updates", &task]);
    assert!(String::from_utf8_lossy(&updates.stdout).contains("Finding Activity"));
    let change = |action: &str, version: &str, extra: &[&str]| {
        run(&[
            &[
                "record",
                "finding",
                action,
                &task,
                &id,
                "--expected-version",
                version,
                "--actor",
                "second-reviewer",
                "--message",
                "Review cycle explanation",
                "--json",
            ][..],
            extra,
        ]
        .concat())
    };
    assert_eq!(json(change("note", "2", &[]))["state"], "resolved");
    let invalid = change("respond", "3", &["--response", "fixed"]);
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("invalid-transition"));
    assert_eq!(json(change("reopen", "3", &[]))["state"], "open");
    let response = json(change("respond", "4", &["--response", "fixed"]));
    assert_eq!(response["state"], "awaiting-review");
    assert_eq!(response["latest_activity"]["response"], "fixed");
    let response = json(change("respond", "5", &["--response", "rejected"]));
    assert_eq!(response["blocking"], true);
    assert_eq!(response["state"], "awaiting-review");
    assert!(!run(&["record", "done", &task]).status.success());
    let amendment = json(change("amend", "6", &["--severity", "minor"]));
    assert_eq!(amendment["blocking"], true);
    assert_eq!(
        amendment["latest_activity"]["amendment"]["old_severity"],
        "critical"
    );
    assert_eq!(
        amendment["latest_activity"]["amendment"]["new_severity"],
        "minor"
    );
    assert!(!change("amend", "7", &[]).status.success());
    assert!(!change("respond", "7", &[]).status.success());
    assert!(
        !change("note", "7", &["--message-file", "-"])
            .status
            .success()
    );
    assert_eq!(
        json(change("amend", "7", &["--blocking", "false"]))["blocking"],
        false
    );
    let stale = change("note", "7", &[]);
    assert!(String::from_utf8_lossy(&stale.stderr).contains("stale-version"));
    json(run(&["record", "done", &task, "--json"]));
    let frozen_amend = change("amend", "8", &["--blocking", "true"]);
    assert!(String::from_utf8_lossy(&frozen_amend.stderr).contains("record-done"));
    assert_eq!(json(change("dismiss", "8", &[]))["state"], "dismissed");
    assert_eq!(json(change("note", "9", &[]))["state"], "dismissed");
    assert_eq!(json(change("reopen", "10", &[]))["state"], "open");
    assert_eq!(
        json(change("respond", "11", &["--response", "rejected"]))["state"],
        "awaiting-review"
    );
    let page = json(run(&["record", "finding", "show", &task, &id, "--json"]));
    assert_eq!(page["finding"]["raised"], raised["raised"]);
    assert_eq!(page["activities"].as_array().unwrap().len(), 12);
    let text = run(&["record", "finding", "show", &task, &id]);
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(text.contains("Rejected") && text.contains("old_severity: Critical"));
    let frozen = run(&[base.as_slice(), &["--message", "Too late"]].concat());
    assert!(String::from_utf8_lossy(&frozen.stderr).contains("record-done"));
    assert!(
        !run(&["--workspace", "missing", "record", "finding", "list", &task])
            .status
            .success()
    );
    server.abort();
}
