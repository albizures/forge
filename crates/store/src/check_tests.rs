use super::*;
use forge_app::checks::{AssessmentInput, Verdict};
use forge_app::outcomes::{AttemptInput, Evidence, OutcomeInput, WorkInvalidationInput};

pub(super) fn evidence() -> Evidence {
    Evidence::Artifact {
        uri: "urn:test:deliverable".into(),
        sha256: "a".repeat(64),
    }
}
pub(super) fn task(repository: &dyn RecordRepository) -> Record {
    repository
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver the work".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap()
}
pub(super) fn attempt(
    repository: &dyn RecordRepository,
    task: u64,
    check: u64,
    id: &str,
) -> OutcomeInput {
    let work = repository.record_work_state(task).unwrap();
    let criterion = repository
        .show_record_check(task, check, None, 1)
        .unwrap()
        .check;
    let evidence = if criterion.position == 0 {
        work.input
    } else {
        work.output
    }
    .unwrap_or_else(evidence);
    repository
        .begin_record_attempt(
            task,
            &AttemptInput {
                id: id.into(),
                check_id: check,
                generation: work.generation,
                evidence: evidence.clone(),
                actor: "operator".into(),
            },
        )
        .unwrap();
    OutcomeInput {
        attempt_id: id.into(),
        generation: work.generation,
        evidence,
        verdict: Verdict::Pass,
        actor: "operator".into(),
        message: "Verified deliverables".into(),
    }
}
pub(super) fn assess(repository: &dyn RecordRepository, task: u64, verdict: Verdict) {
    let check = repository.list_record_checks(task).unwrap().remove(0);
    assess_check(repository, task, check.id, verdict);
}
pub(super) fn assess_check(
    repository: &dyn RecordRepository,
    task: u64,
    check: u64,
    verdict: Verdict,
) {
    if repository.show_record(task).unwrap().status == RecordStatus::Open {
        repository.start_record(task).unwrap();
    }
    let criterion = repository
        .show_record_check(task, check, None, 1)
        .unwrap()
        .check;
    if criterion.current_verdict == Some(Verdict::Pass) {
        repository
            .invalidate_record_work(
                task,
                &WorkInvalidationInput {
                    generation: repository.record_work_state(task).unwrap().generation,
                    evidence: evidence(),
                    actor: "operator".into(),
                    message: "Reassess deliverables".into(),
                },
            )
            .unwrap();
    }
    let mut input = attempt(
        repository,
        task,
        check,
        &format!("test-{task}-{check}-{}", criterion.version),
    );
    input.verdict = verdict;
    repository.submit_record_outcome(task, &input).unwrap();
}

#[test]
fn project_template_replacement_snapshots_future_tasks_only() {
    use forge_app::check_templates::{CheckCriterion, TemplateReplacement};
    let repository = SqliteProjectRepository::in_memory().unwrap();
    repository
        .register(&project("alpha", "/template-alpha"))
        .unwrap();
    let open = || {
        repository
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "Deliver".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap()
    };
    let old = open();
    let initial = repository.show_check_template("alpha", None, 1).unwrap();
    assert_eq!(initial.version, 0);
    assert_eq!(initial.criteria, forge_app::check_templates::baseline());
    let input = TemplateReplacement {
        expected_version: 0,
        actor: "operator".into(),
        message: "Require review".into(),
        criteria: [
            forge_app::check_templates::baseline(),
            vec![CheckCriterion {
                name: " Review ".into(),
                description: " Reviewed deliverables ".into(),
                profile_revision: None,
            }],
        ]
        .concat(),
    };
    assert_eq!(
        repository
            .replace_check_template("alpha", &input)
            .unwrap()
            .version,
        1
    );
    let new = open();
    assert_eq!(repository.list_record_checks(old.id).unwrap().len(), 1);
    let checks = repository.list_record_checks(new.id).unwrap();
    assert_eq!(
        checks.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["Implementation", "Review"]
    );
    assert_eq!(checks[1].description, "Reviewed deliverables");
    assert_eq!(checks[1].position, 1);
    assert_eq!(
        repository
            .replace_check_template("alpha", &input)
            .unwrap_err()
            .code,
        "stale-version"
    );
    repository
        .replace_check_template(
            "alpha",
            &TemplateReplacement {
                expected_version: 1,
                criteria: forge_app::check_templates::baseline(),
                ..input
            },
        )
        .unwrap();
    assert_eq!(repository.list_record_checks(open().id).unwrap().len(), 1);
    assert_eq!(repository.list_record_checks(new.id).unwrap(), checks);
    let page = repository.show_check_template("alpha", None, 1).unwrap();
    assert_eq!(page.version, 2);
    assert_eq!(page.history.len(), 1);
    assert_eq!(page.history[0].actor, "operator");
    assert_eq!(page.history[0].criteria[1].name, "Review");
    let next = repository
        .show_check_template("alpha", page.next_cursor, 1)
        .unwrap();
    assert_eq!(
        next.history[0].criteria,
        forge_app::check_templates::baseline()
    );
    assert!(next.next_cursor.is_none());
}

