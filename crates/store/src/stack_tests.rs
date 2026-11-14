use super::*;
use crate::SqliteProjectRepository;
use rusqlite::params;
use std::sync::{Arc, Mutex};
use forge_app::{Application, ExecutionMode, RepositoryInspector,
    check_templates::{CheckCriterion, TemplateReplacement},
    outcomes::{AttemptInput, Evidence, OutcomeDisposition, OutcomeInput},
    stacks::{Publication, PullRequestObservation, StackDestination, StackHosting, StackSource, TaskStack, task_branch}};
use std::collections::HashMap;

fn fixture() -> SqliteProjectRepository {
    let store = super::SqliteProjectRepository::in_memory().unwrap();
    store.register(&project("alpha", "/stack-project")).unwrap();
    store.replace_check_template("alpha", &TemplateReplacement {
        expected_version: 0, actor: "operator".into(), message: "Require publication".into(),
        criteria: ["Implementation", "Review", "Publication"].into_iter().map(|name| CheckCriterion {
            name: name.into(), description: format!("Verify {name}"), profile_revision: None,
        }).collect(),
    }).unwrap();
    store
}
fn task(store: &SqliteProjectRepository) -> u64 {
    let task = store.open_typed_record(&NewTypedRecord { record_type: RecordType::Task,
        project_slugs: vec!["alpha".into()], body: "Deliver a slice".into(), generated_by_ids: vec![], }).unwrap();
    store.set_record_execution_mode(task.id, task.execution_mode, ExecutionMode::Manual, "operator", "Manual test").unwrap();
    store.set_task_base_commit(task.id, &"0".repeat(40)).unwrap();
    task.id
}
fn root() -> StackSource {
    StackSource::Root { destination: StackDestination { repository: "acme/widgets".into(), remote: "origin".into(), branch: "feature/payments".into() } }
}
fn pin(store: &SqliteProjectRepository, id: u64, commit: &str) -> TaskStack {
    store.pin_task_stack(&store.task_stack(id).unwrap().unwrap(), commit).unwrap()
}
fn outcome(store: &SqliteProjectRepository, id: u64, position: usize, commit: &str) -> OutcomeInput {
    let check = store.list_record_checks(id).unwrap()[position].clone();
    let work = store.record_work_state(id).unwrap();
    let evidence = Evidence::Repository { repository: "/stack-project".into(), commit: commit.into() };
    let input = OutcomeInput { attempt_id: format!("task-{id}-check-{position}-{}", work.generation), generation: work.generation,
        evidence: evidence.clone(), verdict: forge_app::checks::Verdict::Pass, actor: "operator".into(),
        message: "Verified".into(), disposition: OutcomeDisposition::Complete, findings: vec![], verifications: vec![], };
    store.begin_record_attempt(id, &AttemptInput { id: input.attempt_id.clone(), check_id: check.id,
        generation: work.generation, evidence, actor: "operator".into() }).unwrap();
    input
}
fn pass(store: &SqliteProjectRepository, id: u64, position: usize, commit: &str) {
    let input = outcome(store, id, position, commit);
    store.submit_record_outcome(id, &input).unwrap();
}
fn publication(id: u64, commit: &str, base: &str) -> Publication {
    Publication { repository: "acme/widgets".into(), url: format!("https://github.com/acme/widgets/pull/{id}"),
        head_branch: task_branch(id), head_commit: commit.into(), base_branch: base.into(), generation: 1 }
}
fn complete(store: &SqliteProjectRepository, id: u64, commit: &str) {
    store.start_record(id).unwrap();
    pass(store, id, 0, commit);
    pass(store, id, 1, commit);
    let stack = store.task_stack(id).unwrap().unwrap();
    store.record_stack_publication(&stack, &publication(id, commit, &stack.base_branch), None).unwrap();
    pass(store, id, 2, commit);
    store.done_record(id, None).unwrap();
}

