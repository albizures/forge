//! Review Request: an Implementation Agent's only way to finish, with no verdict of its own.
use forge_app::{
    AgentDispatchRepository, Application, RecordRepository,
    checks::Verdict,
    outcomes::{OutcomeDisposition, OutcomeInput},
};
use std::sync::atomic::Ordering;
#[path = "support/implementation_fixture.rs"]
mod fixture;
use fixture::Fixture;

#[test]
fn accepted_review_request_records_the_observed_head_and_hands_off_to_review() {
    let f = Fixture::new();
    let app = f.app();
    let id = f.launch(&app);
    let session = f.session(&app, &id);
    let committed = "b".repeat(40);
    f.set_head(&committed);

    let accepted = session.request_review().unwrap();
    let implementation = f.store.list_record_checks(f.task).unwrap().remove(0);
    let assessment = implementation.latest_assessment.as_ref().unwrap();
    assert_eq!(assessment.verdict, Verdict::Pass);
    assert_eq!(assessment.assignment_id.as_deref(), Some(id.as_str()));
    assert_eq!(accepted.attempt.id, id);
    assert_eq!(accepted.evidence, f.evidence(&committed));
    assert_eq!(accepted.work.output, Some(f.evidence(&committed)));

    // An exact repeat returns the stored result.
    assert_eq!(session.request_review().unwrap(), accepted);

    // Reconciliation retires the Implementation Assignment and launches review.
    assert_eq!(f.launches.lock().unwrap().len(), 1);
    app.reconcile_agent_dispatch("review-request", 100_000)
        .unwrap();
    assert!(
        f.store
            .agent_assignment(&id)
            .unwrap()
            .closed_at_millis
            .is_some()
    );
    assert_eq!(f.launches.lock().unwrap().len(), 2);
    let review = f
        .store
        .list_agent_assignments(f.task)
        .unwrap()
        .into_iter()
        .find(|a| a.closed_at_millis.is_none())
        .unwrap();
    let checks = f.store.list_record_checks(f.task).unwrap();
    assert_eq!(
        f.store
            .assignment_check_attempt(&review.id)
            .unwrap()
            .unwrap()
            .check_id,
        checks[1].id
    );
    // The retired Assignment's replay still answers, even once the worktree has moved on.
    f.set_head(&"c".repeat(40));
    assert_eq!(session.request_review().unwrap(), accepted);
}

#[test]
fn dirty_worktree_rejects_without_recording_anything() {
    let f = Fixture::new();
    let app = f.app();
    let id = f.launch(&app);
    let session = f.session(&app, &id);
    f.set_head(&"b".repeat(40));
    let work = app.record_work_state(f.task).unwrap();
    let checks = f.store.list_record_checks(f.task).unwrap();
    let updates = f.store.list_record_updates(f.task).unwrap();

    f.dirty.store(true, Ordering::SeqCst);
    let error = session.request_review().unwrap_err();
    assert_eq!(error.code, "review-request-worktree-invalid");
    assert!(error.message.contains("commit"), "{}", error.message);
    assert_eq!(app.record_work_state(f.task).unwrap(), work);
    assert_eq!(f.store.list_record_checks(f.task).unwrap(), checks);
    assert_eq!(f.store.list_record_updates(f.task).unwrap(), updates);
    assert!(f.store.dispatch_hold(f.task).unwrap().is_none());
    assert!(
        f.store
            .agent_assignment(&id)
            .unwrap()
            .closed_at_millis
            .is_none()
    );

    // Committing and cleaning up lets the same attempt request review.
    f.dirty.store(false, Ordering::SeqCst);
    session.request_review().unwrap();
}

#[test]
fn review_check_assignment_cannot_request_review() {
    let f = Fixture::new();
    let app = f.app();
    let id = f.launch(&app);
    f.session(&app, &id).request_review().unwrap();
    app.reconcile_agent_dispatch("review-request", 100_000)
        .unwrap();
    let review = f.launch(&app);
    assert_ne!(review, id);
    let before = f.store.list_record_checks(f.task).unwrap();
    let error = f.session(&app, &review).request_review().unwrap_err();
    assert_eq!(error.code, "review-request-not-implementation");
    assert_eq!(f.store.list_record_checks(f.task).unwrap(), before);
}

