//! Entry-point matrix for Spec execution, including legacy state and retained recovery facts.
use super::*;
use forge_app::{AgentDispatchRepository, DispatchPreflightSelection, ManagedWorktreeRepository,
    NewAgentAssignment, outcomes::{AttemptInput, Evidence, WorkInvalidationInput}};

fn evidence(commit: &str) -> Evidence {
    Evidence::Repository { repository: "/stack-project".into(), commit: commit.into() }
}
fn attempt(store: &SqliteProjectRepository, task: u64, id: &str, commit: &str) -> AttemptInput {
    AttemptInput { id: id.into(), check_id: store.list_record_checks(task).unwrap()[0].id,
        generation: store.record_work_state(task).unwrap().generation,
        evidence: evidence(commit), actor: "operator".into() }
}
fn selection() -> DispatchPreflightSelection {
    DispatchPreflightSelection { task_revision: 1, grilling_revision: 1,
        provider_generation: 0, binding_id: "provider".into() }
}
fn status(store: &SqliteProjectRepository, id: u64, value: &str) {
    // Historical snapshots deliberately bypass today's public lifecycle gates.
    store.connection.lock().unwrap().execute("UPDATE records SET status = ?2 WHERE id = ?1",
        params![id.cast_signed(), value]).unwrap();
}

#[test]
fn open_spec_blocks_every_claim_preparation_and_manual_start_without_mutation() {
    let store = fixture();
    let parent = spec(&store);
    let manual = child(&store, parent);
    let dispatched = store.open_task_under_spec("Dispatched", parent, vec![]).unwrap().id;
    let checks = store.list_record_checks(dispatched).unwrap();
    let updates = store.list_record_updates(manual).unwrap();
    assert_eq!(store.start_record(manual).unwrap_err().code, "spec-execution-blocked");
    assert_eq!(store.validate_task_execution_plan(dispatched).unwrap_err().code, "spec-execution-blocked");
    assert_eq!(store.validate_task_check_preparation(dispatched).unwrap_err().code, "spec-execution-blocked");
    assert_eq!(store.claim_record_agent_assignment(dispatched, "explicit").unwrap_err().code, "spec-execution-blocked");
    assert!(store.claim_next_agent_assignment("alpha", "auto").unwrap().is_none());
    assert_eq!(store.claim_task_check_acceptance(dispatched, "check", &selection(), &evidence(&"a".repeat(40)), checks[0].id, 1, None).unwrap_err().code, "spec-execution-blocked");
    assert_eq!(store.create_agent_assignment(&NewAgentAssignment { id: "direct".into(), record_id: dispatched,
        project_slug: "alpha".into(), profile_revision: 1 }).unwrap_err().code, "spec-execution-blocked");
    assert!(store.list_agent_assignments(dispatched).unwrap().is_empty());
    assert_eq!(store.list_record_checks(dispatched).unwrap(), checks);
    assert_eq!(store.list_record_updates(manual).unwrap(), updates);
    assert_eq!(store.record_work_state(dispatched).unwrap().input, None);
    assert!(store.worktree_claim(dispatched).unwrap().is_none());
    // A preexisting In Progress child must not bypass the Manual attempt gate.
    status(&store, manual, "in_progress");
    assert_eq!(store.begin_record_attempt(manual, &attempt(&store, manual, "manual", &"a".repeat(40))).unwrap_err().code, "spec-execution-blocked");
    assert!(store.record_work_state(manual).unwrap().input.is_none());
}

#[test]
fn automatic_claim_skips_open_spec_and_valid_started_spec_retains_dependency_gates() {
    let store = fixture();
    let parent = spec(&store);
    let blocked = child(&store, parent);
    let standalone = task(&store);
    let claimed = store.claim_next_agent_assignment("alpha", "auto").unwrap().unwrap();
    assert_eq!(claimed.record_id, standalone);
    assert!(store.list_agent_assignments(blocked).unwrap().is_empty());
    store.start_record(parent).unwrap();
    let prerequisite = task(&store);
    // Use an isolated second plan because started plans cannot be structurally edited.
    let other = spec(&store);
    let dependent = child(&store, other);
    store.relate_records(dependent, prerequisite, RecordRelationshipKind::DependsOn).unwrap();
    store.start_record(other).unwrap();
    assert_eq!(store.start_record(dependent).unwrap_err().code, "dependencies-not-done");
    status(&store, dependent, "in_progress");
    assert_eq!(store.begin_record_attempt(dependent, &attempt(&store, dependent, "blocked", &"a".repeat(40))).unwrap_err().code, "dependencies-not-done");
    store.start_record(blocked).unwrap();
    store.begin_record_attempt(blocked, &attempt(&store, blocked, "allowed", &"a".repeat(40))).unwrap();
}

