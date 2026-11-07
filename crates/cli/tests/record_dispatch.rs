use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, Application, NewTypedRecord, Project,
    ProjectRepository, RecordRepository, RecordType, RegistrationError, RepositoryInspector,
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
    fn provision(
        &self,
        context: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
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
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        panic!("dispatch must not retire")
    }
}
struct NoInspector;
impl RepositoryInspector for NoInspector {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        panic!("no Git inspection")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_dispatch_round_trips_and_reports_existing_assignment_without_relaunch() {
    let db = std::env::temp_dir().join(format!(
        "forge-record-dispatch-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SqliteProjectRepository::open(&db).unwrap();
    store
        .register(&Project {
            slug: "alpha".into(),
            registered_folder: "/repos/alpha".into(),
            origin_url: "https://example.test/alpha".into(),
        })
        .unwrap();
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
    let launches = Arc::new(AtomicUsize::new(0));
    let app = Application::with_agent_dispatch(
        Box::new(SqliteProjectRepository::open(&db).unwrap()),
        Box::new(SqliteProjectRepository::open(&db).unwrap()),
        Box::new(InteractiveProvider(launches.clone())),
        Box::new(NoInspector),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/rpc", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router(Daemon::new(app)))
            .await
            .unwrap();
    });
    let run = |id: &str, json: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
        command.args(["dispatcher", "dispatch", id, "--server-url", &url]);
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
    server.abort();
    let _ = server.await;
    drop(store);
    std::fs::remove_file(db).unwrap();
}
