use super::check_tests::{attempt, evidence};
use super::*;
use forge_app::check_templates::{CheckCriterion, TemplateReplacement};
use forge_app::checks::Verdict;
use forge_app::findings::{
    FindingMutation, FindingMutationInput, FindingRaiseInput, FindingResolveInput, FindingState,
    Severity,
};
use forge_app::outcomes::{FindingVerification, OutcomeDisposition, OutcomeFinding};

fn reviewed_task(
    repository: &crate::SqliteProjectRepository,
) -> (u64, Vec<forge_app::checks::RecordCheck>) {
    repository
        .register(&project("reviews", "/review-outcomes"))
        .unwrap();
    repository
        .replace_check_template(
            "reviews",
            &TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Two reviews".into(),
                criteria: [
                    forge_app::check_templates::baseline(),
                    ["Review 1", "Review 2"]
                        .map(|name| CheckCriterion {
                            name: name.into(),
                            description: "Review deliverable".into(),
                            profile_revision: None,
                        })
                        .to_vec(),
                ]
                .concat(),
            },
        )
        .unwrap();
    let task = repository
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver".into(),
            project_slugs: vec!["reviews".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    repository.set_record_execution_mode(task.id, forge_app::ExecutionMode::Dispatched, forge_app::ExecutionMode::Manual, "fixture", "Manual reviews").unwrap();
    repository.start_record(task.id).unwrap();
    (task.id, repository.list_record_checks(task.id).unwrap())
}

#[test]
#[allow(clippy::too_many_lines)]
fn substantive_handback_is_atomic_and_original_checks_rerun_in_order() {
    let repository = SqliteProjectRepository::in_memory().unwrap();
    let (task, checks) = reviewed_task(&repository);
    for (index, check) in checks[..2].iter().enumerate() {
        let input = attempt(&repository, task, check.id, &format!("initial-{index}"));
        repository.submit_record_outcome(task, &input).unwrap();
    }
    let mut handback = attempt(&repository, task, checks[2].id, "handback");
    handback.verdict = Verdict::Fail;
    handback.disposition = OutcomeDisposition::Handback;
    handback.findings.push(OutcomeFinding {
        severity: Severity::Major,
        blocking: Some(true),
        message: "Correct the report".into(),
    });
    let result = repository.submit_record_outcome(task, &handback).unwrap();
    assert_eq!(result.work.generation, 2);
    assert_eq!(result.findings.len(), 1);
    assert!(
        repository
            .list_record_checks(task)
            .unwrap()
            .iter()
            .all(|c| c.current_verdict.is_none())
    );
    assert_eq!(result.findings[0].verification_check_id, Some(checks[2].id));
    assert_eq!(
        result.check.latest_assessment.as_ref().unwrap().verdict,
        Verdict::Fail
    );
    assert!(
        result
            .check
            .latest_assessment
            .as_ref()
            .unwrap()
            .invalidation
            .is_some()
    );
    assert_eq!(
        repository.submit_record_outcome(task, &handback).unwrap(),
        result
    );
    let finding = &result.findings[0];
    let mut implementation = attempt(&repository, task, checks[0].id, "remediation");
    implementation.verifications.push(FindingVerification {
        finding_id: finding.id,
        expected_version: finding.version,
        message: "I fixed it".into(),
    });
    let before = repository.list_record_updates(task).unwrap();
    assert_eq!(
        repository
            .submit_record_outcome(task, &implementation)
            .unwrap_err()
            .code,
        "unauthorized-verification"
    );
    assert_eq!(repository.list_record_updates(task).unwrap(), before);
    implementation.verifications.clear();
    implementation.evidence = forge_app::outcomes::Evidence::Artifact {
        uri: "urn:test:remediated-deliverable".into(),
        sha256: "b".repeat(64),
    };
    repository
        .submit_record_outcome(task, &implementation)
        .unwrap();
    let review = attempt(&repository, task, checks[1].id, "rerun-1");
    repository.submit_record_outcome(task, &review).unwrap();
    let mut review = attempt(&repository, task, checks[2].id, "rerun-2");
    assert_eq!(
        repository
            .submit_record_outcome(task, &review)
            .unwrap_err()
            .code,
        "unresolved-check-findings"
    );
    assert_eq!(
        repository
            .list_record_findings(task, None, 100)
            .unwrap()
            .findings[0]
            .state,
        FindingState::Open
    );
    review.verifications.push(FindingVerification {
        finding_id: finding.id,
        expected_version: finding.version,
        message: "Fix verified against new evidence".into(),
    });
    let accepted = repository.submit_record_outcome(task, &review).unwrap();
    assert_eq!(accepted.findings[0].state, FindingState::Resolved);
    assert!(accepted.blocking_findings.is_empty());
    let after = repository.list_record_checks(task).unwrap();
    assert_eq!(
        after
            .iter()
            .map(|c| (c.id, c.profile_revision))
            .collect::<Vec<_>>(),
        checks
            .iter()
            .map(|c| (c.id, c.profile_revision))
            .collect::<Vec<_>>()
    );
    assert!(
        after
            .iter()
            .all(|c| c.current_verdict == Some(Verdict::Pass))
    );
    assert_eq!(
        repository.show_record(task).unwrap().status,
        RecordStatus::InProgress
    );
    assert_eq!(
        repository.record_work_state(task).unwrap().input,
        Some(evidence())
    );
    assert_eq!(
        repository.record_work_state(task).unwrap().output,
        Some(implementation.evidence)
    );
}

