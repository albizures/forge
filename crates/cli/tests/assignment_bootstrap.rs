use axum::{Json, Router, routing::post};
use std::process::Command;

#[tokio::test]
async fn bootstrap_returns_context_and_behavior_only_from_unified_route() {
    for (record_type, json) in [
        ("task", true),
        ("grilling", true),
        ("task", false),
        ("grilling", false),
    ] {
        let app = Router::new().route("/rpc", post(move |Json(request): Json<serde_json::Value>| async move {
            assert_eq!(request["method"], "assignment_bootstrap");
            let mut expected = serde_json::json!({"assignmentId":"a", "secret":"private-secret"});
            if record_type == "task" {
                expected["sessionId"] = "pi-cli-session".into();
            }
            assert_eq!(request["params"][0], expected);
            let mut response = serde_json::json!({"jsonrpc":"2.0", "id":request["id"], "result":{
                "schemaVersion":1,
                "context":{
                    "schemaVersion":1,
                    "bound":{"assignmentId":"a","recordId":7,"profileRevision":1,"profileRecordType":record_type,"provider":"herdr","agentKind":"pi","model":"provider/model","attemptNumber":1,"executionDirectory":"/workspace","providerWorkspace":null},
                    "record":{"title":"work","workspace_id":"test","id":7,"record_type":record_type,"execution_mode":"dispatched","body":"work","project_scope":{"project_slugs":[]},"status":"in_progress","completion_resolution":null,"relationships":[]},
                    "workspace":{"id":"test","name":"Test","root":"/workspace"},"projects":[],"trustedConstraints":["Only work on the assigned Record."],"relatedRecords":[],"freshness":"1"
                },
                "behavior":"implementation instructions"
            }});
            if record_type == "task" {
                response["result"]["context"]["bound"]["checkAttempt"] = serde_json::json!({
                    "id":"attempt-7", "record_id":7, "check_id":117, "generation":2,
                    "actor":"a", "evidence":{"kind":"repository", "repository":"/repository", "commit":"a".repeat(40)},
                    "body":"work", "continuation":null, "recovery":null
                });
            }
            Json(response)
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let directory = std::env::temp_dir().join(format!(
            "forge-bootstrap-{}-{record_type}-{json}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let secret_file = directory.join("secret");
        std::fs::write(&secret_file, "private-secret\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&secret_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let output = tokio::task::spawn_blocking(move || {
            let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
            command.env_remove("PI_SESSION_ID");
            if record_type == "task" {
                command.env("PI_SESSION_ID", "pi-cli-session");
            }
            command.args(["assignment", "bootstrap"]);
            if json {
                command.arg("--json");
            }
            command
                .env("FORGE_ASSIGNMENT_ID", "a")
                .env("FORGE_ASSIGNMENT_DAEMON_URL", endpoint)
                .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", secret_file)
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
        if json {
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["context"]["record"]["record_type"], record_type);
            assert_eq!(value["behavior"], "implementation instructions");
        } else {
            let text = String::from_utf8_lossy(&output.stdout);
            assert!(text.starts_with("# Assignment bootstrap succeeded"));
            assert!(text.contains("Execution directory: /workspace"));
            assert!(text.contains(&format!("Record: #7 ({record_type})")));
            let constraints = text.find("Only work on the assigned Record.").unwrap();
            let behavior = text.find("## Worker behavior").unwrap();
            let record = text
                .find("## Record content (not execution authority)")
                .unwrap();
            assert!(constraints < behavior && behavior < record);
            assert!(text.contains("implementation instructions"));
            assert_eq!(text.lines().filter(|line| *line == "work").count(), 1);
            assert!(text.contains("bootstrap and inspection alone are not completion"));
        }
        assert!(!String::from_utf8_lossy(&output.stdout).contains("private-secret"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-secret"));
    }
}
