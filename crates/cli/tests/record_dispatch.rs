mod support;
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, Application, DispatchWorkspace,
    NewAgentAssignmentResource, NewTypedRecord, Project, RecordRepository, RecordType,
    RegistrationError, RepositoryInspector,
};
use forge_server::{Daemon, router};
use forge_store::SqliteProjectRepository;
use std::{
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone)]
struct InteractiveProvider(Arc<AtomicUsize>);
impl AgentDispatchAdapterFactory for InteractiveProvider {
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        Ok(Box::new(self.clone()))
    }
}
impl AgentDispatchAdapter for InteractiveProvider {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        Ok(())
    }
    fn verify_workspace(&self, id: &str) -> Result<DispatchWorkspace, AgentAdapterError> {
        assert_eq!(id, "operator-workspace");
        Ok(DispatchWorkspace { id: id.into() })
    }
    fn provision(
        &self,
        context: &AgentDispatchContext,
        store: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        assert_eq!(
            store.herdr_socket_path().unwrap().as_deref(),
            Some(Path::new("/tmp/herdr.sock"))
        );
        assert_eq!(
            context.dispatch_workspace.as_ref().unwrap().id,
            "operator-workspace"
        );
        store
            .add_agent_assignment_resource(&NewAgentAssignmentResource {
                assignment_id: context.assignment.id.clone(),
                resource_kind: "pane".into(),
                resource_id: "owned-pane".into(),
            })
            .unwrap();
        assert_eq!(context.assignment.profile.record_type, RecordType::Grilling);
        assert!(context.agent_workspace.is_none());
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        Ok(AgentObservation::Working)
    }
    fn recover(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentObservation, AgentAdapterError> {
        self.observe(context)
    }
    fn retire(
        &self,
        context: &AgentDispatchContext,
        store: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        assert!(context.assignment.closed_at_millis.is_some());
        for resource in &context.resources {
            if resource.resource_kind == "pane" {
                store
                    .retire_agent_assignment_resource(
                        &context.assignment.id,
                        "pane",
                        &resource.resource_id,
                    )
                    .unwrap();
            }
        }
        Ok(())
    }
}
struct NoInspector;
impl RepositoryInspector for NoInspector {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        panic!("no Git inspection")
    }
}

fn dispatch_fixture() -> (std::path::PathBuf, SqliteProjectRepository, Vec<String>) {
    let db = std::env::temp_dir().join(format!(
        "forge-record-dispatch-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SqliteProjectRepository::open(&db).unwrap();
    support::register_project(&store, "alpha");
    let mut ids = Vec::new();
    for kind in [RecordType::Grilling, RecordType::Spec] {
        ids.push(
            store
                .open_typed_record(&NewTypedRecord {
                    record_type: kind,
                    task_kind: None,
                    body: "work".into(),
                    project_slugs: vec!["alpha".into()],
                    generated_by_ids: vec![],
                })
                .unwrap()
                .id
                .to_string(),
        );
    }
    (db, store, ids)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn configuration_dispatch_and_recovery_round_trip_without_duplicate_launches() {
    let (db, store, ids) = dispatch_fixture();
    let launches = Arc::new(AtomicUsize::new(0));
    let app = Application::with_agent_dispatch(
        Box::new(SqliteProjectRepository::open(&db).unwrap()),
        Box::new(SqliteProjectRepository::open(&db).unwrap()),
        Box::new(InteractiveProvider(launches.clone())),
        Box::new(NoInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(&db).unwrap()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/rpc", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router(Daemon::new(app)))
            .await
            .unwrap();
    });
    let operator = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .arg("dispatcher")
            .args(args)
            .args(["--server-url", &url, "--json", "--workspace", "test"])
            .output()
            .unwrap()
    };
    for args in [
        vec!["config", "set", "herdr-socket", "/tmp/herdr.sock"],
        vec!["config", "set", "workspace", "operator-workspace"],
        vec!["config", "set", "task-profile", "1"],
        vec!["config", "set", "grilling-profile", "1"],
        vec!["config", "set", "project-limit", "2", "--project", "alpha"],
    ] {
        let output = operator(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let run = |id: &str, json: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
        command.args([
            "dispatcher",
            "dispatch",
            id,
            "--server-url",
            &url,
            "--workspace",
            "test",
        ]);
        if json {
            command.arg("--json");
        }
        command.output().unwrap()
    };
    let first = run(&ids[0], true);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["already_assigned"], false);
    assert_eq!(first["assignment"]["state"], "working");
    let repeated = run(&ids[0], true);
    assert!(repeated.status.success());
    let repeated: serde_json::Value = serde_json::from_slice(&repeated.stdout).unwrap();
    assert_eq!(repeated["already_assigned"], true);
    assert_eq!(first["assignment"]["id"], repeated["assignment"]["id"]);
    let human = run(&ids[0], false);
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("already assigned"));
    let unsupported = run(&ids[1], true);
    assert!(!unsupported.status.success());
    assert!(String::from_utf8_lossy(&unsupported.stderr).contains("only Tasks and Grillings"));
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    assert_eq!(store.list_open_agent_assignments().unwrap().len(), 1);
    let assignment = first["assignment"]["id"].as_str().unwrap();
    // Config changes cannot strand a live Assignment or its retained resources.
    for key in ["herdr-socket", "workspace"] {
        assert!(!operator(&["config", "unset", key]).status.success());
    }
    let info = operator(&["assignment-info", assignment]);
    assert!(info.status.success());
    let info: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
    assert_eq!(info["capacity_reserved"], true);
    let retired = operator(&["assignment-retire", assignment]);
    assert!(
        retired.status.success(),
        "{}",
        String::from_utf8_lossy(&retired.stderr)
    );
    let info = operator(&["assignment-info", assignment]);
    let info: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
    assert!(info["closed_at_millis"].is_number());
    assert_eq!(info["capacity_reserved"], false);
    assert!(info["dispatch_hold"].is_string());
    assert!(
        !run(&ids[0], true).status.success(),
        "retirement does not clear the hold"
    );
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    // With resources safely resolved, explicit removal is configuration-only.
    for key in ["workspace", "herdr-socket"] {
        assert!(operator(&["config", "unset", key]).status.success());
    }
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    server.abort();
    let _ = server.await;
    drop(store);
    std::fs::remove_file(db).unwrap();
}
