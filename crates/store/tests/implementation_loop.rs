//! The verdict-free Implementation loop end to end on one Task: bootstrap, nudges, Review
//! Request, review handback, a fresh Implementation round, review pass and completion, plus the
//! bounded stops. The Implementation Agent never submits an outcome; only review Checks do.
use forge_app::{
    AgentDispatchRepository, AgentObservation, Application, RecordRepository, RecordStatus,
    assignment::{AgentBootstrap, ImplementationBootstrap},
    checks::Verdict,
    findings::{FindingState, Severity},
    outcomes::{FindingVerification, OutcomeDisposition, OutcomeFinding, OutcomeInput},
};
#[path = "support/scripted_agent.rs"]
mod scripted;
use scripted::{Factory, Script};
#[path = "support/implementation_fixture.rs"]
mod fixture;
use fixture::Fixture;
use std::sync::Arc;

struct World {
    f: Fixture,
    script: Arc<Script>,
    app: Application,
    now: u64,
    commits: u64,
}
impl World {
    fn new() -> Self {
        Self::with(Fixture::new())
    }
    fn with(f: Fixture) -> Self {
        let script = Script::new();
        let app = f.app_with(Box::new(Factory(script.clone())));
        Self {
            f,
            script,
            app,
            now: 0,
            commits: 0,
        }
    }
    /// One reconciliation scan, past the previous scan's lease.
    fn scan(&mut self) {
        self.now += 100_000;
        self.app
            .reconcile_agent_dispatch("implementation-loop", self.now)
            .unwrap();
    }
    fn hold(&self) -> Option<String> {
        self.f
            .store
            .dispatch_hold(self.f.task)
            .unwrap()
            .map(|h| h.reason)
    }
    fn status(&self) -> RecordStatus {
        self.f.store.show_record(self.f.task).unwrap().status
    }
    /// The live Implementation Assignment, acknowledged and started.
    fn implementation(&self) -> String {
        let id = self.f.launch(&self.app);
        let attempt = self.f.store.assignment_check_attempt(&id).unwrap().unwrap();
        assert_eq!(
            attempt.check_id,
            self.f.store.list_record_checks(self.f.task).unwrap()[0].id
        );
        id
    }
    fn bootstrap(&self, id: &str) -> ImplementationBootstrap {
        match self
            .f
            .session(&self.app, id)
            .agent_bootstrap_with_session(None)
            .unwrap()
        {
            AgentBootstrap::Implementation(value) => value,
            AgentBootstrap::Standard(_) => panic!("expected an Implementation bootstrap"),
        }
    }
    /// The Agent commits new work, then asks for review; the next scan launches the review.
    fn request_review(&mut self, id: &str) {
        self.commits += 1;
        self.f.set_head(&format!("{:040x}", self.commits));
        self.f.session(&self.app, id).request_review().unwrap();
        self.scan();
    }
    fn review_input(&self, id: &str, verdict: Verdict) -> OutcomeInput {
        let attempt = self.f.store.assignment_check_attempt(id).unwrap().unwrap();
        OutcomeInput {
            attempt_id: attempt.id,
            generation: attempt.generation,
            evidence: attempt.evidence,
            verdict,
            actor: "ignored".into(),
            message: "Reviewed".into(),
            disposition: OutcomeDisposition::Complete,
            findings: vec![],
            verifications: vec![],
        }
    }
    /// The review Agent hands back with one blocking Finding; the next scan relaunches Implementation.
    fn handback(&mut self, message: &str) {
        let review = self.f.launch(&self.app);
        let mut input = self.review_input(&review, Verdict::Fail);
        input.disposition = OutcomeDisposition::Handback;
        input.findings.push(OutcomeFinding {
            severity: Severity::Major,
            blocking: Some(true),
            message: message.into(),
        });
        self.f
            .session(&self.app, &review)
            .submit_outcome(&input)
            .unwrap();
        self.scan();
    }
}

