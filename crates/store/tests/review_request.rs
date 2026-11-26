//! Review Request: an Implementation Agent's only way to finish, with no verdict of its own.
use forge_app::{
    AgentDispatchRepository, Application, DispatchWorkspace, ManagedWorktreeAdapter,
    ManagedWorktreeRepository, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordType, RegistrationError, RepositoryInspector, WorkspaceDispatchRepository,
    WorkspaceRepository, WorktreeSafetyAssessment,
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
    checks::Verdict,
    outcomes::{Evidence, OutcomeDisposition, OutcomeInput},
};
use forge_store::SqliteProjectRepository;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
#[path = "support/task_attempt_adapters.rs"]
mod adapters;
use adapters::{Local, Provider};

struct Head(Arc<Mutex<String>>);
impl RepositoryInspector for Head {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        unreachable!()
    }
    fn head_commit(&self, _: &Path) -> Result<String, RegistrationError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

struct Checkout(Arc<AtomicBool>);
impl ManagedWorktreeAdapter for Checkout {
    fn validate_recovery(
        &self,
        a: &Path,
        b: &Path,
        c: &str,
        d: &str,
    ) -> Result<(), RegistrationError> {
        Local.validate_recovery(a, b, c, d)
    }
    fn create(&self, _: &Path, _: &Path, _: &str, _: &str) -> Result<(), RegistrationError> {
        unreachable!()
    }
    fn remove(&self, _: &Path, _: bool) -> Result<(), RegistrationError> {
        unreachable!()
    }
    fn assess(&self, path: &Path) -> WorktreeSafetyAssessment {
        let mut assessment = Local.assess(path);
        if self.0.load(Ordering::SeqCst) {
            assessment.facts.push("dirty".into());
            assessment.removable = false;
        }
        assessment
    }
    fn run_setup_hook(
        &self,
        _: &Path,
        _: &str,
        _: &str,
        _: u64,
        _: &str,
        _: &str,
    ) -> Result<(), RegistrationError> {
        unreachable!()
    }
}

