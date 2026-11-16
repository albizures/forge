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
struct Head(Arc<Mutex<String>>);
impl forge_app::RepositoryInspector for Head {
    fn inspect(&self, root: &Path) -> Result<Project, RegistrationError> {
        forge_app::RepositoryInspector::inspect(&adapters::Local, root)
    }
    fn head_commit(&self, _: &Path) -> Result<String, RegistrationError> {
        Ok(self.0.lock().unwrap().clone())
    }
}
struct Checkout(Arc<AtomicBool>);
impl ManagedWorktreeAdapter for Checkout {
    fn create(&self, _: &Path, path: &Path, _: &str, _: &str) -> Result<(), RegistrationError> {
        std::fs::create_dir_all(path).unwrap();
        Ok(())
    }
    fn remove(&self, _: &Path, _: bool) -> Result<(), RegistrationError> {
        unreachable!()
    }
    fn assess(&self, path: &Path) -> WorktreeSafetyAssessment {
        if self.0.load(std::sync::atomic::Ordering::Relaxed) {
            WorktreeSafetyAssessment {
                facts: vec!["dirty".into()],
                blockers: vec![],
                removable: false,
            }
        } else {
            adapters::Local.assess(path)
        }
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
struct Hosting(Arc<Mutex<Vec<PullRequestObservation>>>, String);
impl StackHosting for Hosting {
    fn fetch_branch(
        &self,
        _: &Path,
        _: &StackDestination,
        branch: &str,
    ) -> Result<String, RegistrationError> {
        if branch == "feature/payments" {
            return Ok(self.1.clone());
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

fn reject_unpinned_implementation(store: &SqliteProjectRepository, revision: u32) {
    let criteria = |implementation| {
        ["Implementation", "Publication"]
            .map(|name| CheckCriterion {
                name: name.into(),
                description: name.into(),
                profile_revision: if name == "Implementation" {
                    implementation
                } else {
                    Some(revision)
                },
            })
            .into()
    };
    store
        .replace_check_template(
            "test/api",
            &TemplateReplacement {
                expected_version: 1,
                actor: "operator".into(),
                message: "Incomplete plan fixture".into(),
                criteria: criteria(None),
            },
        )
        .unwrap();
    let parent = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Spec,
            project_slugs: vec!["test/api".into()],
            body: "Incomplete pins".into(),
            generated_by_ids: vec![],
        })
        .unwrap();
    let child = store
        .open_task_under_spec("Incomplete implementation pin", parent.id, vec![])
        .unwrap();
    store
        .configure_task_stack(
            child.id,
            &StackSource::Root {
                destination: StackDestination {
                    repository: "acme/widgets".into(),
                    remote: "origin".into(),
                    branch: "feature/payments".into(),
                },
            },
        )
        .unwrap();
    let checks = store.list_record_checks(child.id).unwrap();
    let history = store.list_record_updates(parent.id).unwrap();
    let error = store.start_record(parent.id).unwrap_err();
    assert_eq!(error.code, "invalid-spec-plan");
    assert!(error.message.contains("Implementation"));
    assert!(error.message.contains("pinned profile"));
    assert_eq!(store.list_record_checks(child.id).unwrap(), checks);
    assert_eq!(store.list_record_updates(parent.id).unwrap(), history);
    assert_eq!(
        store.show_record(parent.id).unwrap().status,
        RecordStatus::Open
    );
    // Restoring the template does not repair this Task's immutable pins.
    store
        .replace_check_template(
            "test/api",
            &TemplateReplacement {
                expected_version: 2,
                actor: "operator".into(),
                message: "Restore complete template".into(),
                criteria: criteria(Some(revision)),
            },
        )
        .unwrap();
    assert!(store.start_record(parent.id).is_err());
}

#[test]
fn dispatched_stack_restarts_between_checks_and_completes_only_after_bound_publication() {
    dispatched_publication(false);
}

#[test]
fn dispatched_unchanged_result_restarts_and_requires_bound_final_attempt() {
    dispatched_publication(true);
}

#[allow(clippy::too_many_lines)]
fn dispatched_publication(unchanged: bool) {
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
    std::fs::write(root.join(".agents/profiles/work.md"), "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: medium\ntools: [read, bash]\n---\nDeliver the bound Check.").unwrap();
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
    reject_unpinned_implementation(&store, profile.revision);
    let tasks: Vec<_> = (0..if unchanged { 1 } else { 4 })
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
    for pair in tasks.windows(2) {
        store
            .relate_records(
                pair[1],
                pair[0],
                forge_app::RecordRelationshipKind::BuildsOn,
            )
            .unwrap();
    }
    let prs: Vec<_> = tasks
        .iter()
        .enumerate()
        .map(|(index, id)| PullRequestObservation {
            repository: "acme/widgets".into(),
            url: format!("https://github.com/acme/widgets/pull/{id}"),
            head_branch: task_branch(*id),
            head_commit: if index == 3 { "c" } else { "a" }.repeat(40),
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
    let hosting = Hosting(
        Arc::new(Mutex::new(if unchanged {
            vec![]
        } else {
            vec![prs[0].clone(), prs[3].clone()]
        })),
        if unchanged { "a" } else { "b" }.repeat(40),
    );
    let head = Arc::new(Mutex::new("a".repeat(40)));
    let dirty = Arc::new(AtomicBool::new(false));
    let launches = Arc::new(Mutex::new(Vec::new()));
    let application = || {
        Application::with_managed_worktrees_and_agent_dispatch(
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(Checkout(dirty.clone())),
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(adapters::Provider {
                unavailable: Arc::new(AtomicBool::new(false)),
                launches: launches.clone(),
            }),
            Box::new(Head(head.clone())),
        )
        .with_workspace_repository(Box::new(SqliteProjectRepository::open(&path).unwrap()))
        .with_stack_hosting(Box::new(hosting.clone()))
    };

    for (index, task) in tasks.iter().enumerate() {
        let unchanged_output = unchanged || matches!(index, 1 | 2);
        let output_commit = if index == 3 { "c" } else { "a" }.repeat(40);
        if store.worktree_claim(*task).unwrap().is_none() {
            application().prepare_dispatch_worktree(*task).unwrap();
        }
        assert_eq!(
            store.task_base_commit(*task).unwrap(),
            Some(if index == 0 && !unchanged {
                "b".repeat(40)
            } else {
                "a".repeat(40)
            })
        );
        *head.lock().unwrap() = store.task_base_commit(*task).unwrap().unwrap();
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
            if index > 0 && position == 0 {
                let history = store.list_record_updates(*task).unwrap();
                let retained = hosting.0.lock().unwrap().remove(0);
                assert!(session.start().is_err());
                assert!(app.execute_task_check_acceptance(*task).is_err());
                assert_eq!(store.list_record_updates(*task).unwrap(), history);
                hosting.0.lock().unwrap().insert(0, retained);
            }
            session.start().unwrap();
            session.context().unwrap();
            let stack = session.stack(None).unwrap().unwrap();
            assert_eq!(stack.record_id, *task);
            assert_eq!(stack.predecessor, index.checked_sub(1).map(|i| tasks[i]));
            if index > 1 {
                // Deriving ancestry never widens bounded Record access.
                assert!(session.record(tasks[0]).is_err());
            }
            // Read-only Tasks retain their exact inherited commit; C changes it.
            head.lock().unwrap().clone_from(&output_commit);
            let outcome = OutcomeInput {
                attempt_id: attempt.id,
                generation: attempt.generation,
                evidence: forge_app::outcomes::Evidence::Repository {
                    repository: root.display().to_string(),
                    commit: output_commit.clone(),
                },
                verdict: Verdict::Pass,
                actor: "ignored".into(),
                message: "Checked".into(),
                disposition: OutcomeDisposition::Complete,
                findings: vec![],
                verifications: vec![],
            };
            if position == 0 {
                assert!(session.stack_unchanged().is_err());
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
                if unchanged_output {
                    assert!(app.publish_unchanged_task_stack(*task, None).is_err());
                    dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                    assert!(session.stack_unchanged().is_err());
                    dirty.store(false, std::sync::atomic::Ordering::Relaxed);
                    let saved = session.stack_unchanged().unwrap().unwrap();
                    assert_eq!(session.stack_unchanged().unwrap(), Some(saved));
                } else {
                    if index == 0 {
                        assert!(session.stack_unchanged().is_err());
                    }
                    session.stack(Some(&prs[index].url)).unwrap();
                }
            }
            let accepted = session.submit_outcome(&outcome).unwrap();
            assert_eq!(session.submit_outcome(&outcome).unwrap(), accepted);
            if index > 0 {
                let retained = hosting.0.lock().unwrap().remove(0);
                // An accepted exact replay remains replay-only despite later drift.
                assert_eq!(session.submit_outcome(&outcome).unwrap(), accepted);
                hosting.0.lock().unwrap().insert(0, retained);
            }
            assert_eq!(
                store.show_record(*task).unwrap().status,
                RecordStatus::InProgress
            );
        }
        application()
            .reconcile_agent_dispatch("stack-restart", 800_000)
            .unwrap();
        assert_eq!(store.show_record(*task).unwrap().status, RecordStatus::Done);
        let stack = store.task_stack(*task).unwrap().unwrap();
        if unchanged_output {
            assert!(stack.publication.is_none());
            assert_eq!(
                stack.unchanged_result.unwrap().evidence,
                forge_app::outcomes::Evidence::Repository {
                    repository: root.display().to_string(),
                    commit: "a".repeat(40),
                }
            );
        } else {
            let publication = stack.publication.unwrap();
            assert_eq!(publication.url, prs[index].url);
            assert_eq!(publication.base_branch, prs[index].base_branch);
        }
    }
    assert_eq!(
        launches.lock().unwrap().len(),
        if unchanged { 2 } else { 8 }
    );
    drop(store);
    drop(fixture);
}