#[test]
fn the_loop_reaches_done_through_a_nudge_a_handback_and_a_second_round_without_an_implementation_outcome()
 {
    let mut w = World::new();
    let other = w.f.raise_finding("Pre-existing note");

    // Round one: the bootstrap is minimal, the Agent stops, Forge nudges once, then it asks for review.
    let first = w.implementation();
    let boot = w.bootstrap(&first);
    assert_eq!(boot.task_body, "Deliver work");
    assert_eq!(
        boot.findings.iter().map(|f| f.id).collect::<Vec<_>>(),
        vec![other]
    );
    assert!(
        boot.command_contract
            .contains("forge assignment request-review")
    );
    w.script.report(AgentObservation::Idle, 1);
    w.scan();
    assert_eq!(w.script.sent().len(), 1, "one nudge for the finished turn");
    w.script.report(AgentObservation::Working, 1);
    w.request_review(&first);
    assert_eq!(w.script.sent().len(), 1, "no nudge after a Review Request");
    assert!(w.hold().is_none());

    // Review hands back with a Finding; Forge starts a fresh Implementation round.
    w.handback("The report omits the required result");
    let second = w.implementation();
    assert_ne!(second, first);
    let boot = w.bootstrap(&second);
    let open: Vec<&str> = boot.findings.iter().map(|f| f.message.as_str()).collect();
    assert_eq!(open.len(), 2, "every open Finding is listed: {open:?}");
    assert!(open.contains(&"Pre-existing note"));
    assert!(open.contains(&"The report omits the required result"));
    let review_finding = boot
        .findings
        .iter()
        .find(|f| f.message.starts_with("The report"))
        .unwrap()
        .id;
    assert_eq!(
        w.f.store.assignment_nudge_state(&second).unwrap(),
        forge_app::nudges::NudgeState::default(),
        "a new Assignment starts with zero counters"
    );
    w.request_review(&second);

    // The Implementation Check was re-assessed by Review Request, then Review passes and verifies.
    let review = w.f.launch(&w.app);
    let verified =
        w.f.app()
            .show_record_finding(w.f.task, review_finding, None, 100, None)
            .unwrap()
            .finding;
    let mut input = w.review_input(&review, Verdict::Pass);
    input.verifications.push(FindingVerification {
        finding_id: verified.id,
        expected_version: verified.version,
        message: "Verified against the new commit".into(),
    });
    w.f.session(&w.app, &review).submit_outcome(&input).unwrap();
    w.scan();
    w.scan();

    assert_eq!(w.status(), RecordStatus::Done);
    assert!(w.hold().is_none());
    assert!(
        w.f.app()
            .list_record_checks(w.f.task)
            .unwrap()
            .iter()
            .all(|c| c.current_verdict == Some(Verdict::Pass))
    );
    assert_eq!(
        w.f.app()
            .show_record_finding(w.f.task, review_finding, None, 100, None)
            .unwrap()
            .finding
            .state,
        FindingState::Resolved
    );
    // Both Implementation Assignments were closed to verdict commands throughout.
    let assignments = w.f.store.list_agent_assignments(w.f.task).unwrap();
    assert_eq!(assignments.len(), 4);
    assert!(assignments.iter().all(|a| a.closed_at_millis.is_some()));
    for id in [&first, &second] {
        assert_eq!(
            w.f.session(&w.app, id)
                .ensure_not_implementation()
                .unwrap_err()
                .code,
            "implementation-outcome-forbidden"
        );
    }
}

#[test]
fn the_fourth_handback_exhausts_remediation_and_holds_with_the_budget_reset_covering_remediation_only()
 {
    let mut w = World::new();
    for round in 1..=4 {
        let id = w.implementation();
        let boot = w.bootstrap(&id);
        assert_eq!(
            boot.findings.len(),
            round - 1,
            "round {round} lists every earlier open Finding"
        );
        w.request_review(&id);
        assert!(w.hold().is_none(), "round {round}");
        w.handback(&format!("Finding {round}"));
    }
    let reason = w.hold().expect("the fourth handback exhausts remediation");
    assert!(reason.contains("three remediation cycles"), "{reason}");
    assert_eq!(w.status(), RecordStatus::InProgress);
    assert!(w.f.store.list_open_agent_assignments().unwrap().is_empty());
    assert_eq!(
        w.f.store.list_agent_assignments(w.f.task).unwrap().len(),
        8,
        "no ninth launch"
    );

    // Resetting remediation clears the count but not the hold; resume reopens dispatch.
    w.app
        .reset_task_remediation(w.f.task, "operator", "Investigated; fresh budget")
        .unwrap();
    assert!(w.hold().is_some());
    w.f.store.resume_dispatch(w.f.task).unwrap();
    w.scan();
    let id = w.implementation();
    assert_eq!(w.bootstrap(&id).findings.len(), 4);
}

