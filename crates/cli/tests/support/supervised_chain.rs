//! Deliberate Continuation is a fresh successor, never same-attempt recovery.
use super::{
    AgentDispatchRepository, AnswerObservation, Arc, AtomicUsize, RecordRepository,
    SqliteProjectRepository, SupervisedDispatch, fs, redacted_tree, supervised_dispatch,
};

async fn blocked_handoff(
    adapter: &SupervisedDispatch,
    store: &SqliteProjectRepository,
    record: u64,
    report: &mut supervised_dispatch::Report,
) {
    let dispatch = adapter.clone();
    assert!(
        tokio::task::spawn_blocking(move || dispatch.app().execute_task_check_acceptance(record))
            .await
            .unwrap()
            .is_err()
    );
    assert!(
        store
            .agent_assignment(&report.launch.assignment_id)
            .unwrap()
            .closed_at_millis
            .is_some()
    );
    // Owned process cleanup is proven and the production launcher already
    // reconciled its provisioning marker: no unknown-resource confirmation.
    assert!(
        !store
            .agent_assignment_info(&report.launch.assignment_id)
            .unwrap()
            .capacity_reserved()
    );
    let secret = fs::read_to_string(&report.capability_file).unwrap();
    report.finish_retired(&secret);
    // Explicit operator intervention for the controlled grant driver's retained
    // stop hold; never an incidental cleanup/recovery side effect.
    adapter.app().resume_task_dispatch(record).unwrap();
}

fn fresh(next: &supervised_dispatch::Report, previous: &supervised_dispatch::Report) {
    assert_ne!(next.launch.assignment_id, previous.launch.assignment_id);
    assert_ne!(next.launch.attempt_id, previous.launch.attempt_id);
    assert_ne!(next.launch.runtime_id, previous.launch.runtime_id);
    assert_ne!(next.launch.conversation_id, previous.launch.conversation_id);
    assert_ne!(next.launch.store_id, previous.launch.store_id);
    assert_ne!(next.directory, previous.directory);
}

/// Explicit acceptance for the Continuation; the independent Review is launched by
/// normal automatic scheduling (T16), through the release gate, only after accepted
/// evidence and retirement.
async fn launch(
    next: &SupervisedDispatch,
    record: u64,
    mode: &str,
) -> (forge_app::AgentAssignment, supervised_dispatch::Report) {
    let dispatch = next.clone();
    let review = mode == "review";
    tokio::task::spawn_blocking(move || {
        let assignment = if review {
            let app = dispatch.app();
            app.reconcile_agent_dispatch("t16-scheduler", 1).unwrap();
            let store = SqliteProjectRepository::open(&dispatch.database).unwrap();
            store.list_agent_assignments(record).unwrap().remove(0)
        } else {
            let (assignment, replay) = dispatch
                .app()
                .execute_task_check_acceptance(record)
                .unwrap();
            assert!(!replay);
            assignment
        };
        // Provision only spawned the host; observe the production driver.
        let report = dispatch.drive(&assignment);
        (assignment, report)
    })
    .await
    .unwrap()
}

pub(super) async fn run(
    adapter: &SupervisedDispatch,
    store: &SqliteProjectRepository,
    record: u64,
    mut first: supervised_dispatch::Report,
) {
    let first_attempt = store
        .assignment_check_attempt(&first.launch.assignment_id)
        .unwrap()
        .unwrap();
    let checks = store.list_record_checks(record).unwrap();
    assert!(checks[0].latest_assessment.is_none());
    assert!(checks[1].latest_assessment.is_none());
    blocked_handoff(adapter, store, record, &mut first).await;
    let mut reports = vec![first];
    let mut previous_output = None;
    for mode in ["chain-complete", "review"] {
        let mut next = adapter.clone();
        next.mode = mode.into();
        next.provisions = Arc::new(AtomicUsize::new(0));
        next.write_fixture();
        let (assignment, mut report) = launch(&next, record, mode).await;
        assert_eq!(next.provisions.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(report.answer, Some(AnswerObservation::OutcomeAccepted));
        let attempt = store
            .assignment_check_attempt(&assignment.id)
            .unwrap()
            .unwrap();
        for predecessor in &reports {
            fresh(&report, predecessor);
        }
        assert_eq!(
            assignment.profile.revision,
            reports[0].launch.profile_revision
        );
        if mode == "chain-complete" {
            assert_eq!(attempt.check_id, first_attempt.check_id);
            let checkpoint = attempt.continuation.as_ref().unwrap();
            assert_eq!(checkpoint.attempt_id, first_attempt.id);
            assert_eq!(checkpoint.evidence, attempt.evidence);
            assert!(
                checkpoint
                    .handoff
                    .remaining_work
                    .contains("accepted-continuation-handoff")
            );
            let checks = store.list_record_checks(record).unwrap();
            let output = checks[0]
                .latest_assessment
                .as_ref()
                .unwrap()
                .evidence
                .clone();
            assert_ne!(output, attempt.evidence);
            previous_output = Some(output);
        } else {
            assert_ne!(attempt.check_id, first_attempt.check_id);
            assert_eq!(Some(attempt.evidence.clone()), previous_output);
            assert!(attempt.continuation.is_none());
            let checks = store.list_record_checks(record).unwrap();
            assert_eq!(
                checks[1].latest_assessment.as_ref().unwrap().evidence,
                attempt.evidence
            );
        }
        blocked_handoff(&next, store, record, &mut report).await;
        reports.push(report);
    }
    assert_eq!(store.list_agent_assignments(record).unwrap().len(), 3);
    assert!(
        store
            .list_record_checks(record)
            .unwrap()
            .iter()
            .all(|c| c.latest_assessment.is_some())
    );
    let dispatch = adapter.clone();
    let error =
        tokio::task::spawn_blocking(move || dispatch.app().execute_task_check_acceptance(record))
            .await
            .unwrap()
            .unwrap_err();
    assert_eq!(error.code, "checks-passed");
    for report in reports {
        let secret = fs::read_to_string(&report.capability_file).unwrap();
        redacted_tree(&report.directory, &secret);
    }
}