#[test]
fn stale_handback_and_cross_task_verification_publish_no_partial_effects() {
    let repository = SqliteProjectRepository::in_memory().unwrap();
    let (task, checks) = reviewed_task(&repository);
    for (index, check) in checks[..2].iter().enumerate() {
        let input = attempt(&repository, task, check.id, &format!("initial-{index}"));
        repository.submit_record_outcome(task, &input).unwrap();
    }
    let other = super::check_tests::task(&repository);
    let foreign = repository
        .raise_record_finding(
            other.id,
            &FindingRaiseInput {
                severity: Severity::Major,
                blocking: Some(true),
                check_id: None,
                assessment_id: None,
                actor: "operator".into(),
                message: "Other Task concern".into(),
                revision: None,
                dirty: None,
            },
        )
        .unwrap();
    let mut review = attempt(&repository, task, checks[2].id, "review");
    review.findings.push(OutcomeFinding {
        severity: Severity::Minor,
        blocking: None,
        message: "New local concern".into(),
    });
    review.verifications.push(FindingVerification {
        finding_id: foreign.id,
        expected_version: foreign.version,
        message: "Verify foreign issue".into(),
    });
    let before = repository.list_record_updates(task).unwrap();
    assert_eq!(
        repository
            .submit_record_outcome(task, &review)
            .unwrap_err()
            .code,
        "finding-not-found"
    );
    assert_eq!(repository.list_record_updates(task).unwrap(), before);
    assert!(
        repository
            .list_record_findings(task, None, 100)
            .unwrap()
            .findings
            .is_empty()
    );
    assert_eq!(
        repository
            .show_record_finding(other.id, foreign.id, None, 100)
            .unwrap()
            .finding,
        foreign
    );
    review.verifications.clear();
    review.verdict = Verdict::Fail;
    review.disposition = OutcomeDisposition::Handback;
    repository
        .invalidate_record_work(
            task,
            &forge_app::outcomes::WorkInvalidationInput {
                generation: 1,
                evidence: evidence(),
                actor: "operator".into(),
                message: "Same evidence, invalidate old review".into(),
            },
        )
        .unwrap();
    let before = repository.list_record_updates(task).unwrap();
    assert_eq!(
        repository
            .submit_record_outcome(task, &review)
            .unwrap_err()
            .code,
        "stale-generation"
    );
    assert_eq!(repository.list_record_updates(task).unwrap(), before);
    assert!(
        repository
            .list_record_findings(task, None, 100)
            .unwrap()
            .findings
            .is_empty()
    );
    assert_eq!(repository.record_work_state(task).unwrap().generation, 2);
}

