use axum::{Json, Router, routing::post};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn finding() -> serde_json::Value {
    let activity = serde_json::json!({"id":9,"finding_id":3,"action":"raise","response":null,
        "amendment":null,"actor":"assigned","assignment_id":"assigned","message":"Review issue",
        "revision":null,"dirty":false,"created_at_millis":123,"comparison":"unknown"});
    serde_json::json!({"id":3,"record_id":7,"version":2,"severity":"minor","blocking":false,
        "check_id":null,"assessment_id":null,"state":"open","raised":activity,
        "latest_activity":activity,"project":null})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn assignment_finding_cli_uses_own_task_routes_and_never_exposes_credentials() {
    let app = Router::new().route("/rpc", post(|Json(r): Json<serde_json::Value>| async move {
        let p = &r["params"][0];
        assert_eq!(p["assignmentId"], "assigned");
        assert_eq!(p["secret"], "sentinel-secret");
        assert!(p.get("task").is_none());
        assert!(p.get("actor").is_none());
        let result = match r["method"].as_str().unwrap() {
            "assignment_findingList" => {
                assert_eq!(p["limit"], 1);
                assert_eq!(p["after"], 2);
                assert_eq!(p["compareRevision"], "a".repeat(40));
                serde_json::json!({"findings":[finding()],"next_cursor":3})
            }
            "assignment_findingShow" => {
                assert_eq!(p["finding"], 3);
                assert_eq!(p["after"], 8);
                assert_eq!(p["limit"], 1);
                serde_json::json!({"finding":finding(),"activities":[finding()["raised"]],"next_cursor":9})
            }
            "assignment_findingRaise" => {
                assert_eq!(p["severity"], "minor");
                assert_eq!(p["blocking"], false);
                assert_eq!(p["checkId"], 4);
                assert_eq!(p["assessmentId"], 5);
                assert_eq!(p["message"], "Review from stdin");
                assert_eq!(p["dirty"], false);
                finding()
            }
            "assignment_findingMutate" => {
                assert_eq!(p["finding"], 3);
                assert_eq!(p["expectedVersion"], 2);
                assert_eq!(p["message"], "Review action");
                assert_eq!(p["dirty"], true);
                match p["change"]["action"].as_str().unwrap() {
                    "respond" => assert_eq!(p["change"]["response"], "rejected"),
                    "amend" => {
                        assert_eq!(p["change"]["severity"], "critical");
                        assert_eq!(p["change"]["blocking"], false);
                    }
                    "note" => return Json(serde_json::json!({"jsonrpc":"2.0","id":r["id"],"error":{
                        "code":-32001,"message":"sentinel-secret must not leak","data":"stale-version"}})),
                    "resolve" | "dismiss" | "reopen" => {},
                    other => panic!("unexpected action {other}"),
                }
                finding()
            }
            other => panic!("unexpected route {other}"),
        };
        Json(serde_json::json!({"jsonrpc":"2.0","id":r["id"],"result":result}))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let secret = root.path().join("secret");
    std::fs::write(&secret, "sentinel-secret\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let run = |args: &[&str], stdin: Option<&str>| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(["assignment", "finding"])
            .args(args)
            .env("FORGE_ASSIGNMENT_ID", "assigned")
            .env("FORGE_ASSIGNMENT_DAEMON_URL", &endpoint)
            .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", &secret)
            .env("FORGE_SERVER_URL", "http://127.0.0.1:1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(input) = stdin {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
        }
        let output = child.wait_with_output().unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains("sentinel-secret"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("sentinel-secret"));
        output
    };
    let output = run(
        &[
            "list",
            "--after",
            "2",
            "--limit",
            "1",
            "--compare-revision",
            &"a".repeat(40),
            "--json",
        ],
        None,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["findings"][0]["version"], 2);
    let output = run(&["show", "3", "--after", "8", "--limit", "1"], None);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("Next cursor: 9")
            && text.contains("assigned")
            && text.contains("Review issue")
    );
    let output = run(
        &[
            "raise",
            "--severity",
            "minor",
            "--blocking",
            "false",
            "--check",
            "4",
            "--assessment",
            "5",
            "--message-file",
            "-",
            "--dirty",
            "false",
            "--json",
        ],
        Some("Review from stdin"),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let file = root.path().join("message.md");
    std::fs::write(&file, "Review action").unwrap();
    for (action, extra) in [
        ("respond", vec!["--response", "rejected"]),
        ("resolve", vec![]),
        ("dismiss", vec![]),
        ("reopen", vec![]),
        (
            "amend",
            vec!["--severity", "critical", "--blocking", "false"],
        ),
        ("note", vec![]),
    ] {
        let mut args = vec![
            action,
            "3",
            "--expected-version",
            "2",
            "--message-file",
            file.to_str().unwrap(),
            "--dirty",
            "true",
        ];
        args.extend(extra);
        let output = run(&args, None);
        if action == "note" {
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("stale-version"));
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    server.abort();
}

#[test]
fn assignment_finding_cli_rejects_task_actor_and_malformed_mutations() {
    for args in [
        vec!["list", "7"],
        vec!["list", "--task", "7"],
        vec!["list", "--actor", "operator"],
        vec![
            "raise",
            "--severity",
            "minor",
            "--message",
            "ok",
            "--actor",
            "operator",
        ],
        vec!["raise", "--severity", "minor"],
        vec![
            "raise",
            "--severity",
            "minor",
            "--message",
            "ok",
            "--message-file",
            "-",
        ],
        vec!["resolve", "3", "--message", "ok"],
        vec!["respond", "3", "--expected-version", "1", "--message", "ok"],
        vec!["amend", "3", "--expected-version", "1", "--message", "ok"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(["assignment", "finding"])
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
}
