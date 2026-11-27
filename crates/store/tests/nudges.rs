//! Nudges: exactly one durable prompt per finished Implementation turn, driven by reconciliation
//! against a scripted provider (the same seam the fake Herdr serves in the server tests).
use forge_app::{
    AgentAdapterError, AgentAssignmentClosureReason, AgentDispatchRepository, AgentObservation,
    AgentPromptOutcome, Application, RecordRepository,
    nudges::{NudgeIntent, NudgeSettlement, NudgeState},
};
use std::sync::Arc;
#[path = "support/implementation_fixture.rs"]
mod fixture;
use fixture::Fixture;

#[path = "support/scripted_agent.rs"]
mod scripted;
use scripted::{Factory, Script, turn};

struct World {
    f: Fixture,
    script: Arc<Script>,
    app: Application,
    assignment: String,
    now: u64,
}
impl World {
    fn new() -> Self {
        let f = Fixture::new();
        let script = Script::new();
        let app = f.app_with(Box::new(Factory(script.clone())));
        let assignment = f.launch(&app);
        Self {
            f,
            script,
            app,
            assignment,
            now: 0,
        }
    }
    /// One reconciliation scan; each is past the previous scan's lease.
    fn scan(&mut self) {
        self.now += 100_000;
        self.app
            .reconcile_agent_dispatch("nudges", self.now)
            .unwrap();
    }
    fn nudges(&self) -> NudgeState {
        self.f
            .store
            .assignment_nudge_state(&self.assignment)
            .unwrap()
    }
    fn hold(&self) -> Option<String> {
        self.f
            .store
            .dispatch_hold(self.f.task)
            .unwrap()
            .map(|h| h.reason)
    }
}

#[test]
fn a_finished_turn_is_nudged_once_and_a_new_finished_turn_is_nudged_again() {
    let mut w = World::new();
    w.scan();
    assert!(w.script.sent().is_empty(), "working agents are not nudged");

    w.script.report(AgentObservation::Idle, 1);
    w.scan();
    w.scan();
    assert_eq!(w.script.sent().len(), 1, "the same turn is nudged once");
    assert_eq!(w.script.prompts.lock().unwrap()[0].completion_seq, 1);

    // `done` is a finished turn too.
    w.script.report(AgentObservation::Done, 2);
    w.scan();
    assert_eq!(w.script.sent().len(), 2);
    assert_eq!(w.nudges().total, 2);
    assert_eq!(w.nudges().last_completion_seq, 2);
}

#[test]
fn working_blocked_and_unknown_agents_are_not_nudged() {
    let mut w = World::new();
    for observation in [
        AgentObservation::Working,
        AgentObservation::Blocked,
        AgentObservation::Unknown,
    ] {
        w.script.report(observation, 5);
        w.scan();
    }
    assert!(w.script.sent().is_empty());
    assert_eq!(w.nudges(), NudgeState::default());
}

#[test]
fn nothing_is_nudged_without_a_comparable_completion_sequence() {
    let mut w = World::new();
    *w.script.turn.lock().unwrap() = turn(AgentObservation::Idle, None);
    w.scan();
    assert!(w.script.sent().is_empty());
}

#[test]
fn no_nudge_after_a_review_request() {
    let mut w = World::new();
    w.f.set_head(&"b".repeat(40));
    w.f.session(&w.app, &w.assignment).request_review().unwrap();
    w.script.report(AgentObservation::Idle, 1);
    w.scan();
    assert!(w.script.sent().is_empty());
}

#[test]
fn no_nudge_under_a_dispatch_hold() {
    let mut w = World::new();
    w.f.store.hold_dispatch(w.f.task, "operator hold").unwrap();
    w.script.report(AgentObservation::Idle, 1);
    w.scan();
    assert!(w.script.sent().is_empty());
}

#[test]
fn review_assignments_are_never_nudged() {
    let mut w = World::new();
    w.f.session(&w.app, &w.assignment).request_review().unwrap();
    w.scan(); // retires the Implementation Assignment and launches review
    let review = w.f.launch(&w.app);
    assert_ne!(review, w.assignment);
    w.script.report(AgentObservation::Idle, 9);
    w.scan();
    assert!(w.script.sent().is_empty());
}