#[test]
fn interruption_closes_attempt_without_substantive_effects_or_handback() {
    let repository = SqliteProjectRepository::in_memory().unwrap();
    let (task, checks) = reviewed_task(&repository);
    let implementation = attempt(&repository, task, checks[0].id, "implementation");
    repository
        .submit_record_outcome(task, &implementation)
        .unwrap();
    let mut interrupted = attempt(&repository, task, checks[1].id, "interrupted");
    interrupted.verdict = Verdict::Fail;
    interrupted.disposition = OutcomeDisposition::Interrupted;
    let before = repository.list_record_checks(task).unwrap();
    let result = repository
        .submit_record_outcome(task, &interrupted)
        .unwrap();
    assert_eq!(repository.list_record_checks(task).unwrap(), before);
    assert_eq!(result.work.generation, 1);
    assert_eq!(
        repository
            .submit_record_outcome(task, &interrupted)
            .unwrap(),
        result
    );
    let retry = attempt(&repository, task, checks[1].id, "retry");
    repository.submit_record_outcome(task, &retry).unwrap();
}

#[test]
fn unlinked_blockers_retain_hold_and_require_explicit_review_owner() {
    let repository = SqliteProjectRepository::in_memory().unwrap();
    let (task, checks) = reviewed_task(&repository);
    let finding = repository
        .raise_record_finding(
            task,
            &FindingRaiseInput {
                severity: Severity::Major,
                blocking: Some(true),
                check_id: None,
                assessment_id: None,
                actor: "operator".into(),
                message: "External concern".into(),
                revision: None,
                dirty: None,
            },
        )
        .unwrap();
    for (index, check) in checks.iter().enumerate() {
        let input = attempt(&repository, task, check.id, &format!("pass-{index}"));
        let result = repository.submit_record_outcome(task, &input).unwrap();
        assert_eq!(result.blocking_findings, vec![finding.id]);
    }
    let hold = AgentDispatchRepository::dispatch_hold(&repository, task)
        .unwrap()
        .unwrap();
    assert!(hold.reason.contains("blocking Findings"));
    let explanation = FindingResolveInput {
        expected_version: finding.version,
        actor: "operator".into(),
        message: "Assign verification".into(),
        revision: None,
        dirty: None,
    };
    assert_eq!(
        repository
            .mutate_record_finding(
                task,
                finding.id,
                &FindingMutationInput {
                    change: FindingMutation::AssignVerification {
                        check_id: checks[0].id
                    },
                    explanation: explanation.clone()
                }
            )
            .unwrap_err()
            .code,
        "invalid-verification-owner"
    );
    let assigned = repository
        .mutate_record_finding(
            task,
            finding.id,
            &FindingMutationInput {
                change: FindingMutation::AssignVerification {
                    check_id: checks[2].id,
                },
                explanation,
            },
        )
        .unwrap();
    assert_eq!(assigned.check_id, None);
    assert_eq!(assigned.verification_check_id, Some(checks[2].id));
    assert_eq!(assigned.state, FindingState::Open);
    let explanation = FindingResolveInput {
        expected_version: assigned.version,
        actor: "implementation".into(),
        message: "Claim fixed".into(),
        revision: None,
        dirty: None,
    };
    assert_eq!(
        repository
            .resolve_record_finding(task, finding.id, &explanation)
            .unwrap_err()
            .code,
        "verification-required"
    );
    assert_eq!(
        AgentDispatchRepository::dispatch_hold(&repository, task)
            .unwrap()
            .unwrap(),
        hold
    );
}

