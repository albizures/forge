use forge_app::{
    Application, ExecutionMode, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordType, RegistrationError, RepositoryInspector, WorkspaceRepository,
    check_templates::{CheckCriterion, TemplateReplacement},
    checks::Verdict,
    outcomes::{AttemptInput, Evidence, OutcomeDisposition, OutcomeInput},
    stacks::{
        CheckoutObservation, PullRequestObservation, StackDestination, StackHosting, StackSource,
        task_branch,
    },
};
use forge_store::SqliteProjectRepository;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
#[derive(Clone, Default)]
struct Hosting(Arc<Mutex<HashMap<String, PullRequestObservation>>>);
impl StackHosting for Hosting {
    fn checkout(&self, root: &Path) -> Result<CheckoutObservation, RegistrationError> {
        Ok(CheckoutObservation {
            commit: git(root, &["rev-parse", "HEAD"]),
            clean: git(root, &["status", "--porcelain"]).is_empty(),
        })
    }
    fn fetch_branch(
        &self,
        root: &Path,
        _: &StackDestination,
        branch: &str,
    ) -> Result<String, RegistrationError> {
        // Local-only fake hosting: observe exact Git commits, never fetch real remotes.
        Ok(git(
            root,
            &["rev-parse", &format!("refs/heads/{branch}^{{commit}}")],
        ))
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
            .get(url)
            .cloned()
            .ok_or_else(|| RegistrationError::new("test", "missing PR"))
    }
}
struct Inspector;
impl RepositoryInspector for Inspector {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        unreachable!()
    }
}
fn application(path: &Path, hosting: &Hosting) -> Application {
    Application::new(
        Box::new(SqliteProjectRepository::open(path).unwrap()),
        Box::new(Inspector),
    )
    .with_stack_hosting(Box::new(hosting.clone()))
}
fn pass(store: &SqliteProjectRepository, root: &Path, id: u64, position: usize, commit: &str) {
    let work = store.record_work_state(id).unwrap();
    let evidence = Evidence::Repository {
        repository: root.display().to_string(),
        commit: commit.into(),
    };
    let attempt = AttemptInput {
        id: format!("{id}-{position}"),
        check_id: store.list_record_checks(id).unwrap()[position].id,
        generation: work.generation,
        evidence: if position == 0 {
            work.input.unwrap_or_else(|| Evidence::Repository {
                repository: root.display().to_string(),
                commit: store.task_base_commit(id).unwrap().unwrap(),
            })
        } else {
            evidence.clone()
        },
        actor: "operator".into(),
    };
    store.begin_record_attempt(id, &attempt).unwrap();
    store
        .submit_record_outcome(
            id,
            &OutcomeInput {
                attempt_id: attempt.id,
                generation: work.generation,
                evidence,
                verdict: Verdict::Pass,
                actor: "operator".into(),
                message: "Verified".into(),
                disposition: OutcomeDisposition::Complete,
                findings: vec![],
                verifications: vec![],
            },
        )
        .unwrap();
}

fn fixture() -> (Fixture, SqliteProjectRepository, Vec<String>) {
    let fixture = Fixture(std::env::temp_dir().join(
        format!("forge-cumulative-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
    ));
    let root = fixture.0.join("repository");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-b", "selected"]);
    let mut commits = vec![];
    for message in ["starting input", "first output", "later project head"] {
        git(
            &root,
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "--allow-empty",
                "--no-gpg-sign",
                "-m",
                message,
            ],
        );
        commits.push(git(&root, &["rev-parse", "HEAD"]));
    }
    // Every revision has the same tree: equal contents cannot stand in for exact commits.
    assert_ne!(commits[0], commits[1]);
    assert_eq!(
        git(&root, &["rev-parse", &format!("{}^{{tree}}", commits[0])]),
        git(&root, &["rev-parse", "HEAD^{tree}"])
    );
    git(&root, &["branch", "starting", &commits[0]]);
    let path = fixture.0.join("state.db");
    let store = SqliteProjectRepository::open(&path).unwrap();
    store.create_workspace("test", "Test", &fixture.0).unwrap();
    store
        .register(&Project {
            reference: "test/alpha".into(),
            workspace_id: "test".into(),
            slug: "alpha".into(),
            project_root: root.clone(),
            origin_url: "https://github.com/acme/widgets.git".into(),
        })
        .unwrap();
    store
        .replace_check_template(
            "test/alpha",
            &TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Require publication".into(),
                criteria: ["Implementation", "Review", "Publication"]
                    .into_iter()
                    .map(|name| CheckCriterion {
                        name: name.into(),
                        description: name.into(),
                        profile_revision: None,
                    })
                    .collect(),
            },
        )
        .unwrap();
    (fixture, store, commits)
}

