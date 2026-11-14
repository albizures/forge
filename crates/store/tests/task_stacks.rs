use forge_app::{
    AgentDispatchRepository, Application, DispatchWorkspace, ManagedWorktreeAdapter,
    ManagedWorktreeRepository, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordStatus, RecordType, RegistrationError, WorkspaceDispatchRepository, WorkspaceRepository,
    WorktreeSafetyAssessment,
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
    checks::Verdict,
    outcomes::{OutcomeDisposition, OutcomeInput},
    stacks::{PullRequestObservation, StackDestination, StackHosting, StackSource, task_branch},
};
use forge_store::SqliteProjectRepository;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::AtomicBool},
};
#[path = "support/task_attempt_adapters.rs"]
mod adapters;

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Checkout;
impl ManagedWorktreeAdapter for Checkout {
    fn create(&self, _: &Path, path: &Path, _: &str, _: &str) -> Result<(), RegistrationError> {
        std::fs::create_dir_all(path).unwrap();
        Ok(())
    }
    fn remove(&self, _: &Path, _: bool) -> Result<(), RegistrationError> {
        unreachable!()
    }
    fn assess(&self, path: &Path) -> WorktreeSafetyAssessment {
        adapters::Local.assess(path)
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
#[derive(Clone)]
struct Hosting(Arc<Mutex<Vec<PullRequestObservation>>>);
impl StackHosting for Hosting {
    fn fetch_branch(
        &self,
        _: &Path,
        _: &StackDestination,
        branch: &str,
    ) -> Result<String, RegistrationError> {
        if branch == "feature/payments" {
            return Ok("b".repeat(40));
        }
        self.0
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.head_branch == branch)
            .map(|p| p.head_commit.clone())
            .ok_or_else(|| RegistrationError::new("test", "missing branch"))
    }
    fn pull_request(
        &self,
        _: &Path,
        _: &str,
        url: &str,
    ) -> Result<PullRequestObservation, RegistrationError> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.url == url)
            .cloned()
            .ok_or_else(|| RegistrationError::new("test", "missing PR"))
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn dispatched_stack_restarts_between_checks_and_completes_only_after_bound_publication() {
    let root = std::env::temp_dir().join(format!(
        "forge-stack-dispatch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fixture = Fixture(root.clone());
    std::fs::create_dir_all(root.join(".agents/profiles")).unwrap();
    std::fs::create_dir_all(root.join("secrets")).unwrap();
    std::fs::write(root.join(".agents/profiles/work.md"), "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\ntools: [read, bash]\n---\nDeliver the bound Check.").unwrap();
    let path = root.join("store.sqlite");
    let store = SqliteProjectRepository::open(&path).unwrap();
    store.create_workspace("test", "Test", &root).unwrap();
    store
        .register(&Project {
            reference: "test/api".into(),
            workspace_id: "test".into(),
            slug: "api".into(),
            project_root: root.clone(),
            origin_url: "https://github.com/acme/widgets.git".into(),
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
        .select_workspace_profile("test", RecordType::Task, Some(profile.revision))
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
                message: "Pin implementation and publication".into(),
                criteria: ["Implementation", "Publication"]
                    .map(|name| CheckCriterion {
                        name: name.into(),
                        description: name.into(),
                        profile_revision: Some(profile.revision),
                    })
                    .into(),
            },
        )
        .unwrap();
    let tasks: Vec<_> = (0..2)
        .map(|_| {
            let task = store
                .open_typed_record(&NewTypedRecord {
                    record_type: RecordType::Task,
                    project_slugs: vec!["test/api".into()],
                    body: "Deliver a slice".into(),
                    generated_by_ids: vec![],
                })
                .unwrap();
            store
                .set_task_base_commit(task.id, &"c".repeat(40))
                .unwrap();
            task.id
        })
        .collect();
    store
        .configure_task_stack(
            tasks[0],
            &StackSource::Root {
                destination: StackDestination {
                    repository: "acme/widgets".into(),
                    remote: "origin".into(),
                    branch: "feature/payments".into(),
                },
            },
        )
        .unwrap();
    store
        .configure_task_stack(
            tasks[1],
            &StackSource::Predecessor {
                record_id: tasks[0],
            },
        )
        .unwrap();
    let prs: Vec<_> = tasks
        .iter()
        .enumerate()
        .map(|(index, id)| PullRequestObservation {
            repository: "acme/widgets".into(),
            url: format!("https://github.com/acme/widgets/pull/{id}"),
            head_branch: task_branch(*id),
            head_commit: "a".repeat(40),
            base_branch: if index == 0 {
                "feature/payments".into()
            } else {
                task_branch(tasks[0])
            },
            open: true,
            draft: false,
            head_repository: "acme/widgets".into(),
        })
        .collect();
    let hosting = Hosting(Arc::new(Mutex::new(prs.clone())));
    let launches = Arc::new(Mutex::new(Vec::new()));
    let application = || {
        Application::with_managed_worktrees_and_agent_dispatch(
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(Checkout),
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(adapters::Provider {
                unavailable: Arc::new(AtomicBool::new(false)),
                launches: launches.clone(),
            }),
            Box::new(adapters::Local),
        )
        .with_workspace_repository(Box::new(SqliteProjectRepository::open(&path).unwrap()))
        .with_stack_hosting(Box::new(hosting.clone()))
    };

    for (index, task) in tasks.iter().enumerate() {
        if store.worktree_claim(*task).unwrap().is_none() {
            application().prepare_dispatch_worktree(*task).unwrap();
        }
        assert_eq!(
            store.task_base_commit(*task).unwrap(),
            Some(if index == 0 {
                "b".repeat(40)
            } else {
                "a".repeat(40)
            })
        );
        for position in 0..2 {
            // Recompose the daemon for every attempt: pins and publication survive restart.
            let app = application();
            let (assignment, _) = app.execute_task_check_acceptance(*task).unwrap();
            let attempt = store
                .assignment_check_attempt(&assignment.id)
                .unwrap()
                .unwrap();
            std::fs::write(root.join("secrets").join(&assignment.id), "secret").unwrap();
            let secret_root = root.join("secrets");
            let session = app
                .assignment(&secret_root, &assignment.id, "secret")
                .unwrap();
            session.acknowledge().unwrap();
            session.start().unwrap();
            session.context().unwrap();
            assert_eq!(session.stack(None).unwrap().unwrap().record_id, *task);
            let outcome = OutcomeInput {
                attempt_id: attempt.id,
                generation: attempt.generation,
                evidence: attempt.evidence,
                verdict: Verdict::Pass,
                actor: "ignored".into(),
                message: "Checked".into(),
                disposition: OutcomeDisposition::Complete,
                findings: vec![],
                verifications: vec![],
            };
            if position == 0 {
                assert!(session.stack(Some(&prs[index].url)).is_err());
            } else {
                assert_eq!(
                    store.show_record(*task).unwrap().status,
                    RecordStatus::InProgress
                );
                assert_eq!(
                    session.submit_outcome(&outcome).unwrap_err().code,
                    "stack-blocked"
                );
                // An operator cannot inject publication while an Assignment owns the Task.
                assert!(
                    app.publish_task_stack(*task, &prs[index].url, None)
                        .is_err()
                );
                session.stack(Some(&prs[index].url)).unwrap();
            }
            let accepted = session.submit_outcome(&outcome).unwrap();
            assert_eq!(session.submit_outcome(&outcome).unwrap(), accepted);
            assert_eq!(
                store.show_record(*task).unwrap().status,
                RecordStatus::InProgress
            );
        }
        application()
            .reconcile_agent_dispatch("stack-restart", 800_000)
            .unwrap();
        assert_eq!(store.show_record(*task).unwrap().status, RecordStatus::Done);
        let publication = store
            .task_stack(*task)
            .unwrap()
            .unwrap()
            .publication
            .unwrap();
        assert_eq!(publication.url, prs[index].url);
        assert_eq!(publication.base_branch, prs[index].base_branch);
    }
    assert_eq!(launches.lock().unwrap().len(), 4);
    drop(store);
    drop(fixture);
}