#[test]
fn legacy_started_partial_plan_rejects_root_and_successor_without_rewriting_pins() {
    let store = fixture();
    let parent = spec(&store);
    let first = child(&store, parent);
    let second = child(&store, parent);
    store.relate_records(second, first, RecordRelationshipKind::DependsOn).unwrap();
    status(&store, parent, "in_progress");
    status(&store, first, "done");
    let before = store.list_record_checks(second).unwrap();
    let error = store.start_record(second).unwrap_err();
    assert_eq!(error.code, "invalid-spec-plan");
    assert!(error.message.contains(&format!("Task {second}, prerequisite {first}")));
    status(&store, first, "open");
    assert_eq!(store.start_record(first).unwrap_err().code, "invalid-spec-plan");
    assert_eq!(store.list_record_checks(second).unwrap(), before);
    assert!(store.task_stack(second).unwrap().is_none());
    assert_eq!(store.configure_task_stack(first, &root()).unwrap_err().code, "spec-plan-started");
}

#[test]
fn legacy_assignment_start_and_restart_launch_recheck_parent_including_idempotent_start() {
    let store = fixture();
    let parent = spec(&store);
    let id = store.open_task_under_spec("Deliver", parent, vec![]).unwrap().id;
    store.start_record(parent).unwrap();
    store.create_agent_assignment(&NewAgentAssignment { id: "legacy".into(), record_id: id,
        project_slug: "alpha".into(), profile_revision: 1 }).unwrap();
    store.start_owned_record(id, "legacy").unwrap();
    status(&store, parent, "open");
    assert_eq!(store.start_owned_record(id, "legacy").unwrap_err().code, "spec-execution-blocked");
    assert_eq!(store.begin_agent_provision("legacy").unwrap_err().code, "spec-execution-blocked");
    assert_eq!(store.begin_agent_launch("legacy", 1).unwrap_err().code, "spec-execution-blocked");
    assert!(store.agent_launch_intent("legacy").unwrap().is_none());
    assert!(store.list_agent_assignment_resources("legacy").unwrap().is_empty());
    assert!(store.agent_assignment("legacy").unwrap().closed_at_millis.is_none());
}

#[test]
fn legacy_wrong_predecessor_commit_blocks_without_repinning_or_rewriting_done_evidence() {
    let store = fixture();
    let parent = spec(&store);
    let first = child(&store, parent);
    let second = child(&store, parent);
    store.configure_task_stack(first, &root()).unwrap();
    link(&store, second, first);
    pin(&store, first, &"a".repeat(40));
    store.start_record(parent).unwrap();
    complete(&store, first, &"b".repeat(40));
    pin(&store, second, &"b".repeat(40));
    let history = store.record_work_state(first).unwrap();
    let checks = store.list_record_checks(first).unwrap();
    let c = store.connection.lock().unwrap();
    for table in ["task_stacks", "task_base_commits"] {
        c.execute(&format!("UPDATE {table} SET base_commit = ?2 WHERE record_id = ?1"),
            params![second.cast_signed(), "c".repeat(40)]).unwrap();
    }
    drop(c);
    let error = store.start_record(second).unwrap_err();
    assert_eq!(error.code, "stack-blocked");
    assert!(error.message.contains(&format!("Stack Predecessor {first}")));
    assert_eq!(store.task_base_commit(second).unwrap(), Some("c".repeat(40)));
    assert_eq!(store.record_work_state(first).unwrap(), history);
    assert_eq!(store.list_record_checks(first).unwrap(), checks);
    assert_eq!(store.show_record(second).unwrap().status, RecordStatus::Open);
}