#[test]
fn unacknowledged_assignment_cannot_request_review() {
    let f = Fixture::new();
    let app = f.app();
    let (assignment, _) = app.execute_task_check_acceptance(f.task).unwrap();
    std::fs::create_dir_all(&f.secrets).unwrap();
    std::fs::write(f.secrets.join(&assignment.id), "secret").unwrap();
    let before = f.store.list_record_checks(f.task).unwrap();
    let error = f
        .session(&app, &assignment.id)
        .request_review()
        .unwrap_err();
    assert_eq!(error.code, "assignment-inactive");
    assert_eq!(f.store.list_record_checks(f.task).unwrap(), before);
}

const OUTCOME_LANGUAGE: &str = "Submit an outcome with fail/complete";

fn implementation(
    f: &Fixture,
    app: &Application,
) -> forge_app::assignment::ImplementationBootstrap {
    let id = f.acknowledged(app);
    match f
        .session(app, &id)
        .agent_bootstrap_with_session(None)
        .unwrap()
    {
        forge_app::assignment::AgentBootstrap::Implementation(value) => value,
        forge_app::assignment::AgentBootstrap::Standard(_) => panic!("expected Implementation"),
    }
}

#[test]
fn implementation_bootstrap_carries_only_worktree_task_open_findings_and_contract() {
    let f = Fixture::with_profile_body(OUTCOME_LANGUAGE);
    let open = f.raise_finding("Fix the parser\nand its tests");
    let dismissed = f.raise_finding("Not a problem");
    f.app()
        .mutate_record_finding(
            f.task,
            dismissed,
            &forge_app::findings::FindingMutationInput {
                change: forge_app::findings::FindingMutation::Dismiss,
                explanation: forge_app::findings::FindingResolveInput {
                    expected_version: 1,
                    actor: "reviewer".into(),
                    message: "ok".into(),
                    revision: None,
                    dirty: None,
                },
            },
        )
        .unwrap();
    let app = f.app();
    let bootstrap = implementation(&f, &app);
    assert_eq!(bootstrap.task_body, "Deliver work");
    assert_eq!(
        bootstrap.execution_directory,
        f.store
            .assignment_execution_directory(&f.store.list_agent_assignments(f.task).unwrap()[0].id)
            .unwrap()
            .unwrap()
    );
    assert_eq!(bootstrap.findings.len(), 1);
    assert_eq!(bootstrap.findings[0].id, open);
    assert_eq!(
        bootstrap.findings[0].message,
        "Fix the parser\nand its tests"
    );
    assert_eq!(
        bootstrap.command_contract,
        forge_app::assignment::IMPLEMENTATION_COMMAND_CONTRACT
    );
    // The fixed command contract legitimately says "attempt limit"; check the rest for leaks.
    let json = serde_json::to_string(&forge_app::assignment::ImplementationBootstrap {
        command_contract: String::new(),
        ..bootstrap.clone()
    })
    .unwrap();
    for omitted in [
        OUTCOME_LANGUAGE,
        "trusted",
        "attempt",
        "generation",
        "evidence",
        "related",
        "continuation",
    ] {
        assert!(
            !json.to_lowercase().contains(&omitted.to_lowercase()),
            "unexpected {omitted}"
        );
    }
}

#[test]
fn contract_is_identical_across_pinned_profile_revisions() {
    let (a, b) = (
        Fixture::with_profile_body("Be concise"),
        Fixture::with_profile_body(OUTCOME_LANGUAGE),
    );
    let (first, second) = (implementation(&a, &a.app()), implementation(&b, &b.app()));
    assert_eq!(first.command_contract, second.command_contract);
    assert!(!first.command_contract.contains("outcome"));
}

#[test]
fn review_check_bootstrap_keeps_the_full_standard_form() {
    let f = Fixture::new();
    let app = f.app();
    let id = f.launch(&app);
    f.session(&app, &id).request_review().unwrap();
    app.reconcile_agent_dispatch("review-request", 100_000)
        .unwrap();
    let review = f.acknowledged(&app);
    let forge_app::assignment::AgentBootstrap::Standard(standard) = f
        .session(&app, &review)
        .agent_bootstrap_with_session(None)
        .unwrap()
    else {
        panic!("review bootstrap must stay standard")
    };
    assert_eq!(standard.behavior, "Work");
    assert!(!standard.context.trusted_constraints.is_empty());
    assert!(standard.context.bound.check_attempt.is_some());
}

