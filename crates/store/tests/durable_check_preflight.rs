//! Controlled application/store dispatch only; no runner/model execution is claimed.
use forge_app::{
    AgentAdapterError, AgentAssignment, AgentDispatchAdapter, AgentDispatchAdapterFactory,
    AgentDispatchContext, AgentDispatchRepository, AgentObservation, AgentProfile, Application,
    CheckDispatchPreflight, DispatchPreflightSelection, DispatchWorkspace,
    ManagedWorktreeRepository, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordType, WorkspaceDispatchRepository, WorkspaceRepository,
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
    outcomes::Evidence,
};
use forge_store::SqliteProjectRepository;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
#[path = "support/task_attempt_adapters.rs"]
mod adapters;

struct Fixture {
    root: PathBuf,
    store: SqliteProjectRepository,
    task: u64,
    check: u64,
    revision: u32,
}
impl Fixture {
    fn new(runtime: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forge-check-preflight-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join(".agents/profiles")).unwrap();
        std::fs::write(root.join(".agents/profiles/work.md"), format!("---\nversion: 1\nruntime: {runtime}\nmodel: openai/gpt-4.1\nthinkingLevel: off\ntools: [read]\n---\nWork")).unwrap();
        let store = SqliteProjectRepository::open(&root.join("store.sqlite")).unwrap();
        store.create_workspace("test", "Test", &root).unwrap();
        store
            .register(&Project {
                reference: "test/api".into(),
                workspace_id: "test".into(),
                slug: "api".into(),
                project_root: root.clone(),
                origin_url: "https://example.test/api.git".into(),
            })
            .unwrap();
        let policy = ResourceCatalog::discover(&root.join("operator"), &root)
            .unwrap()
            .resolve(&root, ".agents/profiles/work.md")
            .unwrap();
        let profile = store
            .register_agent_profile("test", RecordType::Task, &policy)
            .unwrap();
        store
            .select_workspace_profile("test", RecordType::Task, None)
            .unwrap();
        store
            .replace_check_template(
                "test/api",
                &TemplateReplacement {
                    expected_version: 0,
                    actor: "operator".into(),
                    message: "Pin".into(),
                    criteria: vec![CheckCriterion {
                        name: "Implementation".into(),
                        description: "Deliver".into(),
                        profile_revision: Some(profile.revision),
                    }],
                },
            )
            .unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                project_slugs: vec!["test/api".into()],
                body: "Deliver".into(),
                generated_by_ids: vec![],
            })
            .unwrap();
        let check = store.list_record_checks(task.id).unwrap()[0].id;
        store
            .set_task_base_commit(task.id, &"a".repeat(40))
            .unwrap();
        store.reserve_worktree_claim(task.id).unwrap();
        store.activate_worktree_claim(task.id).unwrap();
        Self {
            root,
            store,
            task: task.id,
            check,
            revision: profile.revision,
        }
    }
    fn evidence(&self) -> Evidence {
        Evidence::Repository {
            repository: self.root.display().to_string(),
            commit: "a".repeat(40),
        }
    }
    fn claim(
        &self,
        selection: &CheckDispatchPreflight,
    ) -> Result<(AgentAssignment, bool), forge_app::RegistrationError> {
        self.store.claim_task_check_acceptance(
            self.task,
            "controlled-check",
            selection,
            &self.evidence(),
            self.check,
            self.revision,
            None,
        )
    }
    fn app(&self, factory: Box<dyn AgentDispatchAdapterFactory>) -> Application {
        let db = self.root.join("store.sqlite");
        Application::with_managed_worktrees_and_agent_dispatch(
            Box::new(SqliteProjectRepository::open(&db).unwrap()),
            Box::new(SqliteProjectRepository::open(&db).unwrap()),
            Box::new(adapters::Local),
            Box::new(SqliteProjectRepository::open(&db).unwrap()),
            factory,
            Box::new(adapters::Local),
        )
        .with_workspace_repository(Box::new(SqliteProjectRepository::open(&db).unwrap()))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct DurableOnly(Arc<Mutex<Vec<PathBuf>>>);
impl AgentDispatchAdapterFactory for DurableOnly {
    fn validate_profile(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        assert_eq!(profile.provider, "pi-durable");
        Ok(())
    }
    fn adapter_for(
        &self,
        profile: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        self.validate_profile(profile)?;
        Ok(Box::new(Self(self.0.clone())))
    }
}
impl AgentDispatchAdapter for DurableOnly {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        Ok(())
    }
    fn verify_workspace(&self, _: &str) -> Result<DispatchWorkspace, AgentAdapterError> {
        panic!("durable dispatch must not verify Herdr")
    }
    fn provision(
        &self,
        context: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        assert!(context.dispatch_workspace.is_none());
        self.0
            .lock()
            .unwrap()
            .push(context.agent_workspace.clone().unwrap());
        Ok(())
    }
    fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        Ok(AgentObservation::Working)
    }
    fn recover(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        panic!("recovery not enabled")
    }
    fn retire(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        panic!("retirement not exercised")
    }
}