#[test]
fn evidence_refresh_cannot_bypass_open_retained_or_uncertain_ownership() {
    use forge_app::{AgentAssignmentClosureReason, NewAgentAssignmentResource};
    let store = fixture();
    let parent = spec(&store);
    let id = child(&store, parent);
    store.start_record(parent).unwrap();
    store.start_record(id).unwrap();
    store.create_agent_assignment(&NewAgentAssignment { id: "old".into(), record_id: id,
        project_slug: "alpha".into(), profile_revision: 1 }).unwrap();
    let input = WorkInvalidationInput { generation: 1, evidence: evidence(&"a".repeat(40)),
        actor: "operator".into(), message: "Investigated".into() };
    assert_eq!(store.invalidate_record_work(id, &input).unwrap_err().code, "execution-mode-unsafe-ownership");
    store.close_agent_assignment("old", AgentAssignmentClosureReason::Crashed).unwrap();
    // Closed without resource identities is still unknown, not safe retirement.
    assert_eq!(store.invalidate_record_work(id, &input).unwrap_err().code, "execution-mode-unsafe-ownership");
    store.add_agent_assignment_resource(&NewAgentAssignmentResource { assignment_id: "old".into(),
        resource_kind: "pane".into(), resource_id: "retained".into() }).unwrap();
    assert_eq!(store.invalidate_record_work(id, &input).unwrap_err().code, "execution-mode-unsafe-ownership");
    assert_eq!(store.record_work_state(id).unwrap().generation, 1);
    assert!(store.record_work_state(id).unwrap().input.is_none());
    assert!(store.list_agent_assignment_resources("old").unwrap()[0].retired_at_millis.is_none());
}

#[test]
fn wrong_initial_checkout_and_retained_base_block_but_explicit_safe_evidence_refresh_works() {
    let store = fixture();
    let parent = spec(&store);
    let first = child(&store, parent);
    let second = child(&store, parent);
    store.configure_task_stack(first, &root()).unwrap();
    link(&store, second, first);
    pin(&store, first, &"a".repeat(40));
    store.start_record(parent).unwrap();
    complete(&store, first, &"b".repeat(40));
    pin(&store, second, &"b".repeat(40));
    let checks = store.list_record_checks(second).unwrap();
    store.start_record(second).unwrap();
    let wrong = attempt(&store, second, "wrong", &"a".repeat(40));
    assert_eq!(store.begin_record_attempt(second, &wrong).unwrap_err().code, "stack-blocked");
    assert!(store.record_work_state(second).unwrap().input.is_none());
    // Historical bad pins are diagnosed, not changed to fit the latest observation.
    store.connection.lock().unwrap().execute("UPDATE task_base_commits SET base_commit = ?2 WHERE record_id = ?1",
        params![second.cast_signed(), "a".repeat(40)]).unwrap();
    assert_eq!(store.begin_record_attempt(second, &attempt(&store, second, "bad-pin", &"b".repeat(40))).unwrap_err().code, "stack-blocked");
    store.connection.lock().unwrap().execute("UPDATE task_base_commits SET base_commit = ?2 WHERE record_id = ?1",
        params![second.cast_signed(), "b".repeat(40)]).unwrap();
    // Only fixture restoration above edits pins; recovery below uses supported APIs.
    store.invalidate_record_work(second, &WorkInvalidationInput { generation: 1, evidence: evidence(&"b".repeat(40)), actor: "operator".into(), message: "Verified correct retained checkout; refresh work".into() }).unwrap();
    let fresh = attempt(&store, second, "fresh", &"b".repeat(40));
    store.begin_record_attempt(second, &fresh).unwrap();
    assert_eq!(fresh.generation, 2);
    assert_eq!(store.task_base_commit(second).unwrap(), Some("b".repeat(40)));
    for (after, before) in store.list_record_checks(second).unwrap().iter().zip(checks) {
        assert_eq!(after.id, before.id);
        assert_eq!(after.profile_revision, before.profile_revision);
        assert_eq!(after.description, before.description);
        assert_eq!(after.version, before.version + 1);
    }
    assert_eq!(store.show_record(first).unwrap().status, RecordStatus::Done);
}
