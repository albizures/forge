// A Check-bound Task whose Agent launch fails or is never acknowledged is held at once, through
// the daemon's real composition (`CapabilityDeliveryAdapter` over the fake Herdr socket).
use super::nudges::daemon_application;
use super::recovery::registered_policy;
use forge_app::check_templates::{CheckCriterion, TemplateReplacement};

struct Scenario {
    fixture: Fixture,
    provider: Provider,
    app: Application,
    task: u64,
}

fn scenario() -> Scenario {
    let fixture = Fixture::new();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    let head = Arc::new(Mutex::new("a".repeat(40)));
    let app = daemon_application(&fixture, &head);
    fixture.configure(&app, &provider, "one");
    let (profile, _) = registered_policy(&fixture, RecordType::Task);
    let store = fixture.store();
    store
        .replace_check_template(
            "one/api",
            &TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Pin".into(),
                criteria: vec![CheckCriterion {
                    name: "Implementation".into(),
                    description: "Implementation".into(),
                    profile_revision: Some(profile.revision),
                }],
            },
        )
        .unwrap();
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver work".into(),
            project_slugs: vec!["one/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let claim = store.reserve_worktree_claim(task.id).unwrap();
    std::fs::create_dir_all(&claim.path).unwrap();
    store
        .set_task_base_commit(task.id, &"a".repeat(40))
        .unwrap();
    store.activate_worktree_claim(task.id).unwrap();
    Scenario {
        fixture,
        provider,
        app,
        task: task.id,
    }
}

impl Scenario {
    fn hold(&self) -> Option<String> {
        self.fixture
            .store()
            .dispatch_hold(self.task)
            .unwrap()
            .map(|hold| hold.reason)
    }
    fn dispatch_comments(&self) -> Vec<String> {
        self.fixture
            .store()
            .list_record_comments(self.task)
            .unwrap()
            .into_iter()
            .filter(|comment| comment.author.as_deref() == Some("agent-dispatch"))
            .map(|comment| comment.body)
            .collect()
    }
    fn assignments(&self) -> Vec<forge_app::AgentAssignment> {
        self.fixture
            .store()
            .list_agent_assignments(self.task)
            .unwrap()
    }
    /// The one open, still-provisioning Assignment that keeps its capacity.
    fn retained_assignment(&self) -> forge_app::AgentAssignment {
        let assignments = self.assignments();
        assert_eq!(assignments.len(), 1, "{assignments:?}");
        let assignment = assignments.into_iter().next().unwrap();
        assert!(assignment.closed_at_millis.is_none(), "{assignment:?}");
        assert_eq!(assignment.state, forge_app::AgentAssignmentState::Provisioning);
        assert!(
            self.app
                .agent_assignment_info(&assignment.id)
                .unwrap()
                .capacity_reserved()
        );
        assignment
    }
    fn scan_repeatedly(&self) {
        for round in 1..=3 {
            self.app
                .reconcile_agent_dispatch("daemon", 1_000_000 * round)
                .unwrap();
        }
    }
    fn assert_held_once(&self, expected: &[&str], forbidden: &[&str]) -> String {
        let reason = self.hold().expect("Dispatch Hold");
        let comments = self.dispatch_comments();
        assert_eq!(comments, vec![reason.clone()], "one hold, one comment");
        for text in expected {
            assert!(reason.contains(text), "{text} missing from {reason}");
        }
        for text in forbidden {
            assert!(!reason.contains(text), "{text} leaked into {reason}");
        }
        reason
    }
}

const RUNBOOK_TAIL: &str = "forge dispatcher assignment-retire";

#[test]
fn an_agent_start_error_code_holds_the_task_once_without_leaking_the_message() {
    let s = scenario();
    s.provider.fail_agent_start("mystery_failure", "PRIVATE detail");
    s.app.execute_task_check_acceptance(s.task).unwrap();
    let assignment = s.retained_assignment();
    let reason = s.assert_held_once(
        &[
            "Herdr agent.start: mystery_failure",
            &assignment.id,
            "will not retry",
            &format!("herdr agent get forge-dispatch-{}", assignment.id),
            RUNBOOK_TAIL,
            "assignment-resolve-unknown",
            "forge dispatcher resume",
        ],
        &["PRIVATE"],
    );
    s.scan_repeatedly();
    assert_eq!(s.hold().unwrap(), reason);
    s.assert_held_once(&[], &["PRIVATE"]);
    s.retained_assignment();
    assert_eq!(s.provider.calls_of("agent.start"), 1, "never relaunched");
}