#[test]
fn only_the_implementation_assignment_is_closed_to_verdict_commands() {
    let f = Fixture::new();
    let app = f.app();
    let id = f.launch(&app);
    let session = f.session(&app, &id);
    assert_eq!(
        session.ensure_not_implementation().unwrap_err().code,
        "implementation-outcome-forbidden"
    );
    // The gate records nothing and leaves the Review Request path open.
    f.set_head(&"b".repeat(40));
    session.request_review().unwrap();
    app.reconcile_agent_dispatch("review-request", 100_000)
        .unwrap();
    let review = f.acknowledged(&app);
    f.session(&app, &review)
        .ensure_not_implementation()
        .unwrap();
}

#[test]
fn every_verdict_entry_point_refuses_an_implementation_assignment_and_records_nothing() {
    use forge_app::{
        checks::{AssessmentInput, InvalidationInput},
        findings::{
            FindingMutation, FindingMutationInput, FindingRaiseInput, FindingResolveInput, Severity,
        },
    };
    let f = Fixture::new();
    let app = f.app();
    let id = f.launch(&app);
    let session = f.session(&app, &id);
    let attempt = f.store.assignment_check_attempt(&id).unwrap().unwrap();
    let checks = f.store.list_record_checks(f.task).unwrap();
    let before = (
        checks.clone(),
        f.store.list_record_findings(f.task, None, 100).unwrap(),
    );
    let codes = [
        session
            .submit_outcome(&OutcomeInput {
                attempt_id: attempt.id.clone(),
                generation: attempt.generation,
                evidence: attempt.evidence.clone(),
                verdict: Verdict::Pass,
                actor: "x".into(),
                message: "Self pass".into(),
                disposition: OutcomeDisposition::Complete,
                findings: vec![],
                verifications: vec![],
            })
            .map(|_| ()),
        session
            .check_assess(
                checks[0].id,
                &AssessmentInput {
                    expected_version: 0,
                    verdict: Verdict::Pass,
                    actor: "x".into(),
                    message: "Self pass".into(),
                    revision: None,
                    dirty: None,
                },
            )
            .map(|_| ()),
        session
            .check_invalidate(
                checks[0].id,
                &InvalidationInput {
                    expected_version: 0,
                    assessment_id: 1,
                    actor: "x".into(),
                    message: "No".into(),
                    revision: None,
                    dirty: None,
                },
            )
            .map(|_| ()),
        session
            .finding_raise(&FindingRaiseInput {
                severity: Severity::Major,
                blocking: Some(true),
                check_id: None,
                assessment_id: None,
                actor: "x".into(),
                message: "Self-raised".into(),
                revision: None,
                dirty: None,
            })
            .map(|_| ()),
        session
            .finding_mutate(
                1,
                &FindingMutationInput {
                    change: FindingMutation::Resolve,
                    explanation: FindingResolveInput {
                        expected_version: 1,
                        actor: "x".into(),
                        message: "Self-resolved".into(),
                        revision: None,
                        dirty: None,
                    },
                },
            )
            .map(|_| ()),
    ];
    for result in codes {
        assert_eq!(result.unwrap_err().code, "implementation-outcome-forbidden");
    }
    assert_eq!(
        (
            f.store.list_record_checks(f.task).unwrap(),
            f.store.list_record_findings(f.task, None, 100).unwrap()
        ),
        before
    );
    // Review Request stays open.
    f.set_head(&"b".repeat(40));
    session.request_review().unwrap();
}

#[test]
fn assignment_instructions_withhold_the_profile_body_from_implementation_only() {
    const BODY: &str = "Submit an outcome with fail/complete and list what remains";
    let f = Fixture::with_profile_body(BODY);
    let app = f.app();
    let id = f.launch(&app);
    let instructions = f.session(&app, &id).behavior().unwrap();
    assert_eq!(
        instructions,
        forge_app::assignment::IMPLEMENTATION_COMMAND_CONTRACT
    );
    assert!(!instructions.contains(BODY));

    f.session(&app, &id).request_review().unwrap();
    app.reconcile_agent_dispatch("review-request", 100_000)
        .unwrap();
    let review = f.launch(&app);
    assert_eq!(f.session(&app, &review).behavior().unwrap(), BODY);
}
