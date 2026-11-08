use axum::{Json, Router, extract::State, routing::post};
use std::{
    process::Command,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy)]
enum Scenario {
    Success,
    LostResponses,
    AuthenticationFailure,
    UnconfirmedStart,
    InstructionsFailure,
}

struct Calls {
    scenario: Scenario,
    methods: Vec<String>,
    ack: usize,
    start: usize,
    read: usize,
    report: usize,
}

async fn handler(
    State(state): State<Arc<Mutex<Calls>>>,
    Json(request): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let mut calls = state.lock().unwrap();
    let method = request["method"]
        .as_str()
        .unwrap()
        .rsplit('.')
        .next()
        .unwrap();
    calls.methods.push(method.into());
    // Bootstrap must use secret authentication at every step, not operator credentials.
    let params = &request["params"][0];
    assert_eq!(params["assignment_id"], "a");
    assert_eq!(params["secret"], "private-secret");
    assert_eq!(params["correlation_id"], "bootstrap");
    let result = if method.ends_with("assignmentAcknowledge") {
        calls.ack += 1;
        if matches!(calls.scenario, Scenario::AuthenticationFailure)
            || (matches!(calls.scenario, Scenario::LostResponses) && calls.ack == 1)
        {
            None
        } else {
            Some(serde_json::json!({"id":"a", "record_id":7, "state":"working"}))
        }
    } else if method.ends_with("assignmentTaskStart") {
        assert!(calls.ack > 0);
        assert_eq!(params["task_id"], 7);
        calls.start += 1;
        if matches!(calls.scenario, Scenario::LostResponses) && calls.start == 1 {
            None
        } else {
            Some(serde_json::json!({
                "workspace_id":"test", "id":7, "record_type":"task", "task_kind":null, "body":"task",
                "project_scope":{"project_slugs":[]},
                "status":if matches!(calls.scenario, Scenario::UnconfirmedStart) {"open"} else {"in_progress"},
                "completion_resolution":null, "relationships":[]
            }))
        }
    } else if method.ends_with("assignmentInstructions") {
        assert!(calls.start > 0);
        assert!(!matches!(calls.scenario, Scenario::UnconfirmedStart));
        calls.read += 1;
        if matches!(calls.scenario, Scenario::InstructionsFailure) {
            None
        } else {
            Some(serde_json::json!("implementation instructions"))
        }
    } else if method.ends_with("assignmentBootstrapFailure") {
        calls.report += 1;
        Some(serde_json::Value::Null)
    } else {
        panic!("unexpected method {method}")
    };
    Json(match result {
        Some(value) => serde_json::json!({"jsonrpc":"2.0", "id":request["id"], "result":value}),
        None => {
            serde_json::json!({"jsonrpc":"2.0", "id":request["id"], "error":{"code":-32000,"message":"denied or response lost"}})
        }
    })
}

#[tokio::test]
async fn assignment_bootstrap_command_preserves_authenticated_start_gates_and_bounded_retries() {
    for (scenario, expected, success) in [
        (Scenario::Success, (1, 1, 1, 0), true),
        (Scenario::LostResponses, (2, 2, 1, 0), true),
        (Scenario::AuthenticationFailure, (3, 0, 0, 0), false),
        (Scenario::UnconfirmedStart, (1, 3, 0, 1), false),
        (Scenario::InstructionsFailure, (1, 1, 3, 1), false),
    ] {
        let state = Arc::new(Mutex::new(Calls {
            scenario,
            methods: Vec::new(),
            ack: 0,
            start: 0,
            read: 0,
            report: 0,
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/rpc", post(handler))
            .with_state(state.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let directory = std::env::temp_dir().join(format!(
            "forge-bootstrap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let secret_file = directory.join("secret");
        std::fs::write(&secret_file, "private-secret\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&secret_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let output = tokio::task::spawn_blocking(move || {
            Command::new(env!("CARGO_BIN_EXE_forge"))
                .args(["assignment", "bootstrap"])
                .env("FORGE_ASSIGNMENT_ID", "a")
                .env("FORGE_ASSIGNMENT_DAEMON_URL", endpoint)
                .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", secret_file)
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        std::fs::remove_dir_all(directory).unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if success {
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "implementation instructions\n"
            );
        } else {
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("forge assignment bootstrap:")
            );
        }
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-secret"));
        let calls = state.lock().unwrap();
        assert_eq!(
            (calls.ack, calls.start, calls.read, calls.report),
            expected,
            "{:?}",
            calls.methods
        );
        server.abort();
    }
}