#[test]
fn a_lost_agent_start_response_holds_with_details_withheld() {
    let s = scenario();
    s.provider.state.lock().unwrap().lose_launch_response = true;
    s.app.execute_task_check_acceptance(s.task).unwrap();
    s.retained_assignment();
    s.assert_held_once(&["details withheld", "Agent launch failed"], &[]);
    s.scan_repeatedly();
    s.assert_held_once(&[], &[]);
    assert_eq!(s.provider.calls_of("agent.start"), 1);
}

#[test]
fn a_tab_create_failure_before_the_launch_intent_holds_the_task() {
    let s = scenario();
    s.provider.fail_tab_create("tab_boom", "PRIVATE");
    s.app.execute_task_check_acceptance(s.task).unwrap();
    s.retained_assignment();
    s.assert_held_once(&["Herdr tab.create: tab_boom"], &["PRIVATE"]);
    s.scan_repeatedly();
    s.assert_held_once(&[], &[]);
    assert_eq!(s.provider.calls_of("agent.start"), 0);
}

#[test]
fn error_codes_that_are_not_bare_identifiers_are_withheld() {
    for code in ["Mixed_Case", "has space", "a/b", "trailing: PRIVATE", "../etc", ""] {
        let s = scenario();
        s.provider.fail_agent_start(code, "PRIVATE");
        s.app.execute_task_check_acceptance(s.task).unwrap();
        s.assert_held_once(&["details withheld"], &["PRIVATE", "Herdr agent.start"]);
    }
}

#[test]
fn an_existing_dispatch_hold_keeps_its_reason_and_adds_no_comment() {
    let s = scenario();
    s.fixture
        .store()
        .hold_dispatch(s.task, "Independent operator hold")
        .unwrap();
    s.provider.fail_agent_start("mystery_failure", "PRIVATE");
    // The Task is already held, so acceptance is refused before any launch.
    assert!(s.app.execute_task_check_acceptance(s.task).is_err());
    assert_eq!(s.hold().unwrap(), "Independent operator hold");
    assert!(s.dispatch_comments().is_empty());
}

#[test]
fn a_deadline_after_a_launch_failure_hold_keeps_its_reason_and_adds_no_comment() {
    let s = scenario();
    s.provider.fail_agent_start("mystery_failure", "PRIVATE");
    s.app.execute_task_check_acceptance(s.task).unwrap();
    let first = s.hold().unwrap();
    // A later deadline expiry never rewrites the existing reason nor adds a comment.
    let deadline = s
        .fixture
        .store()
        .agent_launch_intent(&s.retained_assignment().id)
        .unwrap()
        .unwrap()
        .acknowledgement_deadline_millis;
    s.app.reconcile_agent_dispatch("daemon", deadline + 1).unwrap();
    assert_eq!(s.hold().unwrap(), first);
    assert_eq!(s.dispatch_comments().len(), 1);
}

#[test]
fn an_unacknowledged_launch_is_held_only_after_its_deadline() {
    let s = scenario();
    s.app.execute_task_check_acceptance(s.task).unwrap();
    let assignment = s.retained_assignment();
    let deadline = s
        .fixture
        .store()
        .agent_launch_intent(&assignment.id)
        .unwrap()
        .unwrap()
        .acknowledgement_deadline_millis;
    s.app
        .reconcile_agent_dispatch("daemon", deadline - 1)
        .unwrap();
    assert!(s.hold().is_none());
    assert!(s.dispatch_comments().is_empty());

    for round in 0..3 {
        s.app
            .reconcile_agent_dispatch("daemon", deadline + 1 + round)
            .unwrap();
    }
    s.retained_assignment();
    s.assert_held_once(
        &[
            "Agent acknowledgement deadline expired",
            &assignment.id,
            "never acknowledged",
            "will not retry the launch",
            RUNBOOK_TAIL,
        ],
        &[],
    );
    assert_eq!(s.provider.calls_of("agent.start"), 1, "never relaunched");
}

#[test]
fn the_runbook_recovers_a_failed_launch_end_to_end() {
    let s = scenario();
    s.provider.fail_agent_start("mystery_failure", "PRIVATE");
    s.app.execute_task_check_acceptance(s.task).unwrap();
    let failed = s.retained_assignment();
    // The operator confirms in Herdr that the tab and pane are gone, then follows the runbook.
    s.provider.heal();
    s.provider.state.lock().unwrap().tab_closed = true;
    s.app.retire_agent_assignment(&failed.id).unwrap();
    s.app
        .resolve_unknown_agent_resources(&failed.id, true, true)
        .unwrap();
    s.app.resume_task_dispatch(s.task, None).unwrap();
    s.provider.state.lock().unwrap().tab_closed = false;

    let (fresh, existing) = s.app.execute_task_check_acceptance(s.task).unwrap();
    assert!(!existing);
    assert_ne!(fresh.id, failed.id);
    assert!(s.hold().is_none());
    assert_eq!(s.provider.calls_of("agent.start"), 2);
}