#[test]
fn durable_pinned_check_dispatch_needs_no_workspace_profiles_or_herdr_binding() {
    let f = Fixture::new("pi-durable");
    let launches = Arc::new(Mutex::new(vec![]));
    let app = f.app(Box::new(DurableOnly(launches.clone())));
    let (assignment, existing) = app.dispatch_record(f.task).unwrap();
    assert!(!existing);
    assert_eq!(assignment.profile.revision, f.revision);
    assert_eq!(
        *launches.lock().unwrap(),
        vec![f.store.worktree_claim(f.task).unwrap().unwrap().path]
    );
    assert_eq!(app.dispatch_record(f.task).unwrap(), (assignment, true));
    assert_eq!(launches.lock().unwrap().len(), 1);
    assert_eq!(
        f.store
            .assignment_check_attempt(&f.store.list_agent_assignments(f.task).unwrap()[0].id)
            .unwrap()
            .unwrap()
            .check_id,
        f.check
    );
    assert!(
        f.store.list_record_checks(f.task).unwrap()[0]
            .current_verdict
            .is_none()
    );
}

#[test]
fn preflight_backend_is_bound_to_the_immutable_check_pin() {
    for (runtime, token) in [
        ("herdr/pi", CheckDispatchPreflight::PiDurable),
        (
            "pi-durable",
            CheckDispatchPreflight::Herdr(DispatchPreflightSelection {
                task_revision: 1,
                grilling_revision: 1,
                binding_id: "invented".into(),
                provider_generation: 0,
            }),
        ),
    ] {
        let f = Fixture::new(runtime);
        assert_eq!(
            f.claim(&token).unwrap_err().code,
            "dispatch-preflight-stale"
        );
        assert!(f.store.list_agent_assignments(f.task).unwrap().is_empty());
        assert!(
            f.store
                .assignment_check_attempt("controlled-check")
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn durable_preflight_does_not_bypass_manual_dependency_or_parent_gates() {
    for gate in ["manual", "dependency", "parent"] {
        let f = Fixture::new("pi-durable");
        let launches = Arc::new(Mutex::new(vec![]));
        let app = f.app(Box::new(DurableOnly(launches.clone())));
        let (task, expected) = match gate {
            "manual" => {
                f.store
                    .set_record_execution_mode(
                        f.task,
                        forge_app::ExecutionMode::Dispatched,
                        forge_app::ExecutionMode::Manual,
                        "operator",
                        "Manual work",
                    )
                    .unwrap();
                (f.task, "manual-dispatch-forbidden")
            }
            "dependency" => {
                let prerequisite = f
                    .store
                    .open_typed_record(&NewTypedRecord {
                        record_type: RecordType::Task,
                        project_slugs: vec!["test/api".into()],
                        body: "Prerequisite".into(),
                        generated_by_ids: vec![],
                    })
                    .unwrap();
                app.relate_records(
                    f.task,
                    prerequisite.id,
                    forge_app::RecordRelationshipKind::WaitsFor,
                )
                .unwrap();
                (f.task, "record-not-ready")
            }
            "parent" => {
                let spec = f
                    .store
                    .open_typed_record(&NewTypedRecord {
                        record_type: RecordType::Spec,
                        project_slugs: vec!["test/api".into()],
                        body: "Planning only".into(),
                        generated_by_ids: vec![],
                    })
                    .unwrap();
                let child = f
                    .store
                    .open_task_under_spec("Child", spec.id, vec![])
                    .unwrap();
                (child.id, "spec-execution-blocked")
            }
            _ => unreachable!(),
        };
        assert_eq!(app.dispatch_record(task).unwrap_err().code, expected);
        let check = f.store.list_record_checks(task).unwrap()[0].id;
        assert_eq!(
            f.store
                .claim_task_check_acceptance(
                    task,
                    "denied",
                    &CheckDispatchPreflight::PiDurable,
                    &f.evidence(),
                    check,
                    f.revision,
                    None
                )
                .unwrap_err()
                .code,
            expected
        );
        assert!(launches.lock().unwrap().is_empty());
        assert!(f.store.list_agent_assignments(task).unwrap().is_empty());
    }
}

#[test]
fn herdr_pins_still_require_verified_workspace_configuration() {
    let f = Fixture::new("herdr/pi");
    let launches = Arc::new(Mutex::new(vec![]));
    let app = f.app(Box::new(adapters::Provider {
        unavailable: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        launches: launches.clone(),
    }));
    assert_eq!(
        app.dispatch_record(f.task).unwrap_err().code,
        "workspace-dispatch-incomplete"
    );
    assert!(launches.lock().unwrap().is_empty());
    let token = CheckDispatchPreflight::Herdr(DispatchPreflightSelection {
        task_revision: f.revision,
        grilling_revision: 1,
        binding_id: "invented".into(),
        provider_generation: 0,
    });
    assert_eq!(
        f.claim(&token).unwrap_err().code,
        "dispatch-preflight-stale"
    );
    assert!(f.store.list_agent_assignments(f.task).unwrap().is_empty());
}
