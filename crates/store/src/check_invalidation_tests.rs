use super::check_tests::{attempt, evidence, task};
use super::*;
use forge_app::checks::{InvalidationInput, RevisionComparison, Verdict};
use forge_app::outcomes::{AttemptInput, Evidence, WorkInvalidationInput};
use forge_app::{Application, RepositoryInspector};

struct FakeInspector;
impl RepositoryInspector for FakeInspector {
    fn inspect(&self, _: &std::path::Path) -> Result<Project, RegistrationError> {
        panic!("Manual evidence must never inspect Git")
    }
}
fn invalidate(repository: &dyn RecordRepository, task: u64) {
    repository
        .invalidate_record_work(
            task,
            &WorkInvalidationInput {
                generation: repository.record_work_state(task).unwrap().generation,
                evidence: evidence(),
                actor: "operator".into(),
                message: "Same immutable deliverable, new freshness".into(),
            },
        )
        .unwrap();
}

#[test]
fn invalidation_never_revives_history_and_pages_preserve_immutable_attempt_evidence() {
    let repository = SqliteProjectRepository::in_memory().unwrap();
    let task = task(&repository);
    repository.start_record(task.id).unwrap();
    let check = repository.list_record_checks(task.id).unwrap().remove(0);
    let input = attempt(&repository, task.id, check.id, "first");
    let accepted = repository.submit_record_outcome(task.id, &input).unwrap();
    invalidate(&repository, task.id);
    assert_eq!(repository.record_work_state(task.id).unwrap().generation, 2);
    let pending = repository.list_record_checks(task.id).unwrap().remove(0);
    assert_eq!(pending.current_verdict, None);
    assert!(
        pending
            .latest_assessment
            .as_ref()
            .unwrap()
            .invalidation
            .is_some()
    );
    assert_eq!(
        repository.submit_record_outcome(task.id, &input).unwrap(),
        accepted,
        "replay returns original result, not current freshness"
    );
    assert_eq!(
        repository.list_record_checks(task.id).unwrap().remove(0),
        pending
    );
    let mut second = attempt(&repository, task.id, check.id, "second");
    second.verdict = Verdict::Fail;
    repository.submit_record_outcome(task.id, &second).unwrap();
    let history = repository
        .show_record_check(task.id, check.id, None, 100)
        .unwrap();
    assert_eq!(history.assessments.len(), 2);
    assert_eq!(history.activities.len(), 3);
    assert_eq!(history.assessments[0].attempt_id, "first");
    assert_eq!(history.assessments[0].generation, 1);
    assert_eq!(history.assessments[0].evidence, evidence());
    assert_eq!(history.assessments[1].verdict, Verdict::Fail);
    assert_eq!(history.check.current_verdict, Some(Verdict::Fail));
    let mut cursor = None;
    let mut ids = vec![];
    loop {
        let page = repository
            .show_record_check(task.id, check.id, cursor, 1)
            .unwrap();
        ids.extend(page.activities.iter().map(|a| a.id));
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        ids,
        history.activities.iter().map(|a| a.id).collect::<Vec<_>>()
    );
    for limit in [0, 101] {
        assert_eq!(
            repository
                .show_record_check(task.id, check.id, None, limit)
                .unwrap_err()
                .code,
            "invalid-page"
        );
    }
}

#[test]
fn same_commit_invalidation_stale_actor_and_body_edits_cannot_reuse_attempts() {
    let repository = SqliteProjectRepository::in_memory().unwrap();
    let task = task(&repository);
    repository.start_record(task.id).unwrap();
    let check = repository.list_record_checks(task.id).unwrap().remove(0);
    let input = attempt(&repository, task.id, check.id, "old");
    let before = repository.list_record_updates(task.id).unwrap();
    let mut unauthorized = input.clone();
    unauthorized.actor = "other".into();
    assert_eq!(
        repository
            .submit_record_outcome(task.id, &unauthorized)
            .unwrap_err()
            .code,
        "unauthorized-attempt"
    );
    assert_eq!(repository.list_record_updates(task.id).unwrap(), before);
    invalidate(&repository, task.id);
    assert_eq!(
        repository
            .submit_record_outcome(task.id, &input)
            .unwrap_err()
            .code,
        "stale-generation"
    );
    let current = attempt(&repository, task.id, check.id, "new");
    repository
        .edit_record(task.id, "Changed Task deliverables")
        .unwrap();
    assert_eq!(repository.record_work_state(task.id).unwrap().generation, 3);
    assert_eq!(
        repository
            .submit_record_outcome(task.id, &current)
            .unwrap_err()
            .code,
        "stale-generation"
    );
    assert!(
        repository
            .show_record_check(task.id, check.id, None, 100)
            .unwrap()
            .assessments
            .is_empty()
    );
    invalidate(&repository, task.id);
    assert_eq!(repository.record_work_state(task.id).unwrap().generation, 4);
}