#[test]
fn a_review_request_without_a_new_commit_after_a_handback_stops_on_no_progress() {
    let mut w = World::new();
    let first = w.implementation();
    w.request_review(&first);
    w.handback("Needs work");
    let second = w.implementation();
    // The Agent asks again without committing anything new.
    w.f.session(&w.app, &second).request_review().unwrap();
    w.scan();
    w.handback("Still needs work");
    let reason = w.hold().expect("no progress holds the Task");
    assert!(reason.contains("No progress"), "{reason}");
    assert_eq!(w.status(), RecordStatus::InProgress);
    assert!(w.f.store.list_open_agent_assignments().unwrap().is_empty());
}

#[test]
fn retiring_an_implementation_agent_holds_and_resume_relaunches_the_same_check() {
    let mut w = World::new();
    let id = w.implementation();
    w.app.retire_agent_assignment(&id).unwrap();
    w.scan();
    assert!(w.hold().is_some(), "operator retirement holds the Task");
    assert_eq!(w.f.store.list_agent_assignments(w.f.task).unwrap().len(), 1);

    w.app.resume_task_dispatch(w.f.task, None).unwrap();
    w.scan();
    let retry = w.implementation();
    assert_ne!(retry, id);
    assert_eq!(w.f.store.list_agent_assignments(w.f.task).unwrap().len(), 2);
    assert_eq!(w.bootstrap(&retry).task_body, "Deliver work");
}

#[test]
fn an_interrupted_review_is_retried_twice_and_then_held_while_implementation_stays_passed() {
    let mut w = World::new();
    let id = w.implementation();
    w.request_review(&id);
    for interruption in 1..=3 {
        let review = w.f.launch(&w.app);
        let mut input = w.review_input(&review, Verdict::Fail);
        input.disposition = OutcomeDisposition::Interrupted;
        w.f.session(&w.app, &review).submit_outcome(&input).unwrap();
        w.scan();
        assert_eq!(
            w.hold().is_some(),
            interruption == 3,
            "interruption {interruption}"
        );
    }
    let reason = w.hold().unwrap();
    assert!(reason.contains("two automatic retries"), "{reason}");
    // Three Reviews plus the Implementation Assignment; the passed Implementation is not redone.
    assert_eq!(w.f.store.list_agent_assignments(w.f.task).unwrap().len(), 4);
    assert_eq!(
        w.f.app().list_record_checks(w.f.task).unwrap()[0].current_verdict,
        Some(Verdict::Pass)
    );
    w.app.resume_task_dispatch(w.f.task, None).unwrap();
    w.scan();
    assert_eq!(w.f.store.list_agent_assignments(w.f.task).unwrap().len(), 5);
}

#[test]
fn a_task_pinned_to_an_older_profile_revision_bootstraps_with_the_forge_owned_contract() {
    const OUTCOME_LANGUAGE: &str = "Submit an outcome with fail/complete and list what remains";
    let f = Fixture::with_profile_body(OUTCOME_LANGUAGE);
    let older = f.store.list_record_checks(f.task).unwrap()[0]
        .profile_revision
        .unwrap();
    let newer = f.select_newer_profile("Be concise");
    assert!(newer > older);
    let w = World::with(f);
    let id = w.implementation();
    assert_eq!(
        w.f.store.agent_assignment(&id).unwrap().profile.revision,
        older,
        "the Template keeps the older revision"
    );
    let boot = w.bootstrap(&id);
    assert_eq!(
        boot.command_contract,
        forge_app::assignment::IMPLEMENTATION_COMMAND_CONTRACT
    );
    let json = serde_json::to_string(&boot).unwrap().to_lowercase();
    for forbidden in [
        "fail/complete",
        "what remains",
        "submit an outcome",
        "disposition",
    ] {
        assert!(!json.contains(forbidden), "{forbidden} leaked into {json}");
    }
}
