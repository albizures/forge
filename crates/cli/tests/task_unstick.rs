mod support;
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentProfile, Application, NewAgentAssignment,
    NewAgentAssignmentResource, NewTypedRecord, Project, RecordRepository, RecordType,
    RegistrationError, RepositoryInspector,
};
use forge_server::{Daemon, router};
use forge_store::SqliteProjectRepository;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Provider that only supports retirement; any launch or observation is a test failure.
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
        Err(AgentAdapterError::Endpoint(
            "no probe in unstick tests".into(),
        ))
    }
    fn recover(
        &self,
        _: &AgentDispatchContext,
    ) -> Result<forge_app::AgentObservation, AgentAdapterError> {
        panic!("no recovery during unstick")
    }
    fn provision(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        panic!("no launch in unstick tests")
    }
    fn observe(
        &self,
        _: &AgentDispatchContext,
    ) -> Result<forge_app::AgentObservation, AgentAdapterError> {
        panic!("no observation during unstick")
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
        for resource in context
            .resources
            .iter()
            .filter(|r| r.resource_kind == "pane")
        {
            store
                .retire_agent_assignment_resource(
                    &context.assignment.id,
                    "pane",
                    &resource.resource_id,
                )
                .unwrap();
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

struct Fixture {
    path: PathBuf,
    store: SqliteProjectRepository,
    task: u64,
    url: String,
    fail: Arc<AtomicBool>,
    server: tokio::task::JoinHandle<()>,
}

impl Fixture {
    /// An Open Task whose agent missed its acknowledgement deadline and holds a pane,
    /// optionally with a crash-left provisioning marker.
    async fn new(unknown: bool) -> Self {
        let path = std::env::temp_dir().join(format!(
            "forge-unstick-{}-{}.sqlite3",
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
                body: "PRIVATE PROMPT".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap()
            .id;
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "stuck".into(),
                record_id: task,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        store.begin_agent_launch("stuck", 1000).unwrap();
        let mut resources = vec![("pane", "persisted-pane")];
        if unknown {
            resources.push(("provision-intent", "once"));
        }
        for (kind, id) in resources {
            store
                .add_agent_assignment_resource(&NewAgentAssignmentResource {
                    assignment_id: "stuck".into(),
                    resource_kind: kind.into(),
                    resource_id: id.into(),
                })
                .unwrap();
        }
        store
            .hold_dispatch(task, "Acknowledgement deadline missed")
            .unwrap();
        let fail = Arc::new(AtomicBool::new(false));
        let app = Application::with_agent_dispatch(
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(Provider(fail.clone())),
            Box::new(Inspector),
        )
        .with_workspace_repository(Box::new(SqliteProjectRepository::open(&path).unwrap()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/rpc", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router(Daemon::new(app)))
                .await
                .unwrap();
        });
        Self {
            path,
            store,
            task,
            url,
            fail,
            server,
        }
    }

    fn unstick(&self, flags: &[&str]) -> (Output, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_forge"))
            .args([
                "--workspace",
                "test",
                "task",
                "unstick",
                &self.task.to_string(),
            ])
            .args(["--server-url", &self.url, "--json"])
            .args(flags)
            .output()
            .unwrap();
        let report = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("stderr: {}", String::from_utf8_lossy(&output.stderr)));
        (output, report)
    }

    fn steps(report: &Value) -> Vec<(String, String)> {
        report["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["kind"].as_str().unwrap().to_owned(),
                    s["status"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }

    fn assert_untouched(&self) {
        let assignment = self.store.agent_assignment("stuck").unwrap();
        assert!(
            assignment.closed_at_millis.is_none(),
            "plan must not retire"
        );
        assert!(self.store.dispatch_hold(self.task).unwrap().is_some());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        let _ = std::fs::remove_file(&self.path);
    }
}

fn pair(kind: &str, status: &str) -> (String, String) {
    (kind.into(), status.into())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plan_changes_nothing_and_names_required_flags() {
    let fixture = Fixture::new(true).await;
    let (output, report) = fixture.unstick(&[]);
    assert_eq!(output.status.code(), Some(2), "needs-flag exits 2");
    assert_eq!(report["verdict"], "needs-flag");
    assert_eq!(report["applied"], false);
    assert_eq!(
        Fixture::steps(&report),
        vec![
            pair("retire", "pending"),
            pair("resolve-unknown", "needs-flag"),
            pair("resume", "pending"),
            pair("dispatch", "pending"),
        ]
    );
    assert_eq!(report["steps"][1]["required_flags"][0], "agent-gone");
    fixture.assert_untouched();

    // Apply refuses to start a plan it already knows cannot finish.
    let (output, report) = fixture.unstick(&["--apply"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(report["verdict"], "needs-flag");
    fixture.assert_untouched();
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE PROMPT"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn apply_runs_runbook_in_order_and_stops_at_first_failure() {
    let fixture = Fixture::new(true).await;
    let (output, report) = fixture.unstick(&["--apply", "--agent-gone"]);
    // This daemon has no Managed Worktrees, so the final dispatch gate rejects the launch.
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report["verdict"], "failed");
    assert_eq!(
        Fixture::steps(&report),
        vec![
            pair("retire", "done"),
            pair("resolve-unknown", "done"),
            pair("resume", "done"),
            pair("dispatch", "failed"),
        ]
    );
    assert!(
        report["steps"][3]["error"]
            .as_str()
            .unwrap()
            .contains("managed-worktrees-unavailable")
    );
    let info = fixture.store.agent_assignment_info("stuck").unwrap();
    assert!(info.assignment.closed_at_millis.is_some());
    assert!(!info.capacity_reserved(), "all resources reconciled");
    assert!(info.unknown_resource_resolution.is_some());
    assert!(fixture.store.dispatch_hold(fixture.task).unwrap().is_none());

    // A re-run sees the reconciled state and only retries the launch.
    let (_, report) = fixture.unstick(&[]);
    assert_eq!(Fixture::steps(&report), vec![pair("dispatch", "pending")]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_failure_stops_before_later_steps_and_keeps_hold() {
    let fixture = Fixture::new(false).await;
    fixture.fail.store(true, Ordering::SeqCst);
    let (output, report) = fixture.unstick(&["--apply"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        Fixture::steps(&report),
        vec![
            pair("retire", "failed"),
            pair("resume", "pending"),
            pair("dispatch", "pending"),
        ]
    );
    let info = fixture.store.agent_assignment_info("stuck").unwrap();
    assert!(info.capacity_reserved(), "failed cleanup retains capacity");
    assert!(fixture.store.dispatch_hold(fixture.task).unwrap().is_some());

    // Retrying after the provider recovers retires the closed attempt and continues.
    fixture.fail.store(false, Ordering::SeqCst);
    let (_, report) = fixture.unstick(&["--apply"]);
    assert_eq!(Fixture::steps(&report)[0], pair("retire", "done"));
    assert_eq!(Fixture::steps(&report)[1], pair("resume", "done"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_agent_is_never_retired_without_explicit_flag() {
    let fixture = Fixture::new(false).await;
    fixture.store.acknowledge_agent_assignment("stuck").unwrap();
    // Late acknowledgement: the agent works, but the timeout hold remains.
    let (output, report) = fixture.unstick(&[]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["verdict"], "stuck");
    assert_eq!(report["assignment_id"], "stuck");
    assert_eq!(Fixture::steps(&report), vec![pair("resume", "pending")]);

    let (_, report) = fixture.unstick(&["--apply"]);
    assert_eq!(report["verdict"], "unstuck");
    assert!(
        fixture
            .store
            .agent_assignment("stuck")
            .unwrap()
            .closed_at_millis
            .is_none()
    );
    let (output, report) = fixture.unstick(&[]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["verdict"], "healthy");
    assert!(Fixture::steps(&report).is_empty());

    // Opting in plans a retirement of the active agent.
    let (_, report) = fixture.unstick(&["--retire-working"]);
    assert_eq!(Fixture::steps(&report)[0], pair("retire", "pending"));
}
