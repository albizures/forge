use axum::{Json, Router, routing::post};
use std::process::{Command, Output};

async fn run_request_review(reply: serde_json::Value) -> Output {
    let router = Router::new().route(
        "/rpc",
        post(move |Json(r): Json<serde_json::Value>| {
            let reply = reply.clone();
            async move {
                assert_eq!(r["method"], "assignment_reviewRequest");
                // Credentials only: the Agent supplies no attempt, verdict or evidence.
                assert_eq!(
                    r["params"][0],
                    serde_json::json!({"assignmentId": "bound-assignment", "secret": "sentinel-secret"})
                );
                let mut reply = reply;
                reply["id"] = r["id"].clone();
                Json(reply)
            }
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
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(["assignment", "request-review"])
            .env("FORGE_ASSIGNMENT_ID", "bound-assignment")
            .env("FORGE_ASSIGNMENT_DAEMON_URL", &endpoint)
            .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", &secret)
            .env("FORGE_SERVER_URL", "http://127.0.0.1:1")
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    server.abort();
    output
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejection_names_the_specific_error_and_how_to_proceed() {
    for (code, hint) in [
        ("review-request-worktree-invalid", "commit your work"),
        ("review-request-not-implementation", "Implementation Check"),
    ] {
        let output = run_request_review(serde_json::json!({"jsonrpc":"2.0", "error":{
            "code":-32001, "message":format!("{hint} (sentinel-secret must not be added)"), "data":code}}))
        .await;
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(code), "{stderr}");
        assert!(stderr.contains(hint), "{stderr}");
        assert!(output.stdout.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_failures_stay_redacted() {
    let output = run_request_review(serde_json::json!({"jsonrpc":"2.0", "error":{
        "code":-32001, "message":"sentinel-secret must not leak", "data":"storage"}}))
    .await;
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Review Request rejected"), "{stderr}");
    assert!(!stderr.contains("sentinel-secret"), "{stderr}");
}