#[test]
fn handback_and_verification_roll_back_together_and_replay_after_restart() {
    let path = std::env::temp_dir().join(format!(
        "forge-review-outcomes-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = SqliteProjectRepository::open(&path).unwrap();
    let (task, checks) = reviewed_task(&repository);
    for (index, check) in checks[..2].iter().enumerate() {
        let input = attempt(&repository, task, check.id, &format!("initial-{index}"));
        repository.submit_record_outcome(task, &input).unwrap();
    }
    let mut handback = attempt(&repository, task, checks[2].id, "handback");
    handback.verdict = Verdict::Fail;
    handback.disposition = OutcomeDisposition::Handback;
    handback.findings.push(OutcomeFinding {
        severity: Severity::Major,
        blocking: None,
        message: "Wrong answer".into(),
    });
    let before = repository.list_record_checks(task).unwrap();
    let updates = repository.list_record_updates(task).unwrap();
    let fault = rusqlite::Connection::open(&path).unwrap();
    fault.execute_batch("CREATE TRIGGER fail_outcome BEFORE UPDATE OF result_json ON record_attempts BEGIN SELECT RAISE(ABORT, 'injected final acceptance failure'); END;").unwrap();
    assert!(repository.submit_record_outcome(task, &handback).is_err());
    assert_eq!(repository.list_record_checks(task).unwrap(), before);
    assert_eq!(repository.list_record_updates(task).unwrap(), updates);
    assert!(
        repository
            .list_record_findings(task, None, 100)
            .unwrap()
            .findings
            .is_empty()
    );
    assert_eq!(repository.record_work_state(task).unwrap().generation, 1);
    assert!(
        AgentDispatchRepository::dispatch_hold(&repository, task)
            .unwrap()
            .is_none()
    );
    fault.execute_batch("DROP TRIGGER fail_outcome").unwrap();
    let accepted = repository.submit_record_outcome(task, &handback).unwrap();
    let hold = AgentDispatchRepository::dispatch_hold(&repository, task).unwrap();
    drop(repository);
    let repository = SqliteProjectRepository::open(&path).unwrap();
    assert_eq!(
        repository.submit_record_outcome(task, &handback).unwrap(),
        accepted
    );
    assert_eq!(
        AgentDispatchRepository::dispatch_hold(&repository, task).unwrap(),
        hold
    );
    for (index, check) in checks[..2].iter().enumerate() {
        let input = attempt(&repository, task, check.id, &format!("rerun-{index}"));
        repository.submit_record_outcome(task, &input).unwrap();
    }
    let mut review = attempt(&repository, task, checks[2].id, "verify");
    let finding = &accepted.findings[0];
    review.verifications.push(FindingVerification {
        finding_id: finding.id,
        expected_version: finding.version,
        message: "Verified corrected answer".into(),
    });
    let before = repository.list_record_checks(task).unwrap();
    let updates = repository.list_record_updates(task).unwrap();
    fault.execute_batch("CREATE TRIGGER fail_verify BEFORE UPDATE OF result_json ON record_attempts BEGIN SELECT RAISE(ABORT, 'injected final acceptance failure'); END;").unwrap();
    assert!(repository.submit_record_outcome(task, &review).is_err());
    assert_eq!(repository.list_record_checks(task).unwrap(), before);
    assert_eq!(repository.list_record_updates(task).unwrap(), updates);
    assert_eq!(
        repository
            .show_record_finding(task, finding.id, None, 100)
            .unwrap()
            .finding,
        *finding
    );
    fault.execute_batch("DROP TRIGGER fail_verify").unwrap();
    let verified = repository.submit_record_outcome(task, &review).unwrap();
    drop(repository);
    let repository = SqliteProjectRepository::open(&path).unwrap();
    assert_eq!(
        repository.submit_record_outcome(task, &review).unwrap(),
        verified
    );
    assert_eq!(
        repository.submit_record_outcome(task, &handback).unwrap(),
        accepted
    );
    assert_eq!(
        AgentDispatchRepository::dispatch_hold(&repository, task).unwrap(),
        hold,
        "verification must not clear an operator-owned hold"
    );
    drop(repository);
    drop(fault);
    std::fs::remove_file(path).unwrap();
}
