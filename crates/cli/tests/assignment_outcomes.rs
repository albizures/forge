use axum::{Json, Router, routing::post};
use std::process::Command;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outcome_cli_uses_protected_delivery_and_redacts_provider_messages() {
    check_outcome_rejection("outcome-conflict").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_interruption_cli_reports_safe_choices_without_success_json() {
    check_outcome_rejection("interrupted-evidence-changed").await;
}

#[test]
fn retired_continuation_disposition_is_rejected_before_delivery() {
    let root = tempfile::tempdir().unwrap();
    let secret = root.path().join("secret");
    std::fs::write(&secret, "sentinel-secret\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let file = root.path().join("outcome.json");
    std::fs::write(
        &file,
        serde_json::json!({
            "attempt_id":"bound-assignment", "generation":7,
            "evidence":{"kind":"repository", "repository":"/repo", "commit":"a".repeat(40)},
            "verdict":"fail", "actor":"", "message":"Checkpoint",
            "disposition":{"continuation":{
                "stopping_reason":"Execution constraint", "completed_work":"Checkpoint",
                "verification":"Focused tests pass; full gates unrun", "remaining_work":"Finish"
            }}
        })
        .to_string(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(["assignment", "outcome", "--file"])
        .arg(&file)
        .arg("--json")
        .env("FORGE_ASSIGNMENT_ID", "bound-assignment")
        .env("FORGE_ASSIGNMENT_DAEMON_URL", "http://127.0.0.1:1")
        .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", &secret)
        .env("FORGE_SERVER_URL", "http://127.0.0.1:1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid outcome JSON"));
}

async fn check_outcome_rejection(category: &'static str) {
    let router = Router::new().route(
        "/rpc",
        post(move |Json(r): Json<serde_json::Value>| async move {
            assert_eq!(r["method"], "assignment_outcomeSubmit");
            let p = &r["params"][0];
            assert_eq!(p["assignmentId"], "bound-assignment");
            assert_eq!(p["secret"], "sentinel-secret");
            assert_eq!(p["outcome"]["attempt_id"], "bound-assignment");
            assert_eq!(p["outcome"]["generation"], 7);
            assert!(p.get("record").is_none());
            Json(serde_json::json!({"jsonrpc":"2.0", "id":r["id"], "error":{
            "code":-32001, "message":"sentinel-secret must not leak", "data":category}}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let secret = root.path().join("secret");
    std::fs::write(&secret, "sentinel-secret\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let file = root.path().join("outcome.json");
    std::fs::write(
        &file,
        serde_json::json!({
            "attempt_id":"bound-assignment", "generation":7,
            "evidence":{"kind":"repository", "repository":"/repo", "commit":"a".repeat(40)},
            "verdict":"pass", "actor":"", "message":"Verified"
        })
        .to_string(),
    )
    .unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(["assignment", "outcome", "--file"])
            .arg(&file)
            .arg("--json")
            .env("FORGE_ASSIGNMENT_ID", "bound-assignment")
            .env("FORGE_ASSIGNMENT_DAEMON_URL", &endpoint)
            .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", &secret)
            .env("FORGE_SERVER_URL", "http://127.0.0.1:1")
            .output()
            .unwrap()
    };
    let output = run();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(category), "{stderr}");
    assert!(output.stdout.is_empty());
    if category == "interrupted-evidence-changed" {
        for guidance in [
            "operator's explicit",
            "Preserve the checkout",
            "recover-retained-work",
        ] {
            assert!(stderr.contains(guidance), "{stderr}");
        }
    }
    assert!(!String::from_utf8_lossy(&output.stderr).contains("sentinel-secret"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("sentinel-secret"));
    std::fs::write(&file, "{invalid JSON").unwrap();
    assert!(String::from_utf8_lossy(&run().stderr).contains("invalid outcome JSON"));
    server.abort();
}
