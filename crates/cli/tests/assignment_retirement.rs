mod support;
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentProfile, Application, NewAgentAssignment,
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
        atomic::{AtomicBool, Ordering},
    },
};

struct Provider(Arc<AtomicBool>);
impl AgentDispatchAdapterFactory for Provider {
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        Ok(Box::new(Self(self.0.clone())))
    }
}
impl AgentDispatchAdapter for Provider {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        panic!("no probe during retirement")
    }
    fn recover(
        &self,
        _: &AgentDispatchContext,
    ) -> Result<forge_app::AgentObservation, AgentAdapterError> {
        panic!("no recovery during retirement")
    }
    fn provision(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        panic!("no launch during retirement")
    }
    fn observe(
        &self,
        _: &AgentDispatchContext,
    ) -> Result<forge_app::AgentObservation, AgentAdapterError> {
        panic!("no observation during retirement")
    }
    fn retire(
        &self,
        context: &AgentDispatchContext,
        store: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        assert!(
            context.assignment.closed_at_millis.is_some(),
            "revoke before contacting provider"
        );
        if self.0.load(Ordering::SeqCst) {
            return Err(AgentAdapterError::Cleanup("provider unavailable".into()));
        }
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
struct Inspector;
impl RepositoryInspector for Inspector {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        panic!("no Git changes")
    }
    fn head_commit(&self, _: &Path) -> Result<String, RegistrationError> {
        panic!("no Git changes")
    }
}

fn retirement_fixture(unknown: bool) -> (std::path::PathBuf, SqliteProjectRepository) {
    let path = std::env::temp_dir().join(format!(
        "forge-retirement-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SqliteProjectRepository::open(&path).unwrap();
    support::register_project(&store, "alpha");
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            task_kind: None,
            body: "PRIVATE PROMPT".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    store
        .create_agent_assignment(&NewAgentAssignment {
            id: "retire-me".into(),
            record_id: task.id,
            project_slug: "alpha".into(),
            profile_revision: 1,
        })
        .unwrap();
    store.begin_agent_launch("retire-me", 1000).unwrap();
    if unknown {
        store
            .add_agent_assignment_resource(&NewAgentAssignmentResource {
                assignment_id: "retire-me".into(),
                resource_kind: "provision-intent".into(),
                resource_id: "once".into(),
            })
            .unwrap();
    }
    store
        .add_agent_assignment_resource(&NewAgentAssignmentResource {
            assignment_id: "retire-me".into(),
            resource_kind: "pane".into(),
            resource_id: "persisted-pane".into(),
        })
        .unwrap();
    (path, store)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retirement_cli_retries_failed_cleanup_and_reports_remaining_uncertainty() {
    for unknown in [false, true] {
        let (path, store) = retirement_fixture(unknown);
        let fail = Arc::new(AtomicBool::new(true));
        let app = Application::with_agent_dispatch(
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(store),
            Box::new(Provider(fail.clone())),
            Box::new(Inspector),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/rpc", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router(Daemon::new(app)))
                .await
                .unwrap();
        });
        let run = |operation: &str, json: bool| {
            let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
            command.args(["dispatcher", operation, "retire-me", "--server-url", &url]);
            if json {
                command.arg("--json");
            }
            command.output().unwrap()
        };
        let failed = run("assignment-retire", false);
        assert!(!failed.status.success());
        let info: serde_json::Value =
            serde_json::from_slice(&run("assignment-info", true).stdout).unwrap();
        assert!(info["closed_at_millis"].is_number());
        assert_eq!(info["capacity_reserved"], true);
        fail.store(false, Ordering::SeqCst);
        let retried = run("assignment-retire", false);
        assert!(
            retried.status.success(),
            "{}",
            String::from_utf8_lossy(&retried.stderr)
        );
        let human = String::from_utf8(retried.stdout).unwrap();
        assert_eq!(human.contains("uncertain-provisioning"), unknown, "{human}");
        assert!(human.contains("Dispatch Hold"));
        assert!(!human.contains("PRIVATE PROMPT"));
        let info: serde_json::Value =
            serde_json::from_slice(&run("assignment-info", true).stdout).unwrap();
        assert_eq!(
            info["capacity_reserved"], unknown,
            "only known-resource cleanup releases capacity"
        );
        assert!(
            info["resources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["resource_kind"] == "pane")
                .unwrap()["retired_at_millis"]
                .is_number()
        );
        if unknown {
            verify_unknown_resource_resolution(&url, &run);
        }
        server.abort();
        let _ = std::fs::remove_file(path);
    }
}

fn verify_unknown_resource_resolution(
    url: &str,
    run: &impl Fn(&str, bool) -> std::process::Output,
) {
    let resolve = |flags: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args([
                "dispatcher",
                "assignment-resolve-unknown",
                "retire-me",
                "--server-url",
                url,
            ])
            .args(flags)
            .output()
            .unwrap()
    };
    for flags in [
        vec![],
        vec!["--confirm-old-agent-cannot-act"],
        vec!["--confirm-unknown-resources-safe"],
    ] {
        let rejected = resolve(&flags);
        assert!(!rejected.status.success());
        let warning = String::from_utf8_lossy(&rejected.stderr);
        assert!(warning.contains("old agent can no longer act"), "{warning}");
        assert!(warning.contains("unknown resources"), "{warning}");
        let info: serde_json::Value =
            serde_json::from_slice(&run("assignment-info", true).stdout).unwrap();
        assert_eq!(info["capacity_reserved"], true);
        assert!(info["unknown_resource_resolution"].is_null());
    }
    let confirmed = resolve(&[
        "--confirm-old-agent-cannot-act",
        "--confirm-unknown-resources-safe",
    ]);
    assert!(
        confirmed.status.success(),
        "{}",
        String::from_utf8_lossy(&confirmed.stderr)
    );
    let human = String::from_utf8(confirmed.stdout).unwrap();
    assert!(human.contains("Operator confirmed"), "{human}");
    assert!(human.contains("cannot undo external actions"), "{human}");
    assert!(!human.contains("PRIVATE PROMPT"));
    let info: serde_json::Value =
        serde_json::from_slice(&run("assignment-info", true).stdout).unwrap();
    assert_eq!(info["capacity_reserved"], false);
    assert!(info["dispatch_hold"].is_string());
    assert!(info["unknown_resource_resolution"]["confirmed_at_millis"].is_number());
    let json = resolve(&[
        "--confirm-old-agent-cannot-act",
        "--confirm-unknown-resources-safe",
        "--json",
    ]);
    assert!(json.status.success());
    let repeated: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(
        repeated["unknown_resource_resolution"],
        info["unknown_resource_resolution"]
    );
}