#[test]
fn nudge_text_follows_the_template_and_counts_consecutive_no_commit_nudges() {
    let mut w = World::new();
    let worktree =
        w.f.store
            .assignment_execution_directory(&w.assignment)
            .unwrap()
            .unwrap();
    for (seq, commit, expected) in [(1, None, 1), (2, None, 2), (3, Some("c"), 0), (4, None, 1)] {
        if let Some(commit) = commit {
            w.f.set_head(&commit.repeat(40));
        }
        w.script.report(AgentObservation::Idle, seq);
        w.scan();
        let text = w.script.sent().pop().unwrap();
        assert!(
            text.contains(&worktree.display().to_string()),
            "names the worktree: {text}"
        );
        assert!(text.contains("commit your work"));
        assert!(text.contains("forge assignment request-review"));
        assert!(text.contains("forge assignment comment"));
        assert!(!text.contains("Deliver work"), "does not repeat the Task");
        assert!(
            text.ends_with(&format!(
                "No new commit since the last nudge: {expected} of 3."
            )),
            "{text}"
        );
        assert_eq!(w.nudges().consecutive_no_commit, expected);
    }
    assert_eq!(w.nudges().total, 4);
}

#[test]
fn a_new_assignment_starts_with_zero_counters() {
    let mut w = World::new();
    w.script.report(AgentObservation::Idle, 1);
    w.scan();
    assert_eq!(w.nudges().total, 1);
    w.f.session(&w.app, &w.assignment).request_review().unwrap();
    w.scan();
    let review = w.f.launch(&w.app);
    assert_eq!(
        w.f.store.assignment_nudge_state(&review).unwrap(),
        NudgeState::default()
    );
}

fn uncertain_delivery_holds_and_never_resends(error: AgentAdapterError, cause: &str) {
    let mut w = World::new();
    *w.script.prompt_result.lock().unwrap() = Err(error);
    w.script.report(AgentObservation::Idle, 1);
    w.scan();
    let hold = w.hold().expect("uncertain delivery holds the Task");
    assert!(hold.contains("Prompt Delivery Uncertainty"), "{hold}");
    assert!(hold.contains(cause), "{hold}");
    assert_eq!(w.script.sent().len(), 1);
    assert_eq!(w.nudges().total, 1, "the uncertain prompt is spent");
    assert!(
        w.f.store
            .agent_assignment(&w.assignment)
            .unwrap()
            .closed_at_millis
            .is_none(),
        "the possibly live Agent keeps its Assignment"
    );

    // A later finished turn is not nudged while the hold stands, and nothing is resent.
    w.script.report(AgentObservation::Idle, 2);
    w.scan();
    w.scan();
    assert_eq!(w.script.sent().len(), 1);
}

#[test]
fn a_lost_or_timed_out_response_is_uncertain() {
    uncertain_delivery_holds_and_never_resends(
        AgentAdapterError::Transient("could not read Herdr response".into()),
        "did not confirm",
    );
}

#[test]
fn an_identity_mismatch_is_uncertain() {
    uncertain_delivery_holds_and_never_resends(
        AgentAdapterError::Conflict("terminal changed".into()),
        "identity",
    );
}

#[test]
fn restart_after_recording_an_intent_is_uncertain_and_never_sends() {
    let mut w = World::new();
    // A previous daemon recorded the intent and died before settling it.
    assert!(
        w.f.store
            .begin_assignment_nudge(
                &w.assignment,
                &NudgeIntent {
                    sequence: 1,
                    completion_seq: 1,
                    consecutive_no_commit: 1,
                    head: "a".repeat(40),
                },
            )
            .unwrap()
    );
    w.script.report(AgentObservation::Idle, 1);
    w.scan();
    assert!(w.script.sent().is_empty());
    assert!(w.hold().unwrap().contains("Prompt Delivery Uncertainty"));
    assert_eq!(w.nudges().total, 1);
}

#[test]
fn an_agent_that_moved_on_before_the_send_is_not_held_and_is_nudged_for_its_next_turn() {
    let mut w = World::new();
    *w.script.prompt_result.lock().unwrap() = Ok(AgentPromptOutcome::NotReady);
    w.script.report(AgentObservation::Idle, 1);
    w.scan();
    assert!(w.hold().is_none());
    assert_eq!(w.nudges(), NudgeState::default());
    *w.script.prompt_result.lock().unwrap() = Ok(AgentPromptOutcome::Delivered);
    w.scan();
    assert_eq!(w.nudges().total, 1);
}

