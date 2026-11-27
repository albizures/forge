//! Shared Implementation Check fixture: a launched Task with a controllable HEAD and dirty flag.
#![allow(dead_code)]
use forge_app::{
    Application, DispatchWorkspace, ManagedWorktreeAdapter,
    ManagedWorktreeRepository, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordType, RegistrationError, RepositoryInspector, WorkspaceDispatchRepository,
    WorkspaceRepository, WorktreeSafetyAssessment,
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
    outcomes::Evidence,
};
use forge_store::SqliteProjectRepository;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
#[path = "task_attempt_adapters.rs"]
mod adapters;
pub use adapters::{Local, Provider};

pub struct Head(Arc<Mutex<String>>);
impl RepositoryInspector for Head {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        unreachable!()
    }
    fn head_commit(&self, _: &Path) -> Result<String, RegistrationError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

pub struct Checkout(Arc<AtomicBool>);
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

pub struct Fixture {
    pub root: PathBuf,
    pub store: SqliteProjectRepository,
    pub task: u64,
    pub head: Arc<Mutex<String>>,
    pub dirty: Arc<AtomicBool>,
    pub launches: Arc<Mutex<Vec<u32>>>,
    pub path: PathBuf,
    pub secrets: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        Self::with_profile_body("Work")
    }

    pub fn with_profile_body(profile_body: &str) -> Self {
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
            format!("---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: [read]\n---\n{profile_body}"),
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

    pub fn app(&self) -> Application {
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
    pub fn launch(&self, app: &Application) -> String {
        let (assignment, _) = app.execute_task_check_acceptance(self.task).unwrap();
        std::fs::create_dir_all(&self.secrets).unwrap();
        std::fs::write(self.secrets.join(&assignment.id), "secret").unwrap();
        let session = self.session(app, &assignment.id);
        session.acknowledge().unwrap();
        session.start().unwrap();
        session.context().unwrap();
        assignment.id
    }

    pub fn raise_finding(&self, message: &str) -> u64 {
        self.app()
            .raise_record_finding(
                self.task,
                &forge_app::findings::FindingRaiseInput {
                    severity: forge_app::findings::Severity::Major,
                    blocking: Some(false),
                    check_id: None,
                    assessment_id: None,
                    actor: "reviewer".into(),
                    message: message.into(),
                    revision: None,
                    dirty: None,
                },
            )
            .unwrap()
            .id
    }

    pub fn acknowledged(&self, app: &Application) -> String {
        let (assignment, _) = app.execute_task_check_acceptance(self.task).unwrap();
        std::fs::create_dir_all(&self.secrets).unwrap();
        std::fs::write(self.secrets.join(&assignment.id), "secret").unwrap();
        assignment.id
    }

    pub fn session<'a>(
        &'a self,
        app: &'a Application,
        id: &'a str,
    ) -> forge_app::assignment::AuthenticatedAssignment<'a> {
        app.assignment(&self.secrets, id, "secret").unwrap()
    }

    pub fn set_head(&self, commit: &str) {
        *self.head.lock().unwrap() = commit.into();
    }

    pub fn evidence(&self, commit: &str) -> Evidence {
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