struct Fixture {
    root: PathBuf,
    store: SqliteProjectRepository,
    task: u64,
    head: Arc<Mutex<String>>,
    dirty: Arc<AtomicBool>,
    launches: Arc<Mutex<Vec<u32>>>,
    path: PathBuf,
    secrets: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forge-review-request-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".agents/skills/work")).unwrap();
        std::fs::write(
            root.join(".agents/skills/work/SKILL.md"),
            "---\nname: work\ndescription: work\n---\nWork",
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".agents/profiles")).unwrap();
        std::fs::write(
            root.join(".agents/profiles/work.md"),
            "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: [read]\n---\nWork",
        )
        .unwrap();
        let path = root.join("store.sqlite");
        let store = SqliteProjectRepository::open(&path).unwrap();
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
        let pinned = store
            .register_agent_profile("test", RecordType::Task, &policy)
            .unwrap();
        store
            .select_workspace_profile("test", RecordType::Task, Some(pinned.revision))
            .unwrap();
        store
            .select_workspace_profile("test", RecordType::Grilling, Some(1))
            .unwrap();
        let generation = store
            .workspace_dispatch_configuration("test")
            .unwrap()
            .provider_generation;
        store
            .retain_verified_workspace_binding(
                "test",
                Some(&DispatchWorkspace {
                    id: "provider".into(),
                }),
                generation,
            )
            .unwrap();
        store
            .replace_check_template(
                "test/api",
                &TemplateReplacement {
                    expected_version: 0,
                    actor: "operator".into(),
                    message: "Pin".into(),
                    criteria: ["Implementation", "Review"]
                        .map(|name| CheckCriterion {
                            name: name.into(),
                            description: name.into(),
                            profile_revision: Some(pinned.revision),
                        })
                        .into(),
                },
            )
            .unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                project_slugs: vec!["test/api".into()],
                body: "Deliver work".into(),
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .set_task_base_commit(task.id, &"a".repeat(40))
            .unwrap();
        store.reserve_worktree_claim(task.id).unwrap();
        store.activate_worktree_claim(task.id).unwrap();
        Self {
            store,
            task: task.id,
            head: Arc::new(Mutex::new("a".repeat(40))),
            dirty: Arc::new(AtomicBool::new(false)),
            launches: Arc::new(Mutex::new(Vec::new())),
            secrets: root.join("secrets"),
            path,
            root,
        }
    }

    fn app(&self) -> Application {
        Application::with_managed_worktrees_and_agent_dispatch(
            Box::new(SqliteProjectRepository::open(&self.path).unwrap()),
            Box::new(SqliteProjectRepository::open(&self.path).unwrap()),
            Box::new(Checkout(self.dirty.clone())),
            Box::new(SqliteProjectRepository::open(&self.path).unwrap()),
            Box::new(Provider {
                unavailable: Arc::new(AtomicBool::new(false)),
                launches: self.launches.clone(),
            }),
            Box::new(Head(self.head.clone())),
        )
        .with_workspace_repository(Box::new(SqliteProjectRepository::open(&self.path).unwrap()))
    }

    /// Launch the next Check's Assignment and run its acknowledge/start bootstrap.
    fn launch(&self, app: &Application) -> String {
        let (assignment, _) = app.execute_task_check_acceptance(self.task).unwrap();
        std::fs::create_dir_all(&self.secrets).unwrap();
        std::fs::write(self.secrets.join(&assignment.id), "secret").unwrap();
        let session = self.session(app, &assignment.id);
        session.acknowledge().unwrap();
        session.start().unwrap();
        session.context().unwrap();
        assignment.id
    }

    fn session<'a>(
        &'a self,
        app: &'a Application,
        id: &'a str,
    ) -> forge_app::assignment::AuthenticatedAssignment<'a> {
        app.assignment(&self.secrets, id, "secret").unwrap()
    }

    fn set_head(&self, commit: &str) {
        *self.head.lock().unwrap() = commit.into();
    }

    fn evidence(&self, commit: &str) -> Evidence {
        Evidence::Repository {
            repository: self.root.display().to_string(),
            commit: commit.into(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn accepted_review_request_records_the_observed_head_and_hands_off_to_review() {
    let f = Fixture::new();
    let app = f.app();
    let id = f.launch(&app);
    let session = f.session(&app, &id);
    let committed = "b".repeat(40);
    f.set_head(&committed);

    let accepted = session.request_review().unwrap();
    let implementation = f.store.list_record_checks(f.task).unwrap().remove(0);
    let assessment = implementation.latest_assessment.as_ref().unwrap();
    assert_eq!(assessment.verdict, Verdict::Pass);
    assert_eq!(assessment.assignment_id.as_deref(), Some(id.as_str()));
    assert_eq!(accepted.attempt.id, id);
    assert_eq!(accepted.evidence, f.evidence(&committed));
    assert_eq!(accepted.work.output, Some(f.evidence(&committed)));

    // An exact repeat returns the stored result.
    assert_eq!(session.request_review().unwrap(), accepted);

    // Reconciliation retires the Implementation Assignment and launches review.
    assert_eq!(f.launches.lock().unwrap().len(), 1);
    app.reconcile_agent_dispatch("review-request", 100_000)
        .unwrap();
    assert!(
        f.store
            .agent_assignment(&id)
            .unwrap()
            .closed_at_millis
            .is_some()
    );
    assert_eq!(f.launches.lock().unwrap().len(), 2);
    let review = f
        .store
        .list_agent_assignments(f.task)
        .unwrap()
        .into_iter()
        .find(|a| a.closed_at_millis.is_none())
        .unwrap();
    let checks = f.store.list_record_checks(f.task).unwrap();
    assert_eq!(
        f.store
            .assignment_check_attempt(&review.id)
            .unwrap()
            .unwrap()
            .check_id,
        checks[1].id
    );
    // The retired Assignment's replay still answers, even once the worktree has moved on.
    f.set_head(&"c".repeat(40));
    assert_eq!(session.request_review().unwrap(), accepted);
}

#[test]
fn dirty_worktree_rejects_without_recording_anything() {
    let f = Fixture::new();
    let app = f.app();
    let id = f.launch(&app);
    let session = f.session(&app, &id);
    f.set_head(&"b".repeat(40));
    let work = app.record_work_state(f.task).unwrap();
    let checks = f.store.list_record_checks(f.task).unwrap();
    let updates = f.store.list_record_updates(f.task).unwrap();

    f.dirty.store(true, Ordering::SeqCst);
    let error = session.request_review().unwrap_err();
    assert_eq!(error.code, "review-request-worktree-invalid");
    assert!(error.message.contains("commit"), "{}", error.message);
    assert_eq!(app.record_work_state(f.task).unwrap(), work);
    assert_eq!(f.store.list_record_checks(f.task).unwrap(), checks);
    assert_eq!(f.store.list_record_updates(f.task).unwrap(), updates);
    assert!(f.store.dispatch_hold(f.task).unwrap().is_none());
    assert!(
        f.store
            .agent_assignment(&id)
            .unwrap()
            .closed_at_millis
            .is_none()
    );

    // Committing and cleaning up lets the same attempt request review.
    f.dirty.store(false, Ordering::SeqCst);
    session.request_review().unwrap();
}

#[test]
fn review_check_assignment_cannot_request_review() {
    let f = Fixture::new();
    let app = f.app();
    let id = f.launch(&app);
    f.session(&app, &id).request_review().unwrap();
    app.reconcile_agent_dispatch("review-request", 100_000)
        .unwrap();
    let review = f.launch(&app);
    assert_ne!(review, id);
    let before = f.store.list_record_checks(f.task).unwrap();
    let error = f.session(&app, &review).request_review().unwrap_err();
    assert_eq!(error.code, "review-request-not-implementation");
    assert_eq!(f.store.list_record_checks(f.task).unwrap(), before);
}

#[test]
fn review_request_after_another_outcome_conflicts() {
    for (verdict, disposition, message) in [
        (
            Verdict::Pass,
            OutcomeDisposition::Complete,
            "Self-reported pass",
        ),
        (Verdict::Fail, OutcomeDisposition::Interrupted, "Stopped"),
    ] {
        let f = Fixture::new();
        let app = f.app();
        let id = f.launch(&app);
        let session = f.session(&app, &id);
        let attempt = f.store.assignment_check_attempt(&id).unwrap().unwrap();
        session
            .submit_outcome(&OutcomeInput {
                attempt_id: attempt.id,
                generation: attempt.generation,
                evidence: attempt.evidence,
                verdict,
                actor: "ignored".into(),
                message: message.into(),
                disposition,
                findings: vec![],
                verifications: vec![],
            })
            .unwrap();
        assert_eq!(
            session.request_review().unwrap_err().code,
            "outcome-conflict"
        );
    }
}