#[test]
fn an_intent_is_exclusive_per_assignment_and_sequence() {
    let w = World::new();
    let intent = NudgeIntent {
        sequence: 1,
        completion_seq: 1,
        consecutive_no_commit: 0,
        head: "a".repeat(40),
    };
    assert!(
        w.f.store
            .begin_assignment_nudge(&w.assignment, &intent)
            .unwrap()
    );
    assert!(
        !w.f.store
            .begin_assignment_nudge(&w.assignment, &intent)
            .unwrap()
    );
    w.f.store
        .settle_assignment_nudge(&w.assignment, 1, NudgeSettlement::Confirmed)
        .unwrap();
    assert!(
        !w.f.store
            .begin_assignment_nudge(&w.assignment, &intent)
            .unwrap()
    );
    assert_eq!(
        w.f.store
            .assignment_nudge_state(&w.assignment)
            .unwrap()
            .total,
        1
    );
}

/// Idle turns with a fresh head each time never accumulate a no-commit streak.
fn nudge_with_commit(w: &mut World, seq: u64) {
    w.f.set_head(&format!("{seq:040x}"));
    w.script.report(AgentObservation::Idle, seq);
    w.scan();
}

fn exhaust_consecutive_no_commit(w: &mut World) {
    for seq in 1..=3 {
        w.script.report(AgentObservation::Idle, seq);
        w.scan();
    }
    assert_eq!(w.script.sent().len(), 3);
    assert!(w.hold().is_none());
    w.script.report(AgentObservation::Idle, 4);
    w.scan();
}

#[test]
fn three_no_commit_nudges_then_another_idle_hold_instead_of_a_fourth() {
    let mut w = World::new();
    exhaust_consecutive_no_commit(&mut w);
    assert_eq!(w.script.sent().len(), 3, "no fourth nudge");
    let hold = w.hold().expect("exhaustion holds the Task");
    assert!(hold.contains("consecutive-no-commit limit"), "{hold}");
    assert!(hold.contains("no comment"), "{hold}");
    // Further idle turns neither nudge nor re-hold.
    w.script.report(AgentObservation::Idle, 5);
    w.scan();
    assert_eq!(w.script.sent().len(), 3);
}

#[test]
fn twenty_nudges_with_commits_then_another_idle_hold_naming_the_total_limit() {
    let mut w = World::new();
    for seq in 1..=20 {
        nudge_with_commit(&mut w, seq);
    }
    assert_eq!(w.script.sent().len(), 20);
    assert!(w.hold().is_none());
    nudge_with_commit(&mut w, 21);
    assert_eq!(w.script.sent().len(), 20, "no twenty-first nudge");
    let hold = w.hold().expect("the total limit holds the Task");
    assert!(hold.contains("total limit"), "{hold}");
}

#[test]
fn the_hold_references_the_latest_agent_comment_and_keeps_the_assignment_open() {
    let mut w = World::new();
    w.f.store
        .add_record_comment(w.f.task, "first thoughts", None)
        .unwrap();
    let latest =
        w.f.store
            .add_record_comment(w.f.task, "Blocked: need the API key", None)
            .unwrap();
    exhaust_consecutive_no_commit(&mut w);
    let hold = w.hold().unwrap();
    assert!(hold.contains(&format!("#{}", latest.id.0)), "{hold}");
    assert!(hold.contains("Blocked: need the API key"), "{hold}");
    assert!(!hold.contains("first thoughts"), "{hold}");
    assert!(
        w.f.store
            .agent_assignment(&w.assignment)
            .unwrap()
            .closed_at_millis
            .is_none(),
        "the Agent is not retired"
    );
    assert_eq!(w.f.store.list_open_agent_assignments().unwrap().len(), 1);
}

#[test]
fn resume_with_an_explanation_sends_exactly_one_nudge_and_restarts_the_counters() {
    let mut w = World::new();
    exhaust_consecutive_no_commit(&mut w);
    let sequence_before = w.nudges().prompt_sequence;
    w.app
        .resume_task_dispatch(w.f.task, Some("  Use the staging key.  "))
        .unwrap();
    assert!(w.hold().is_none());
    assert_eq!(w.nudges().total, 0);
    assert_eq!(w.nudges().consecutive_no_commit, 0);

    // The Agent is still idle on the turn that tripped the limit.
    w.scan();
    w.scan();
    let sent = w.script.sent();
    assert_eq!(sent.len(), 4, "exactly one more nudge");
    assert!(
        sent[3].contains("Operator explanation: Use the staging key."),
        "{}",
        sent[3]
    );
    assert!(sent[3].ends_with("No new commit since the last nudge: 1 of 3."));
    assert_eq!(w.nudges().total, 1, "counting restarted from zero");
    assert_eq!(w.nudges().prompt_sequence, sequence_before + 1);
    assert_eq!(w.nudges().pending_explanation, None);

    // The explanation is not repeated at the next finished turn.
    w.script.report(AgentObservation::Idle, 5);
    w.scan();
    assert!(!w.script.sent()[4].contains("Operator explanation"));
}

