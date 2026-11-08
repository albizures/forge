mod support;
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchRepository,
    AgentProfile, Application, NewAgentAssignment, NewAgentAssignmentResource, NewTypedRecord,
    Project, RecordRepository, RecordType, RegistrationError, RepositoryInspector,
};
use forge_server::{Daemon, router};
use forge_store::SqliteProjectRepository;
use std::{path::Path, process::Command};

struct NoProvider;
impl AgentDispatchAdapterFactory for NoProvider {
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        panic!("inspection must not contact the provider")
    }
}
struct NoInspector;
impl RepositoryInspector for NoInspector {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        panic!("inspection must not inspect Git")
    }
    fn head_commit(&self, _: &Path) -> Result<String, RegistrationError> {
        panic!("inspection must not inspect Git")
    }
}

fn inspection_application() -> Application {
    let store = SqliteProjectRepository::in_memory().unwrap();
    support::register_project(&store, "alpha");
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            task_kind: None,
            body: "PRIVATE TASK PROMPT".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    store
        .create_agent_assignment(&NewAgentAssignment {
            id: "inspect-me".into(),
            record_id: task.id,
            project_slug: "alpha".into(),
            profile_revision: 1,
        })
        .unwrap();
    store.begin_agent_launch("inspect-me", 1000).unwrap();
    store
        .guard_agent_provisioning("inspect-me", 301_000)
        .unwrap();
    store
        .add_agent_assignment_resource(&NewAgentAssignmentResource {
            assignment_id: "inspect-me".into(),
            resource_kind: "pane".into(),
            resource_id: "pane-42".into(),
        })
        .unwrap();
    store
        .add_agent_assignment_resource(&NewAgentAssignmentResource {
            assignment_id: "inspect-me".into(),
            resource_kind: "tab".into(),
            resource_id: "tab-retired".into(),
        })
        .unwrap();
    store
        .retire_agent_assignment_resource("inspect-me", "tab", "tab-retired")
        .unwrap();
    Application::with_agent_dispatch(
        Box::new(SqliteProjectRepository::in_memory().unwrap()),
        Box::new(store),
        Box::new(NoProvider),
        Box::new(NoInspector),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn assignment_info_cli_renders_human_and_json_diagnostics() {
    let app = inspection_application();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/rpc", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router(Daemon::new(app)))
            .await
            .unwrap();
    });
    let run = |id: &str, json: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
        command.args(["dispatcher", "assignment-info", id, "--server-url", &url]);
        if json {
            command.arg("--json");
        }
        command.output().unwrap()
    };
    let human = run("inspect-me", false);
    assert!(
        human.status.success(),
        "{}",
        String::from_utf8_lossy(&human.stderr)
    );
    let human = String::from_utf8(human.stdout).unwrap();
    for expected in [
        "inspect-me",
        "Task 1",
        "provisioning",
        "301000",
        "not acknowledged",
        "Dispatch Hold",
        "deadline expired",
        "pane-42",
        "tab-retired",
        "retired at",
        "unretired-resources",
        "assignment-retire inspect-me",
        "resume 1",
        "does not resolve",
    ] {
        assert!(human.contains(expected), "missing {expected}: {human}");
    }
    let json = run("inspect-me", true);
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["record_id"], 1);
    assert_eq!(value["acknowledgement_deadline_millis"], 301_000);
    assert_eq!(value["resources"][0]["resource_id"], "pane-42");
    assert_eq!(
        value["resources"][0]["retired_at_millis"],
        serde_json::Value::Null
    );
    for text in [human, String::from_utf8(json.stdout).unwrap()] {
        for private in [
            "PRIVATE TASK PROMPT",
            "prompt_template",
            "agent-dispatch/task/v1",
            "capability",
            "secret",
        ] {
            assert!(!text.contains(private), "leaked {private}");
        }
    }
    for json in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
        command.args(["dispatcher", "resume", "1", "--server-url", &url]);
        if json {
            command.arg("--json");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        if json {
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["task_id"], 1);
            assert_eq!(value["resolves_recovery_blockers"], false);
            assert_eq!(value["authorizes_replacement"], false);
        } else {
            assert!(text.contains("Dispatch Hold cleared"), "{text}");
            assert!(
                text.contains("does not resolve recovery blockers"),
                "{text}"
            );
            assert!(text.contains("does not authorize a replacement"), "{text}");
        }
        let inspected = run("inspect-me", true);
        let info: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
        assert!(info["dispatch_hold"].is_null());
        assert_eq!(info["capacity_reserved"], true);
        assert_eq!(info["state"], "provisioning");
    }
    let missing = run("missing", true);
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("does not exist"));
    server.abort();
}