#[test]
fn stacks_are_explicit_linear_and_dependencies_cannot_be_removed() {
    let store = fixture();
    let first = task(&store);
    let second = task(&store);
    let third = task(&store);
    store.configure_task_stack(first, &root()).unwrap();
    assert!(store.task_base_commit(first).unwrap().is_none());
    store.configure_task_stack(second, &StackSource::Predecessor { record_id: first }).unwrap();
    assert_eq!(store.task_stack(second).unwrap().unwrap().base_branch, task_branch(first));
    assert!(store.configure_task_stack(third, &StackSource::Predecessor { record_id: first }).is_err());
    assert!(store.task_stack(third).unwrap().is_none());
    assert!(store.configure_task_stack(first, &StackSource::Predecessor { record_id: second }).is_err());
    assert!(store.unrelate_records(second, first, RecordRelationshipKind::DependsOn).is_err());
    assert!(store.start_record(first).is_err());
    assert!(store.show_record(second).unwrap().relationships.iter().any(|r| r.kind == RecordRelationshipKind::DependsOn && r.to_id == first));
}

#[test]
fn successor_pins_completed_predecessor_not_creation_head_or_adjacent_id() {
    let store = fixture();
    let first = task(&store);
    let _unrelated = task(&store);
    let second = task(&store);
    store.configure_task_stack(first, &root()).unwrap();
    store.configure_task_stack(second, &StackSource::Predecessor { record_id: first }).unwrap();
    assert!(store.pin_task_stack(&store.task_stack(second).unwrap().unwrap(), &"a".repeat(40)).is_err());
    pin(&store, first, &"1".repeat(40));
    complete(&store, first, &"a".repeat(40));
    let before = store.task_stack(second).unwrap().unwrap();
    assert!(store.pin_task_stack(&before, &"b".repeat(40)).is_err());
    let prepared = pin(&store, second, &"a".repeat(40));
    assert_eq!(prepared.base_commit.as_deref(), Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
    assert_eq!(prepared.base_branch, task_branch(first));
    assert!(store.configure_task_stack(first, &root()).is_err());
    assert!(store.configure_task_stack(second, &root()).is_err());
}

#[test]
fn publication_is_required_and_stale_evidence_never_completes() {
    let store = fixture();
    let id = task(&store);
    store.configure_task_stack(id, &root()).unwrap();
    pin(&store, id, &"1".repeat(40));
    store.start_record(id).unwrap();
    let initial = store.task_stack(id).unwrap().unwrap();
    assert!(store.record_stack_publication(&initial, &publication(id, &"a".repeat(40), &initial.base_branch), None).is_err());
    pass(&store, id, 0, &"a".repeat(40));
    pass(&store, id, 1, &"a".repeat(40));
    let input = outcome(&store, id, 2, &"a".repeat(40));
    assert_eq!(store.submit_record_outcome(id, &input).unwrap_err().code, "stack-blocked");
    let stack = store.task_stack(id).unwrap().unwrap();
    let mut pr = publication(id, &"a".repeat(40), &stack.base_branch);
    pr.generation = 2;
    assert!(store.record_stack_publication(&stack, &pr, None).is_err());
    pr.generation = 1;
    let saved = store.record_stack_publication(&stack, &pr, None).unwrap();
    assert_eq!(saved.publication, Some(pr));
    store.submit_record_outcome(id, &input).unwrap();
    store.invalidate_record_work(id, &forge_app::outcomes::WorkInvalidationInput {
        generation: 1, evidence: Evidence::Repository { repository: "/stack-project".into(), commit: "a".repeat(40) }, actor: "operator".into(), message: "Recheck".into(),
    }).unwrap();
    assert!(store.done_record(id, None).is_err());
}

#[test]
fn late_link_to_completed_task_is_allowed_but_incompatible_checks_are_rejected() {
    let store = fixture();
    let first = task(&store);
    store.configure_task_stack(first, &root()).unwrap();
    pin(&store, first, &"1".repeat(40));
    complete(&store, first, &"a".repeat(40));
    let second = task(&store);
    store.configure_task_stack(second, &StackSource::Predecessor { record_id: first }).unwrap();
    pin(&store, second, &"a".repeat(40));
    store.replace_check_template("alpha", &TemplateReplacement { expected_version: 1, actor: "operator".into(), message: "Implementation only".into(), criteria: vec![CheckCriterion { name: "Implementation".into(), description: "Deliver".into(), profile_revision: None }] }).unwrap();
    let incompatible = task(&store);
    assert!(store.configure_task_stack(incompatible, &root()).is_err());
}

#[test]
fn stale_preparation_is_atomic_and_preserves_the_current_destination() {
    let store = fixture();
    let first = task(&store);
    let original = store.configure_task_stack(first, &root()).unwrap();
    let alternate = StackSource::Root { destination: StackDestination { branch: "release/2.x".into(), ..original.destination.clone() } };
    let changed = store.configure_task_stack(first, &alternate).unwrap();
    assert!(store.pin_task_stack(&original, &"a".repeat(40)).is_err());
    assert_eq!(store.task_stack(first).unwrap(), Some(changed));
    assert!(store.task_base_commit(first).unwrap().is_none());
}

#[derive(Clone, Default)]
struct Hosting {
    branches: Arc<Mutex<HashMap<String, String>>>,
    prs: Arc<Mutex<HashMap<String, PullRequestObservation>>>,
}
impl StackHosting for Hosting {
    fn fetch_branch(&self, _: &Path, _: &StackDestination, branch: &str) -> Result<String, RegistrationError> {
        self.branches.lock().unwrap().get(branch).cloned().ok_or_else(|| RegistrationError::new("hosting-test", "branch unavailable"))
    }
    fn pull_request(&self, _: &Path, _: &str, url: &str) -> Result<PullRequestObservation, RegistrationError> {
        self.prs.lock().unwrap().get(url).cloned().ok_or_else(|| RegistrationError::new("hosting-test", "PR unavailable"))
    }
}
struct Inspector;
impl RepositoryInspector for Inspector {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> { Err(RegistrationError::new("test", "unused")) }
}
fn app(store: &SqliteProjectRepository, hosting: &Hosting) -> Application {
    Application::new(Box::new(SqliteProjectRepository { connection: store.connection.clone(), record_workspace: store.record_workspace.clone() }), Box::new(Inspector)).with_stack_hosting(Box::new(hosting.clone()))
}
fn observation(pr: &Publication) -> PullRequestObservation {
    PullRequestObservation { repository: pr.repository.clone(), url: pr.url.clone(), head_branch: pr.head_branch.clone(), head_commit: pr.head_commit.clone(), base_branch: pr.base_branch.clone(), open: true, draft: false, head_repository: pr.repository.clone() }
}

#[test]
fn application_fetches_any_root_branch_and_rejects_draft_wrong_base_and_wrong_head() {
    let store = fixture();
    let hosting = Hosting::default();
    let app = app(&store, &hosting);
    let id = task(&store);
    app.configure_task_stack(id, &root()).unwrap();
    assert!(app.prepare_task_stack(id).is_err());
    assert!(store.task_base_commit(id).unwrap().is_none());
    hosting.branches.lock().unwrap().insert("feature/payments".into(), "1".repeat(40));
    let stack = app.prepare_task_stack(id).unwrap().unwrap();
    assert_eq!(stack.base_commit, Some("1".repeat(40)));
    store.start_record(id).unwrap();
    pass(&store, id, 0, &"a".repeat(40));
    pass(&store, id, 1, &"a".repeat(40));
    let pr = publication(id, &"a".repeat(40), &stack.base_branch);
    hosting.branches.lock().unwrap().insert(task_branch(id), "a".repeat(40));
    for bad in [PullRequestObservation { draft: true, ..observation(&pr) }, PullRequestObservation { base_branch: "main".into(), ..observation(&pr) }, PullRequestObservation { head_commit: "b".repeat(40), ..observation(&pr) }, PullRequestObservation { head_repository: "other/widgets".into(), ..observation(&pr) }, PullRequestObservation { open: false, ..observation(&pr) }] {
        hosting.prs.lock().unwrap().insert(pr.url.clone(), bad);
        assert!(app.publish_task_stack(id, &pr.url, None).is_err());
        assert!(store.task_stack(id).unwrap().unwrap().publication.is_none());
    }
    hosting.prs.lock().unwrap().insert(pr.url.clone(), observation(&pr));
    app.publish_task_stack(id, &pr.url, None).unwrap();
    // Identical verification is safe to repeat after an uncertain response.
    app.publish_task_stack(id, &pr.url, None).unwrap();
    pass(&store, id, 2, &"a".repeat(40));
    hosting.branches.lock().unwrap().insert(task_branch(id), "b".repeat(40));
    assert!(app.done_record(id, None).is_err());
    hosting.branches.lock().unwrap().insert(task_branch(id), "a".repeat(40));
    app.done_record(id, None).unwrap();
}

#[test]
fn predecessor_movement_closed_pr_and_missing_branch_block_successor_preparation() {
    let store = fixture();
    let hosting = Hosting::default();
    let app = app(&store, &hosting);
    let first = task(&store);
    let second = task(&store);
    store.configure_task_stack(first, &root()).unwrap();
    store.configure_task_stack(second, &StackSource::Predecessor { record_id: first }).unwrap();
    pin(&store, first, &"1".repeat(40));
    complete(&store, first, &"a".repeat(40));
    let pr = store.task_stack(first).unwrap().unwrap().publication.unwrap();
    hosting.prs.lock().unwrap().insert(pr.url.clone(), observation(&pr));
    assert!(app.prepare_task_stack(second).is_err());
    hosting.branches.lock().unwrap().insert(task_branch(first), "b".repeat(40));
    assert!(app.prepare_task_stack(second).is_err());
    hosting.branches.lock().unwrap().insert(task_branch(first), "a".repeat(40));
    hosting.prs.lock().unwrap().get_mut(&pr.url).unwrap().open = false;
    assert!(app.prepare_task_stack(second).is_err());
    assert!(store.task_stack(second).unwrap().unwrap().base_commit.is_none());
    hosting.prs.lock().unwrap().get_mut(&pr.url).unwrap().open = true;
    let prepared = app.prepare_task_stack(second).unwrap().unwrap();
    assert_eq!(prepared.base_commit, Some("a".repeat(40)));
    hosting.branches.lock().unwrap().insert(task_branch(first), "b".repeat(40));
    assert!(app.prepare_task_stack(second).is_err());
}

#[test]
fn concurrent_successors_cannot_fork_a_stack_or_leave_partial_dependencies() {
    let store = fixture();
    let first = task(&store);
    let second = task(&store);
    let third = task(&store);
    store.configure_task_stack(first, &root()).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let results: Vec<_> = [second, third].into_iter().map(|id| {
        let connection = store.connection.clone();
        let workspace = store.record_workspace.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let handle = SqliteProjectRepository { connection, record_workspace: workspace };
            barrier.wait();
            (id, handle.configure_task_stack(id, &StackSource::Predecessor { record_id: first }).is_ok())
        })
    }).collect::<Vec<_>>().into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|(_, success)| *success).count(), 1);
    for (id, success) in results {
        assert_eq!(store.task_stack(id).unwrap().is_some(), success);
        assert_eq!(store.show_record(id).unwrap().relationships.iter().any(|r| r.kind == RecordRelationshipKind::DependsOn && r.to_id == first), success);
    }
}