#[test]
fn resume_with_an_explanation_waits_for_the_agent_to_finish_a_turn() {
    let mut w = World::new();
    exhaust_consecutive_no_commit(&mut w);
    w.app.resume_task_dispatch(w.f.task, Some("Hint")).unwrap();
    w.script.report(AgentObservation::Working, 4);
    w.scan();
    assert_eq!(w.script.sent().len(), 3);
    w.script.report(AgentObservation::Idle, 5);
    w.scan();
    assert!(w.script.sent()[3].contains("Operator explanation: Hint"));
}

#[test]
fn resume_without_an_explanation_sends_the_plain_template() {
    let mut w = World::new();
    exhaust_consecutive_no_commit(&mut w);
    w.app.resume_task_dispatch(w.f.task, Some("   ")).unwrap();
    w.scan();
    let sent = w.script.sent();
    assert_eq!(sent.len(), 4);
    assert!(!sent[3].contains("Operator explanation"));
}

#[test]
fn unstick_with_retirement_retires_the_held_agent_and_counts_an_interruption() {
    let mut w = World::new();
    exhaust_consecutive_no_commit(&mut w);
    let report = w
        .app
        .unstick_apply(
            w.f.task,
            &forge_app::unstick::UnstickConsent {
                agent_gone: true,
                retire_working: true,
                actor: Some("operator".into()),
                message: Some("Starting over".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(format!("{:?}", report.verdict), "Unstuck", "{report:?}");
    assert!(
        w.f.store
            .agent_assignment(&w.assignment)
            .unwrap()
            .closed_at_millis
            .is_some()
    );
    let interrupted = w
        .app
        .record_work_state(w.f.task)
        .unwrap()
        .progress
        .interrupted_checks;
    assert_eq!(interrupted[0].interruptions, 1);
}

#[test]
fn an_absent_check_bound_agent_closes_its_assignment_lost_and_holds_the_task() {
    let mut w = World::new();
    w.script.report(AgentObservation::Absent, 1);
    w.scan();
    let closed = w.f.store.agent_assignment(&w.assignment).unwrap();
    assert!(closed.closed_at_millis.is_some());
    assert_eq!(
        closed.closure_reason,
        Some(AgentAssignmentClosureReason::Lost)
    );
    assert_eq!(*w.script.retirements.lock().unwrap(), 1);
    let hold = w.hold().expect("an Absent Agent holds the Task");
    assert!(hold.contains(&w.assignment), "{hold}");
    assert!(hold.contains("recover-retained-work"), "{hold}");
    assert!(
        w.script.sent().is_empty(),
        "an Absent Agent is never prompted"
    );
    // Never automatically relaunched while the hold stands.
    w.scan();
    w.scan();
    assert!(w.f.store.list_open_agent_assignments().unwrap().is_empty());
}

#[test]
fn an_unknown_agent_is_not_closed_by_the_absent_rule() {
    let mut w = World::new();
    w.script.report(AgentObservation::Unknown, 1);
    w.scan();
    assert!(
        w.f.store
            .agent_assignment(&w.assignment)
            .unwrap()
            .closed_at_millis
            .is_none()
    );
    assert!(w.hold().is_none());
}

#[test]
fn an_absent_agent_whose_retirement_fails_keeps_the_assignment_open_but_the_task_held() {
    let mut w = World::new();
    *w.script.retire_result.lock().unwrap() =
        Err(AgentAdapterError::Transient("herdr down".into()));
    w.script.report(AgentObservation::Absent, 1);
    w.scan();
    assert!(
        w.f.store
            .agent_assignment(&w.assignment)
            .unwrap()
            .closed_at_millis
            .is_none()
    );
    assert!(w.hold().is_some());
    *w.script.retire_result.lock().unwrap() = Ok(());
    w.scan();
    assert!(
        w.f.store
            .agent_assignment(&w.assignment)
            .unwrap()
            .closed_at_millis
            .is_some()
    );
}