#[test]
fn implementation_check_gates_completion_and_unbound_assessments_cannot_bypass_attempts() {
    let repository = SqliteProjectRepository::in_memory().unwrap();
    let task = task(&repository);
    let check = repository.list_record_checks(task.id).unwrap().remove(0);
    assert_eq!(check.name, "Implementation");
    assert_eq!(
        check.description,
        forge_app::checks::IMPLEMENTATION_CRITERION
    );
    assert_eq!(check.latest_assessment, None);
    repository.start_record(task.id).unwrap();
    assert_eq!(
        repository.done_record(task.id, None).unwrap_err().code,
        "checks-not-passed"
    );
    let unbound = AssessmentInput {
        expected_version: 1,
        verdict: Verdict::Pass,
        actor: "operator".into(),
        message: "Verified".into(),
        revision: Some("a".repeat(40)),
        dirty: Some(false),
    };
    let updates = repository.list_record_updates(task.id).unwrap();
    assert_eq!(
        repository
            .assess_record_check(task.id, check.id, &unbound)
            .unwrap_err()
            .code,
        "attempt-required"
    );
    assert_eq!(repository.list_record_updates(task.id).unwrap(), updates);
    assess(&repository, task.id, Verdict::Fail);
    assert_eq!(
        repository.done_record(task.id, None).unwrap_err().code,
        "checks-not-passed"
    );
    assess(&repository, task.id, Verdict::Pass);
    repository.done_record(task.id, None).unwrap();
    assert_eq!(
        repository.show_record(task.id).unwrap().status,
        RecordStatus::Done
    );
}

#[test]
fn outcome_failure_rolls_back_assessment_activity_updates_and_acceptance() {
    let path = std::env::temp_dir().join(format!(
        "forge-outcome-rollback-{}-{}.db",
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
    let input = attempt(&repository, task.id, check.id, "rollback");
    let before = repository.list_record_updates(task.id).unwrap();
    fault.execute_batch("CREATE TRIGGER reject_assessment_update BEFORE UPDATE ON record_attempts WHEN NEW.outcome_json IS NOT NULL BEGIN SELECT RAISE(ABORT, 'injected acceptance failure'); END;").unwrap();
    assert_eq!(
        repository
            .submit_record_outcome(task.id, &input)
            .unwrap_err()
            .code,
        "storage"
    );
    assert_eq!(
        repository
            .show_record_check(task.id, check.id, None, 100)
            .unwrap()
            .assessments
            .len(),
        0
    );
    assert_eq!(repository.list_record_checks(task.id).unwrap(), vec![check]);
    assert_eq!(repository.list_record_updates(task.id).unwrap(), before);
    assert_eq!(repository.record_work_state(task.id).unwrap().output, None);
    fault
        .execute_batch("DROP TRIGGER reject_assessment_update")
        .unwrap();
    let accepted = repository.submit_record_outcome(task.id, &input).unwrap();
    assert_eq!(
        repository.submit_record_outcome(task.id, &input).unwrap(),
        accepted
    );
    drop(fault);
    drop(repository);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_task_opening_path_provisions_criteria_and_failed_opening_rolls_back() {
    let repository = SqliteProjectRepository::in_memory().unwrap();
    repository
        .register(&project("checks", "/repos/checks"))
        .unwrap();
    let spec = repository
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Spec,
            body: "Spec".into(),
            project_slugs: vec!["checks".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let map = repository
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Wayfinder,
            body: "Map".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    for body in [
        "Implement a feature",
        "Research an API",
        "Review work",
        "Build a prototype",
    ] {
        let standalone = repository
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: body.into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        let child = repository
            .open_task_under_spec(body, spec.id, vec![])
            .unwrap();
        let map_child = repository
            .open_task_under_wayfinder(body, map.id, None, vec![])
            .unwrap();
        for task in [standalone, child, map_child] {
            let checks = repository.list_record_checks(task.id).unwrap();
            assert_eq!(checks.len(), 1);
            assert_eq!(checks[0].record_id, task.id);
            assert_eq!(checks[0].position, 0);
            assert_eq!(checks[0].latest_assessment, None);
        }
    }
    assert_eq!(
        repository.list_record_checks(spec.id).unwrap_err().code,
        "invalid-record-type"
    );
    let first = task(&repository);
    let other = task(&repository);
    let check = repository.list_record_checks(first.id).unwrap().remove(0);
    assert_eq!(
        repository
            .show_record_check(other.id, check.id, None, 50)
            .unwrap_err()
            .code,
        "check-not-found"
    );
    repository.start_record(other.id).unwrap();
    assert_eq!(
        repository
            .begin_record_attempt(
                other.id,
                &AttemptInput {
                    id: "foreign-check".into(),
                    check_id: check.id,
                    generation: 1,
                    evidence: evidence(),
                    actor: "operator".into()
                }
            )
            .unwrap_err()
            .code,
        "check-not-found"
    );
    let filter = RecordListFilter {
        project_slug: None,
        status: None,
        record_type: None,
    };
    let before = repository.list_records(&filter).unwrap();
    assert!(
        repository
            .open_task_under_spec("Invalid generator", spec.id, vec![999_999])
            .is_err()
    );
    assert_eq!(repository.list_records(&filter).unwrap(), before);
    let repository = crate::SqliteProjectRepository::in_memory().unwrap();
    for name in ["one", "two"] {
        repository
            .create_workspace(
                name,
                name,
                &project(name, &format!("/checks-scope/{name}")).project_root,
            )
            .unwrap();
    }
    let local = repository.records_in_workspace("one").unwrap();
    let foreign = repository.records_in_workspace("two").unwrap();
    let task = task(local.as_ref());
    assert_eq!(
        foreign.record_work_state(task.id).unwrap_err().code,
        "cross-workspace-record"
    );
    local.start_record(task.id).unwrap();
    let check = local.list_record_checks(task.id).unwrap().remove(0);
    let input = attempt(local.as_ref(), task.id, check.id, "scoped");
    assert_eq!(
        foreign
            .submit_record_outcome(task.id, &input)
            .unwrap_err()
            .code,
        "cross-workspace-record"
    );
    assert_eq!(local.list_record_checks(task.id).unwrap(), vec![check]);
}
