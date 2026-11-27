use axum::{Json, Router, routing::post};
use std::process::Command;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn implementation_outcome_rejection_names_the_specific_error() {
    let router = Router::new().route(
        "/rpc",
        post(|Json(r): Json<serde_json::Value>| async move {
            Json(serde_json::json!({"jsonrpc":"2.0", "id": r["id"], "error":{
                "code":-32001, "message":"sentinel-secret must not leak",
                "data":"implementation-outcome-forbidden"}}))
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
    let outcome = root.path().join("outcome.json");
    std::fs::write(
        &outcome,
        r#"{"attempt_id":"a","generation":1,"evidence":{"kind":"repository","repository":"r","commit":"0000000000000000000000000000000000000000"},"verdict":"pass","actor":"x","message":"done"}"#,
    )
    .unwrap();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(["assignment", "outcome", "--file"])
            .arg(&outcome)
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
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("implementation-outcome-forbidden"), "{stderr}");
    assert!(!stderr.contains("sentinel-secret"), "{stderr}");
    assert!(output.stdout.is_empty());
}
