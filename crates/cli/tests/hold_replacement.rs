mod support;
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, Application, NewAgentAssignment,
    NewAgentAssignmentResource, NewTypedRecord, Project, RecordRepository, RecordStatus,
    RecordType, RegistrationError, RepositoryInspector,
};
use forge_rpc::{AgentDispatchApiServer, RecordIdRequest, RetireAgentAssignmentRequest};
use forge_server::Daemon;
use forge_store::SqliteProjectRepository;
use std::path::{Path, PathBuf};

// A provider contract double: any launch/prompt path is a duplicate attempt in these scenarios.
#[derive(Clone)]
struct Provider(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl AgentDispatchAdapterFactory for Provider {
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        Ok(Box::new(self.clone()))
    }
}
impl AgentDispatchAdapter for Provider {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        Ok(())
    }
    fn provision(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        panic!("resume must not launch or resend a prompt to an uncertain Assignment")
    }
    fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        panic!("missing external identity is not evidence of absence")
    }
    fn recover(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        panic!("uncertain provisioning must not trigger recovery relaunch")
    }
    fn retire(
        &self,
        context: &AgentDispatchContext,
        store: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        if !self.0.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AgentAdapterError::Cleanup(
                "provider absence not verified".into(),
            ));
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
struct NoGit;
impl RepositoryInspector for NoGit {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        panic!("no Git side effects")
    }
    fn head_commit(&self, _: &Path) -> Result<String, RegistrationError> {
        panic!("no Git side effects")
    }
}
struct Fixture {
    path: PathBuf,
    task: u64,
    cleanup_safe: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "forge-hold-replacement-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteProjectRepository::open(&path).unwrap();
        support::register_project(&store, "alpha");
        for kind in [RecordType::Task, RecordType::Grilling] {
            forge_app::WorkspaceDispatchRepository::select_workspace_profile(
                &store,
                "test",
                kind,
                Some(1),
            )
            .unwrap();
        }
        forge_app::WorkspaceDispatchRepository::retain_verified_workspace_binding(
            &store,
            "test",
            Some(&forge_app::DispatchWorkspace {
                id: "operator".into(),
            }),
            0,
        )
        .unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "private task instructions".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "old".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        store.begin_agent_launch("old", 1000).unwrap();
        assert!(!store.guard_agent_provisioning("old", 301_000).unwrap());
        Self {
            path,
            task: task.id,
            cleanup_safe: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
    fn store(&self) -> SqliteProjectRepository {
        SqliteProjectRepository::open(&self.path).unwrap()
    }
    fn app(&self) -> Application {
        Application::with_agent_dispatch(
            Box::new(self.store()),
            Box::new(self.store()),
            Box::new(Provider(self.cleanup_safe.clone())),
            Box::new(NoGit),
        )
    }
    fn scan(&self, now: u64) {
        let result = self
            .app()
            .reconcile_agent_dispatch("test-daemon", now)
            .unwrap();
        assert!(result.acquired_lease);
        assert_eq!(self.store().list_open_agent_assignments().unwrap().len(), 1);
        assert_eq!(
            self.store().agent_assignment("old").unwrap().attempt_number,
            1
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[tokio::test]
async fn resume_and_restart_do_not_relaunch_and_late_ack_can_work_under_hold() {
    let f = Fixture::new();
    for now in [302_000, 340_000] {
        AgentDispatchApiServer::resume(&Daemon::new(f.app()), RecordIdRequest { id: f.task })
            .await
            .unwrap();
        assert!(f.store().dispatch_hold(f.task).unwrap().is_none());
        // Fresh application and fresh SQLite connections simulate restart on each scan.
        f.scan(now);
        assert!(f.store().dispatch_hold(f.task).unwrap().is_none());
    }
    let app = f.app();
    app.acknowledge_assignment("old", "late-ack").unwrap();
    assert_eq!(
        app.start_acknowledged_assignment_task("old", "late-start", f.task)
            .unwrap()
            .status,
        RecordStatus::InProgress
    );
    assert!(f.store().dispatch_hold(f.task).unwrap().is_none());
    f.scan(380_000);
    AgentDispatchApiServer::resume(&Daemon::new(f.app()), RecordIdRequest { id: f.task })
        .await
        .unwrap();
    f.scan(420_000);
    assert!(f.store().dispatch_hold(f.task).unwrap().is_none());
    assert!(
        f.store()
            .agent_assignment_info("old")
            .unwrap()
            .capacity_reserved()
    );
}

#[test]
fn concurrent_resume_late_acknowledgement_and_scan_never_duplicate_the_attempt() {
    // Each round uses a new persisted store and three independent applications.
    // Do not retry/ignore an operation failure: every competing call must succeed.
    for _ in 0..64 {
        race_resume_late_acknowledgement_and_scan();
    }
}

fn race_resume_late_acknowledgement_and_scan() {
    let f = Fixture::new();
    let barrier = std::sync::Barrier::new(3);
    std::thread::scope(|scope| {
        let resume = scope.spawn(|| {
            let app = f.app();
            barrier.wait();
            app.resume_task_dispatch(f.task).unwrap();
        });
        let acknowledge = scope.spawn(|| {
            let app = f.app();
            barrier.wait();
            app.acknowledge_assignment("old", "racing-ack").unwrap();
            app.start_acknowledged_assignment_task("old", "racing-start", f.task)
                .unwrap();
        });
        let scan = scope.spawn(|| {
            let app = f.app();
            barrier.wait();
            app.reconcile_agent_dispatch("racing-daemon", 302_000)
                .unwrap();
        });
        resume.join().unwrap();
        acknowledge.join().unwrap();
        scan.join().unwrap();
    });
    assert_eq!(
        f.store().show_record(f.task).unwrap().status,
        RecordStatus::InProgress
    );
    let open = f.store().list_open_agent_assignments().unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, "old");
    assert_eq!(open[0].attempt_number, 1);
    assert_eq!(open[0].state, forge_app::AgentAssignmentState::Working);
    let info = f.store().agent_assignment_info("old").unwrap();
    assert!(info.acknowledged_at_millis.is_some());
    assert!(info.capacity_reserved());
    // Retrying the public start is idempotent; no second transition is recorded.
    assert_eq!(
        f.app()
            .start_acknowledged_assignment_task("old", "repeated-start", f.task)
            .unwrap()
            .status,
        RecordStatus::InProgress
    );
    assert_eq!(
        f.store()
            .list_record_updates(f.task)
            .unwrap()
            .iter()
            .filter(|update| matches!(
                update.kind,
                forge_app::RecordUpdateKind::StatusTransition {
                    to: RecordStatus::InProgress,
                    ..
                }
            ))
            .count(),
        1
    );
    assert!(
        f.store()
            .claim_next_agent_assignment("alpha", "duplicate")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn replacement_requires_revocation_all_resource_resolution_and_separate_resume() {
    let f = Fixture::new();
    f.store()
        .add_agent_assignment_resource(&NewAgentAssignmentResource {
            assignment_id: "old".into(),
            resource_kind: "pane".into(),
            resource_id: "known-pane".into(),
        })
        .unwrap();
    let retire = || RetireAgentAssignmentRequest {
        assignment_id: "old".into(),
    };
    assert!(
        AgentDispatchApiServer::assignment_retire(&Daemon::new(f.app()), retire())
            .await
            .is_err()
    );
    let closed = f.store().agent_assignment_info("old").unwrap();
    assert!(closed.assignment.closed_at_millis.is_some());
    assert!(closed.capacity_reserved());
    AgentDispatchApiServer::resume(&Daemon::new(f.app()), RecordIdRequest { id: f.task })
        .await
        .unwrap();
    assert!(
        f.store()
            .claim_next_agent_assignment("alpha", "unsafe-replacement")
            .unwrap()
            .is_none()
    );

    AgentDispatchApiServer::assignment_resolve_unknown(
        &Daemon::new(f.app()),
        forge_rpc::ResolveUnknownAgentResourcesRequest {
            assignment_id: "old".into(),
            confirm_old_agent_cannot_act: true,
            confirm_unknown_resources_safe: true,
        },
    )
    .await
    .unwrap();
    // The confirmation cannot retire known resources or interrupted-cleanup evidence.
    assert!(
        f.store()
            .agent_assignment_info("old")
            .unwrap()
            .capacity_reserved()
    );
    assert!(
        f.store()
            .claim_next_agent_assignment("alpha", "still-unsafe")
            .unwrap()
            .is_none()
    );
    f.cleanup_safe
        .store(true, std::sync::atomic::Ordering::SeqCst);
    AgentDispatchApiServer::assignment_retire(&Daemon::new(f.app()), retire())
        .await
        .unwrap();
    assert!(
        !f.store()
            .agent_assignment_info("old")
            .unwrap()
            .capacity_reserved()
    );
    assert!(f.store().dispatch_hold(f.task).unwrap().is_some());
    assert!(
        f.store()
            .claim_next_agent_assignment("alpha", "held-replacement")
            .unwrap()
            .is_none()
    );
    AgentDispatchApiServer::resume(&Daemon::new(f.app()), RecordIdRequest { id: f.task })
        .await
        .unwrap();

    // Independent reopened connections race for the one now-safe slot.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|i| {
            let path = f.path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let store = SqliteProjectRepository::open(&path).unwrap();
                barrier.wait();
                store
                    .claim_next_agent_assignment("alpha", &format!("replacement-{i}"))
                    .unwrap()
            })
        })
        .collect();
    let claimed: Vec<_> = handles
        .into_iter()
        .filter_map(|h| h.join().unwrap())
        .collect();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].record_id, f.task);
    assert_eq!(claimed[0].attempt_number, 2);
    assert_eq!(f.store().list_open_agent_assignments().unwrap().len(), 1);
}
