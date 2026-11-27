//! Nudges: exactly one durable prompt per finished Implementation turn, driven by reconciliation
//! against a scripted provider (the same seam the fake Herdr serves in the server tests).
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, AgentPrompt, AgentPromptOutcome,
    AgentTurn, Application, DispatchWorkspace,
    nudges::{NudgeIntent, NudgeSettlement, NudgeState},
};
use std::sync::{Arc, Mutex};
#[path = "support/implementation_fixture.rs"]
mod fixture;
use fixture::Fixture;

/// The scripted agent: what Herdr would report and how `agent.prompt` behaves.
struct Script {
    turn: Mutex<AgentTurn>,
    prompts: Mutex<Vec<AgentPrompt>>,
    prompt_result: Mutex<Result<AgentPromptOutcome, AgentAdapterError>>,
}
impl Script {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            turn: Mutex::new(turn(AgentObservation::Working, Some(0))),
            prompts: Mutex::new(vec![]),
            prompt_result: Mutex::new(Ok(AgentPromptOutcome::Delivered)),
        })
    }
    fn report(&self, observation: AgentObservation, seq: u64) {
        *self.turn.lock().unwrap() = turn(observation, Some(seq));
    }
    fn sent(&self) -> Vec<String> {
        self.prompts
            .lock()
            .unwrap()
            .iter()
            .map(|p| p.text.clone())
            .collect()
    }
}
fn turn(observation: AgentObservation, completion_seq: Option<u64>) -> AgentTurn {
    AgentTurn {
        observation,
        completion_seq,
    }
}
struct Factory(Arc<Script>);
impl AgentDispatchAdapterFactory for Factory {
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        Ok(Box::new(Adapter(self.0.clone())))
    }
}
struct Adapter(Arc<Script>);
impl AgentDispatchAdapter for Adapter {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        Ok(())
    }
    fn verify_workspace(&self, id: &str) -> Result<DispatchWorkspace, AgentAdapterError> {
        Ok(DispatchWorkspace { id: id.into() })
    }
    fn provision(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        Ok(())
    }
    fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        Ok(self.0.turn.lock().unwrap().observation)
    }
    fn observe_turn(&self, _: &AgentDispatchContext) -> Result<AgentTurn, AgentAdapterError> {
        Ok(*self.0.turn.lock().unwrap())
    }
    fn prompt(
        &self,
        _: &AgentDispatchContext,
        prompt: &AgentPrompt,
    ) -> Result<AgentPromptOutcome, AgentAdapterError> {
        self.0.prompts.lock().unwrap().push(prompt.clone());
        self.0.prompt_result.lock().unwrap().clone()
    }
    fn recover(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        unreachable!()
    }
    fn retire(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        Ok(())
    }
}

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