#[test]
fn stack_membership_rejects_other_projects_and_prepared_suffix_changes_atomically() {
    let store = fixture();
    let first = task(&store);
    let second = task(&store);
    store.configure_task_stack(first, &root()).unwrap();
    store.configure_task_stack(second, &StackSource::Predecessor { record_id: first }).unwrap();
    // Simulate a retained preparation on the suffix to exercise the root mutation gate.
    store.connection.lock().unwrap().execute("UPDATE task_stacks SET base_commit = ?2 WHERE record_id = ?1", params![second.cast_signed(), "a".repeat(40)]).unwrap();
    let before = store.task_stack(first).unwrap();
    assert!(store.configure_task_stack(first, &StackSource::Root { destination: StackDestination { branch: "release/3.x".into(), ..before.as_ref().unwrap().destination.clone() } }).is_err());
    assert_eq!(store.task_stack(first).unwrap(), before);
    store.register(&project("other", "/stack-other-project")).unwrap();
    store.replace_check_template("other", &TemplateReplacement { expected_version: 0, actor: "operator".into(), message: "Compatible checks".into(), criteria: store.show_check_template("alpha", None, 10).unwrap().criteria }).unwrap();
    let other = store.open_typed_record(&NewTypedRecord { record_type: RecordType::Task, project_slugs: vec!["other".into()], body: "Other work".into(), generated_by_ids: vec![] }).unwrap();
    store.set_record_execution_mode(other.id, other.execution_mode, ExecutionMode::Manual, "operator", "Manual fixture").unwrap();
    let error = store.configure_task_stack(other.id, &StackSource::Predecessor { record_id: first }).unwrap_err();
    assert!(error.message.contains("same Project"));
    assert!(store.task_stack(other.id).unwrap().is_none());
}
