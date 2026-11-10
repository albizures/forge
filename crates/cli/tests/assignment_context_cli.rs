use axum::{Json, Router, routing::post};
use std::process::Command;

fn protect(file: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn context_requires_delivered_assignment_credentials() {
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(["assignment", "context", "--json"])
        .env_remove("FORGE_ASSIGNMENT_ID")
        .env_remove("FORGE_ASSIGNMENT_DAEMON_URL")
        .env_remove("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("missing Assignment"), "{error}");
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn delivery_rejects_symlink_and_non_loopback_daemon() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = std::env::temp_dir().join(format!("forge-cli-malformed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("secret");
    std::fs::write(&file, "sentinel-secret").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.join("link");
    symlink(&file, &link).unwrap();
    for (endpoint, path) in [
        ("http://127.0.0.1:1", &link),
        ("http://example.com:80", &file),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(["assignment", "context"])
            .env("FORGE_ASSIGNMENT_ID", "assigned")
            .env("FORGE_ASSIGNMENT_DAEMON_URL", endpoint)
            .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", path)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("protected") || error.contains("loopback"),
            "{error}"
        );
        assert!(!error.contains("sentinel-secret"));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn delivery_rejects_world_readable_secret_before_connecting() {
    use std::os::unix::fs::PermissionsExt;
    let directory =
        std::env::temp_dir().join(format!("forge-cli-unprotected-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("secret");
    std::fs::write(&file, "sentinel-secret").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(["assignment", "context"])
        .env("FORGE_ASSIGNMENT_ID", "assigned")
        .env("FORGE_ASSIGNMENT_DAEMON_URL", "http://127.0.0.1:1")
        .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", &file)
        .output()
        .unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("protected"), "{error}");
    assert!(!error.contains("sentinel-secret"));
}

#[test]
fn comments_require_explicit_target() {
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(["assignment", "comments"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("<ID>"), "{error}");
}

#[tokio::test]
async fn bootstrap_uses_unified_route_and_does_not_fallback_after_failure() {
    let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = called.clone();
    let app = Router::new().route("/rpc", post(move |Json(request): Json<serde_json::Value>| { let flag = flag.clone(); async move {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(request["method"], "assignment_bootstrap");
        assert_eq!(request["params"][0], serde_json::json!({"assignmentId":"assigned", "secret":"sentinel-secret"}));
        Json(serde_json::json!({"jsonrpc":"2.0", "id":request["id"], "error":{"code":-32000,"message":"sentinel-secret should not be printed"}}))
    }}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let directory =
        std::env::temp_dir().join(format!("forge-cli-bootstrap-new-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("secret");
    std::fs::write(&file, "sentinel-secret\n").unwrap();
    protect(&file);
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(["assignment", "bootstrap", "--json"])
            .env("FORGE_ASSIGNMENT_ID", "assigned")
            .env("FORGE_ASSIGNMENT_DAEMON_URL", endpoint)
            .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", file)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    server.abort();
    std::fs::remove_dir_all(directory).unwrap();
    assert!(called.load(std::sync::atomic::Ordering::SeqCst));
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("sentinel-secret"));
}

#[test]
fn legacy_task_start_cannot_accept_model_facing_secrets() {
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args([
            "assignment",
            "task-start",
            "42",
            "--assignment-id",
            "assigned",
            "--capability-id",
            "sentinel-secret",
            "--correlation-id",
            "x",
        ])
        .output()
        .unwrap();
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("unrecognized subcommand"), "{error}");
    assert!(!error.contains("sentinel-secret"));
}

#[test]
fn own_done_rejects_target_selector() {
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(["assignment", "done", "--target", "42"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!error.contains("unrecognized subcommand 'done'"), "{error}");
    assert!(error.contains("--target"), "{error}");
}

#[test]
fn own_comment_requires_message_and_rejects_target() {
    for args in [
        &["assignment", "comment"][..],
        &[
            "assignment",
            "comment",
            "--message",
            "hello",
            "--target",
            "42",
        ][..],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand 'comment'")
        );
    }
}

#[test]
fn assignment_subscription_command_is_removed() {
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(["assignment", "subscription", "--help"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand 'subscription'")
    );
}

#[tokio::test]
async fn own_start_uses_delivered_identity_without_target() {
    let app = Router::new().route("/rpc", post(|Json(request): Json<serde_json::Value>| async move {
        assert_eq!(request["method"], "assignment_start");
        assert_eq!(request["params"][0], serde_json::json!({"assignmentId":"assigned", "secret":"sentinel-secret"}));
        Json(serde_json::json!({"jsonrpc":"2.0", "id":request["id"], "result":{"schemaVersion":1,"record":{"id":42},"freshness":1}}))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let directory = std::env::temp_dir().join(format!("forge-cli-start-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("secret");
    std::fs::write(&file, "sentinel-secret\n").unwrap();
    protect(&file);
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(["assignment", "start", "--json"])
            .env("FORGE_ASSIGNMENT_ID", "assigned")
            .env("FORGE_ASSIGNMENT_DAEMON_URL", endpoint)
            .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", file)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    server.abort();
    std::fs::remove_dir_all(directory).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("sentinel-secret"));
}

#[tokio::test]
async fn record_read_uses_delivered_credentials_and_production_rpc_name() {
    let app = Router::new().route("/rpc", post(|Json(request): Json<serde_json::Value>| async move {
        assert_eq!(request["method"], "assignment_record");
        assert_eq!(request["params"][0], serde_json::json!({"assignmentId":"assigned", "secret":"sentinel-secret", "target":42}));
        Json(serde_json::json!({"jsonrpc":"2.0", "id":request["id"], "result":{"schemaVersion":1,"record":{"id":42},"freshness":1}}))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let directory = std::env::temp_dir().join(format!("forge-cli-record-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("secret");
    std::fs::write(&file, "sentinel-secret\n").unwrap();
    protect(&file);
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(["assignment", "record", "42", "--json"])
            .env("FORGE_ASSIGNMENT_ID", "assigned")
            .env("FORGE_ASSIGNMENT_DAEMON_URL", endpoint)
            .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", file)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    server.abort();
    std::fs::remove_dir_all(directory).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"schemaVersion\":1"));
    assert!(!stdout.contains("sentinel-secret"));
}