#[test]
fn check_invalidation_advances_work_generation_atomically_and_failure_rolls_back() {
    let path = std::env::temp_dir().join(format!(
        "forge-invalidation-rollback-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = SqliteProjectRepository::open(&path).unwrap();
    let fault = rusqlite::Connection::open(&path).unwrap();
    let task = task(&repository);
    repository.start_record(task.id).unwrap();
    let check = repository.list_record_checks(task.id).unwrap().remove(0);
    let input = attempt(&repository, task.id, check.id, "invalidate");
    let accepted = repository.submit_record_outcome(task.id, &input).unwrap();
    let invalidation = InvalidationInput {
        expected_version: accepted.check.version,
        assessment_id: accepted.check.latest_assessment.as_ref().unwrap().id,
        actor: "operator".into(),
        message: "Withdraw".into(),
        revision: None,
        dirty: None,
    };
    fault.execute_batch("CREATE TRIGGER fail_invalidate BEFORE INSERT ON record_updates WHEN NEW.kind = 'check-invalidated' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").unwrap();
    assert!(
        repository
            .invalidate_record_check(task.id, check.id, &invalidation)
            .is_err()
    );
    assert_eq!(repository.record_work_state(task.id).unwrap().generation, 1);
    assert_eq!(
        repository.list_record_checks(task.id).unwrap(),
        vec![accepted.check.clone()]
    );
    fault.execute_batch("DROP TRIGGER fail_invalidate").unwrap();
    let pending = repository
        .invalidate_record_check(task.id, check.id, &invalidation)
        .unwrap();
    assert_eq!(pending.current_verdict, None);
    assert_eq!(repository.record_work_state(task.id).unwrap().generation, 2);
    assert_eq!(
        repository
            .invalidate_record_check(task.id, check.id, &invalidation)
            .unwrap_err()
            .code,
        "stale-version"
    );
    drop(fault);
    drop(repository);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[allow(clippy::too_many_lines)]
fn fixed_order_requires_implementation_output_and_failed_implementation_never_advances_reviews() {
    use forge_app::check_templates::{CheckCriterion, TemplateReplacement};
    let repository = SqliteProjectRepository::in_memory().unwrap();
    repository
        .register(&project("alpha", "/manual-order"))
        .unwrap();
    repository
        .replace_check_template(
            "alpha",
            &TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Two reviews".into(),
                criteria: [
                    forge_app::check_templates::baseline(),
                    ["Review 1", "Review 2"]
                        .map(|name| CheckCriterion {
                            name: name.into(),
                            description: "Review output".into(),
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
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    repository.start_record(task.id).unwrap();
    let checks = repository.list_record_checks(task.id).unwrap();
    let review = AttemptInput {
        id: "review".into(),
        check_id: checks[1].id,
        generation: 1,
        evidence: evidence(),
        actor: "operator".into(),
    };
    assert_eq!(
        repository
            .begin_record_attempt(task.id, &review)
            .unwrap_err()
            .code,
        "check-order"
    );
    let mut implementation = attempt(&repository, task.id, checks[0].id, "failed");
    implementation.verdict = Verdict::Fail;
    implementation.message = "Need more research".into();
    repository
        .submit_record_outcome(task.id, &implementation)
        .unwrap();
    assert_eq!(
        repository
            .begin_record_attempt(task.id, &review)
            .unwrap_err()
            .code,
        "check-order"
    );
    let mut implementation = attempt(&repository, task.id, checks[0].id, "passed");
    let output = Evidence::Artifact {
        uri: "urn:report:final".into(),
        sha256: "b".repeat(64),
    };
    implementation.evidence = output.clone();
    repository
        .submit_record_outcome(task.id, &implementation)
        .unwrap();
    assert_eq!(
        repository
            .begin_record_attempt(task.id, &review)
            .unwrap_err()
            .code,
        "evidence-mismatch"
    );
    let second = AttemptInput {
        id: "second-review".into(),
        check_id: checks[2].id,
        evidence: output.clone(),
        ..review.clone()
    };
    assert_eq!(
        repository
            .begin_record_attempt(task.id, &second)
            .unwrap_err()
            .code,
        "check-order"
    );
    let first = attempt(&repository, task.id, checks[1].id, "first-review");
    let mut wrong = first.clone();
    wrong.evidence = evidence();
    assert_eq!(
        repository
            .submit_record_outcome(task.id, &wrong)
            .unwrap_err()
            .code,
        "evidence-mismatch"
    );
    repository.submit_record_outcome(task.id, &first).unwrap();
    let last = attempt(&repository, task.id, checks[2].id, "last-review");
    repository.submit_record_outcome(task.id, &last).unwrap();
    assert_eq!(
        repository.show_record(task.id).unwrap().status,
        RecordStatus::InProgress
    );
    invalidate(&repository, task.id);
    assert!(
        repository
            .list_record_checks(task.id)
            .unwrap()
            .iter()
            .all(|c| c.current_verdict.is_none())
    );
    assert_eq!(
        repository
            .begin_record_attempt(
                task.id,
                &AttemptInput {
                    generation: 2,
                    ..second
                }
            )
            .unwrap_err()
            .code,
        "check-order"
    );
    let implementation = attempt(&repository, task.id, checks[0].id, "remediation");
    repository
        .submit_record_outcome(task.id, &implementation)
        .unwrap();
    let first = attempt(&repository, task.id, checks[1].id, "rerun-first");
    repository.submit_record_outcome(task.id, &first).unwrap();
    let late = attempt(&repository, task.id, checks[2].id, "late-review");
    invalidate(&repository, task.id);
    assert_eq!(
        repository
            .submit_record_outcome(task.id, &late)
            .unwrap_err()
            .code,
        "stale-generation"
    );
    assert!(
        repository
            .list_record_checks(task.id)
            .unwrap()
            .iter()
            .all(|c| c.current_verdict.is_none())
    );
}

#[test]
fn application_compares_repository_evidence_without_git_and_cannot_change_authority() {
    let repository = SqliteProjectRepository::in_memory().unwrap();
    repository
        .register(&project("alpha", "/not-a-git-repository"))
        .unwrap();
    let task = repository
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    repository.start_record(task.id).unwrap();
    let check = repository.list_record_checks(task.id).unwrap().remove(0);
    let mut input = attempt(&repository, task.id, check.id, "repository");
    input.evidence = Evidence::Repository {
        repository: "alpha".into(),
        commit: "a".repeat(40),
    };
    repository.submit_record_outcome(task.id, &input).unwrap();
    let app = Application::new(Box::new(repository), Box::new(FakeInspector));
    assert_eq!(
        app.compare_record_checks(task.id, Some(&"A".repeat(40)))
            .unwrap()[0]
            .latest_assessment
            .as_ref()
            .unwrap()
            .comparison,
        RevisionComparison::Matching
    );
    assert_eq!(
        app.compare_record_check(task.id, check.id, None, 100, Some(&"b".repeat(40)))
            .unwrap()
            .assessments[0]
            .comparison,
        RevisionComparison::Mismatching
    );
    assert_eq!(
        app.list_record_checks(task.id).unwrap()[0]
            .latest_assessment
            .as_ref()
            .unwrap()
            .comparison,
        RevisionComparison::Unknown
    );
    assert_eq!(
        app.compare_record_checks(task.id, Some("short"))
            .unwrap_err()
            .code,
        "invalid-revision"
    );
    let accepted = app.submit_record_outcome(task.id, &input).unwrap();
    let work = app
        .invalidate_record_work(
            task.id,
            &WorkInvalidationInput {
                generation: 1,
                evidence: input.evidence.clone(),
                actor: "operator".into(),
                message: "Recheck unchanged commit".into(),
            },
        )
        .unwrap();
    assert_eq!(work.generation, 2);
    assert_eq!(work.input, Some(input.evidence.clone()));
    assert_eq!(
        app.submit_record_outcome(task.id, &input).unwrap(),
        accepted
    );
    assert_eq!(
        app.list_record_checks(task.id).unwrap()[0].current_verdict,
        None
    );
    app.begin_record_attempt(
        task.id,
        &AttemptInput {
            id: "same-commit-new-generation".into(),
            check_id: check.id,
            generation: 2,
            evidence: input.evidence.clone(),
            actor: "operator".into(),
        },
    )
    .unwrap();
    let current = forge_app::outcomes::OutcomeInput {
        attempt_id: "same-commit-new-generation".into(),
        generation: 2,
        ..input
    };
    assert_eq!(
        app.submit_record_outcome(task.id, &current)
            .unwrap()
            .check
            .current_verdict,
        Some(Verdict::Pass)
    );
}
