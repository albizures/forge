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
async fn operators_assess_record_checks_with_versions_and_explicit_explanations() {
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
        "Deliver the work",
        "--json",
    ]));
    let task = task["id"].to_string();
    let checks = json(run(&["record", "check", "list", &task, "--json"]));
    let check = checks[0]["id"].to_string();
    assert_eq!(checks[0]["name"], "Implementation");
    assert!(checks[0]["latest_assessment"].is_null());
    json(run(&["record", "start", &task, "--json"]));
    assert!(!run(&["record", "done", &task]).status.success());
    let base = [
        "record",
        "check",
        "assess",
        &task,
        &check,
        "--verdict",
        "pass",
        "--actor",
        "operator",
        "--expected-version",
        "1",
    ];
    for invalid in [
        vec![],
        vec!["--message", " "],
        vec!["--message", "ok", "--message-file", "-"],
    ] {
        assert!(
            !run(&[base.as_slice(), invalid.as_slice()].concat())
                .status
                .success()
        );
    }
    let message = root.path().join("message.txt");
    std::fs::write(&message, "Verified deliverables").unwrap();
    let passed = json(run(&[
        base.as_slice(),
        &[
            "--message-file",
            message.to_str().unwrap(),
            "--dirty",
            "false",
            "--json",
        ],
    ]
    .concat()));
    assert_eq!(passed["version"], 2);
    assert_eq!(passed["latest_assessment"]["verdict"], "pass");
    assert_eq!(passed["latest_assessment"]["dirty"], false);
    let updates = run(&["record", "updates", &task]);
    assert!(
        String::from_utf8_lossy(&updates.stdout)
            .contains(&format!("Assessment {}", passed["latest_assessment"]["id"]))
    );
    assert!(
        !run(&[base.as_slice(), &["--message", "stale"]].concat())
            .status
            .success()
    );
    let page = json(run(&[
        "record", "check", "show", &task, &check, "--limit", "1", "--json",
    ]));
    assert_eq!(page["assessments"].as_array().unwrap().len(), 1);
    let text = run(&["record", "check", "list", &task]);
    assert!(String::from_utf8_lossy(&text.stdout).contains("pass"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args([
            "record",
            "check",
            "assess",
            &task,
            &check,
            "--expected-version",
            "2",
            "--verdict",
            "fail",
            "--actor",
            "operator",
            "--message-file",
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
        .write_all(b"A deliverable needs correction")
        .unwrap();
    assert_eq!(
        json(child.wait_with_output().unwrap())["latest_assessment"]["verdict"],
        "fail"
    );
    assert!(!run(&["record", "done", &task]).status.success());
    let passed = json(run(&[
        "record",
        "check",
        "assess",
        &task,
        &check,
        "--expected-version",
        "3",
        "--actor",
        "operator",
        "--verdict",
        "pass",
        "--message",
        "Correction verified",
        "--json",
    ]));
    let assessment = passed["latest_assessment"]["id"].to_string();
    let invalidate = [
        "record",
        "check",
        "invalidate",
        &task,
        &check,
        "--assessment",
        &assessment,
        "--expected-version",
        "4",
        "--actor",
        "operator",
    ];
    for extra in [
        vec![],
        vec!["--message", " "],
        vec!["--message", "why", "--message-file", "-"],
        vec!["--message", "why", "--revision", "short"],
    ] {
        assert!(
            !run(&[invalidate.as_slice(), &extra].concat())
                .status
                .success()
        );
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(invalidate)
        .args([
            "--message-file",
            "-",
            "--revision",
            &"a".repeat(40),
            "--dirty",
            "true",
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
        .write_all(b"Withdraw unsupported verdict")
        .unwrap();
    let pending = json(child.wait_with_output().unwrap());
    assert_eq!(pending["version"], 5);
    assert!(pending["current_verdict"].is_null());
    assert_eq!(pending["latest_assessment"]["verdict"], "pass");
    assert_eq!(
        pending["latest_assessment"]["invalidation"]["message"],
        "Withdraw unsupported verdict"
    );
    assert!(
        !run(&[invalidate.as_slice(), &["--message", "stale"]].concat())
            .status
            .success()
    );
    let repeated = run(&[
        "record",
        "check",
        "invalidate",
        &task,
        &check,
        "--assessment",
        &assessment,
        "--expected-version",
        "5",
        "--actor",
        "operator",
        "--message",
        "again",
    ]);
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("invalid-transition"));
    assert!(!run(&["record", "done", &task]).status.success());
    let summary = json(run(&["record", "show", &task, "--json"]));
    assert_eq!(summary["checks"][0]["version"], 5);
    assert!(summary["checks"][0]["current_verdict"].is_null());
    assert!(String::from_utf8_lossy(&run(&["record", "show", &task]).stdout).contains("[pending]"));
    let compared = json(run(&[
        "record",
        "check",
        "list",
        &task,
        "--compare-revision",
        &"a".repeat(40),
        "--json",
    ]));
    // Caller evidence survives on Projectless Tasks, but comparison is unknown.
    assert_eq!(
        compared[0]["latest_assessment"]["invalidation"]["comparison"],
        "unknown"
    );
    assert_eq!(
        compared[0]["latest_assessment"]["invalidation"]["dirty"],
        true
    );
    let page = json(run(&[
        "record",
        "check",
        "show",
        &task,
        &check,
        "--compare-revision",
        &"a".repeat(40),
        "--json",
    ]));
    assert_eq!(page["activities"].as_array().unwrap().len(), 4);
    assert_eq!(
        page["activities"][3]["invalidation"]["assessment_id"].to_string(),
        assessment
    );
    let text = run(&["record", "check", "show", &task, &check]);
    assert!(String::from_utf8_lossy(&text.stdout).contains("Withdraw unsupported verdict"));
    assert!(
        !run(&[
            "record",
            "check",
            "list",
            &task,
            "--compare-revision",
            "short"
        ])
        .status
        .success()
    );
    json(run(&[
        "record",
        "check",
        "assess",
        &task,
        &check,
        "--expected-version",
        "5",
        "--actor",
        "operator",
        "--verdict",
        "pass",
        "--message",
        "Reverified",
        "--json",
    ]));
    json(run(&["record", "done", &task, "--json"]));
    let frozen = run(&[
        "record",
        "check",
        "invalidate",
        &task,
        &check,
        "--assessment",
        &assessment,
        "--expected-version",
        "6",
        "--actor",
        "operator",
        "--message",
        "frozen",
    ]);
    assert!(String::from_utf8_lossy(&frozen.stderr).contains("record-done"));
    assert!(
        !run(&[
            "record",
            "check",
            "assess",
            &task,
            &check,
            "--expected-version",
            "2",
            "--actor",
            "operator",
            "--verdict",
            "fail",
            "--message",
            "frozen"
        ])
        .status
        .success()
    );
    server.abort();
}
