use axum::{Json, Router, routing::post};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn check() -> serde_json::Value {
    serde_json::json!({"id":3,"record_id":7,"name":"Implementation","description":"Deliver work",
        "position":0,"version":2,"latest_assessment":{"id":4,"check_id":3,"verdict":"pass",
        "actor":"operator","assignment_id":null,"message":"verified","created_at_millis":123,
        "attempt_id":"manual-fixture","generation":1,"evidence":{"kind":"repository","repository":"test/project","commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
        "revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","dirty":false,"invalidation":null,"comparison":"matching"},
        "current_verdict":"pass","project":"test/project"})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn assignment_check_cli_uses_typed_own_task_routes_and_protected_credentials() {
    let app = Router::new().route("/rpc", post(|Json(r): Json<serde_json::Value>| async move {
        let p = &r["params"][0];
        assert_eq!(p["assignmentId"], "assigned");
        assert_eq!(p["secret"], "sentinel-secret");
        assert!(p.get("task").is_none());
        assert!(p.get("actor").is_none());
        let result = match r["method"].as_str().unwrap() {
            "assignment_checkList" => {
                assert_eq!(p["compareRevision"], "a".repeat(40));
                serde_json::json!([check()])
            }
            "assignment_checkShow" => {
                assert_eq!(p["check"], 3);
                assert_eq!(p["limit"], 1);
                assert_eq!(p["after"], 9);
                serde_json::json!({"check":check(),"assessments":[check()["latest_assessment"]],
                    "activities":[{"id":10,"assessment_id":4,"invalidation":null}],"next_cursor":10})
            }
            "assignment_checkAssess" => {
                assert_eq!(p["expectedVersion"], 1);
                assert_eq!(p["verdict"], "pass");
                assert_eq!(p["message"], "verified from stdin");
                assert_eq!(p["dirty"], false);
                return Json(serde_json::json!({"jsonrpc":"2.0","id":r["id"],"error":{
                    "code":-32001,"message":"sentinel-secret must not leak","data":"attempt-required"}}));
            }
            "assignment_checkInvalidate" => {
                assert_eq!(p["assessmentId"], 4);
                assert_eq!(p["expectedVersion"], 2);
                assert_eq!(p["message"], "withdraw");
                assert_eq!(p["dirty"], true);
                return Json(serde_json::json!({"jsonrpc":"2.0","id":r["id"],"error":{
                    "code":-32001,"message":"sentinel-secret must not leak","data":"stale-version"}}));
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
        let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
        command
            .args(["assignment", "check"])
            .args(args)
            .env("FORGE_ASSIGNMENT_ID", "assigned")
            .env("FORGE_ASSIGNMENT_DAEMON_URL", &endpoint)
            .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", &secret)
            // Operator routing must never override capability delivery.
            .env("FORGE_SERVER_URL", "http://127.0.0.1:1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
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
        &["list", "--compare-revision", &"a".repeat(40), "--json"],
        None,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value[0]["version"], 2);
    let output = run(&["show", "3", "--after", "9", "--limit", "1"], None);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Next cursor: 10"));
    assert!(text.contains("operator"));
    let output = run(
        &[
            "assess",
            "3",
            "--expected-version",
            "1",
            "--verdict",
            "pass",
            "--message-file",
            "-",
            "--dirty",
            "false",
            "--json",
        ],
        Some("verified from stdin"),
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("attempt-required"));
    let output = run(
        &[
            "invalidate",
            "3",
            "--assessment",
            "4",
            "--expected-version",
            "2",
            "--message",
            "withdraw",
            "--dirty",
            "true",
        ],
        None,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("stale-version"));
    server.abort();
}

#[test]
fn assignment_check_arguments_never_accept_task_or_actor_overrides() {
    for args in [
        vec!["list", "7"],
        vec![
            "assess",
            "3",
            "--expected-version",
            "1",
            "--verdict",
            "pass",
            "--actor",
            "operator",
            "--message",
            "ok",
        ],
        vec!["assess", "3", "--verdict", "pass", "--message", "ok"],
        vec![
            "assess",
            "3",
            "--expected-version",
            "1",
            "--verdict",
            "pass",
        ],
        vec![
            "assess",
            "3",
            "--expected-version",
            "1",
            "--verdict",
            "pass",
            "--message",
            "ok",
            "--message-file",
            "-",
        ],
        vec![
            "invalidate",
            "3",
            "--expected-version",
            "1",
            "--message",
            "why",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(["assignment", "check"])
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
}