fn assert_inspected_lineage(app: &Application, id: u64, predecessor: Option<u64>, expected: &str) {
    let inspected = app.task_activity(id).unwrap().stack.unwrap();
    assert_eq!(inspected.predecessor, predecessor);
    assert_eq!(inspected.expected_commit.as_deref(), Some(expected));
    assert_eq!(
        inspected.verified_predecessor_commit.as_deref(),
        predecessor.map(|_| expected)
    );
    assert!(inspected.problem.is_none());
}

#[test]
fn cumulative_exact_git_input_and_publication_lineage_survive_database_reopen() {
    let (fixture, store, commits) = fixture();
    let root = fixture.0.join("repository");
    let path = fixture.0.join("state.db");
    let hosting = Hosting::default();
    let app = application(&path, &hosting);
    let mut ids = vec![];
    for _ in 0..4 {
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                project_slugs: vec!["test/alpha".into()],
                body: "Cumulative slice".into(),
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .set_record_execution_mode(
                task.id,
                task.execution_mode,
                ExecutionMode::Manual,
                "operator",
                "Manual fixture",
            )
            .unwrap();
        let source = ids.last().map_or_else(
            || StackSource::Root {
                destination: StackDestination {
                    repository: "acme/widgets".into(),
                    remote: "origin".into(),
                    branch: "starting".into(),
                },
            },
            |id| StackSource::Predecessor { record_id: *id },
        );
        app.configure_task_stack(task.id, &source).unwrap();
        ids.push(task.id);
    }
    for (index, id) in ids.iter().copied().enumerate() {
        // Reopen the database and application at each handoff.
        let app = application(&path, &hosting);
        let prepared = app.prepare_task_stack(id).unwrap().unwrap();
        let expected = if index == 0 { &commits[0] } else { &commits[1] };
        assert_eq!(prepared.base_commit.as_ref(), Some(expected));
        assert_eq!(store.task_base_commit(id).unwrap().as_ref(), Some(expected));
        assert_eq!(prepared.predecessor, index.checked_sub(1).map(|i| ids[i]));
        assert_inspected_lineage(&app, id, prepared.predecessor, expected);
        assert_eq!(
            prepared.base_branch,
            if index == 0 {
                "starting".into()
            } else {
                task_branch(ids[0])
            }
        );
        assert_eq!(app.prepare_task_stack(id).unwrap(), Some(prepared.clone()));
        store.start_record(id).unwrap();
        let output = if index == 3 { &commits[2] } else { &commits[1] };
        pass(&store, &root, id, 0, output);
        pass(&store, &root, id, 1, output);
        if index == 1 || index == 2 {
            git(&root, &["checkout", "--detach", output]);
            app.publish_unchanged_task_stack(id, None).unwrap();
        } else {
            let url = format!("https://github.com/acme/widgets/pull/{id}");
            git(&root, &["branch", &task_branch(id), output]);
            hosting.0.lock().unwrap().insert(
                url.clone(),
                PullRequestObservation {
                    repository: "acme/widgets".into(),
                    url: url.clone(),
                    head_branch: task_branch(id),
                    head_commit: output.clone(),
                    base_branch: prepared.base_branch,
                    open: true,
                    draft: false,
                    head_repository: "acme/widgets".into(),
                },
            );
            app.publish_task_stack(id, &url, None).unwrap();
        }
        pass(&store, &root, id, 2, output);
        app.done_record(id, None).unwrap();
        // Deliberately move the main checkout to a later baseline before the next preparation.
        git(&root, &["checkout", "--detach", &commits[2]]);
    }
    drop(app);
    drop(store);
    let reopened = SqliteProjectRepository::open(&path).unwrap();
    let final_stack = reopened.task_stack(ids[3]).unwrap().unwrap();
    assert_eq!(final_stack.base_commit, Some(commits[1].clone()));
    assert_eq!(
        final_stack.publication.unwrap().base_branch,
        task_branch(ids[0])
    );
    for id in &ids[1..3] {
        let unchanged = reopened.task_stack(*id).unwrap().unwrap();
        assert!(unchanged.publication.is_none());
        assert!(unchanged.unchanged_result.is_some());
    }
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), commits[2]);
    assert!(git(&root, &["status", "--porcelain"]).is_empty());
}
