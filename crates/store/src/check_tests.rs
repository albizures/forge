use super::*;
use forge_app::checks::{AssessmentInput, Verdict};

fn task(repository: &dyn RecordRepository) -> Record {
    repository
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver the work".into(),
            project_slugs: vec![],
            task_kind: None,
            generated_by_ids: vec![],
        })
        .unwrap()
}

pub(super) fn assess(repository: &dyn RecordRepository, task: u64, verdict: Verdict) {
    let check = repository.list_task_checks(task).unwrap().remove(0);
    repository
        .assess_task_check(
            task,
            check.id,
            &AssessmentInput {
                expected_version: check.version,
                verdict,
                actor: "operator".into(),
                message: "Verified deliverables".into(),
                revision: None,
                dirty: None,
            },
        )
        .unwrap();
}

#[test]
fn implementation_check_gates_task_completion_and_latest_assessment_wins() {
    let repository = super::SqliteProjectRepository::in_memory().unwrap();
    let task = task(&repository);
    let checks = repository.list_task_checks(task.id).unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].name, "Implementation");
    assert_eq!(
        checks[0].description,
        "The work and deliverables described by this Task are complete."
    );
    assert_eq!(checks[0].latest_assessment, None);
    repository.start_record(task.id).unwrap();
    let blocked = repository.done_record(task.id, None).unwrap_err();
    assert_eq!(blocked.code, "checks-not-passed");
    assert!(blocked.message.contains(&checks[0].id.to_string()));
    assess(&repository, task.id, Verdict::Pass);
    assess(&repository, task.id, Verdict::Fail);
    assert_eq!(
        repository.done_record(task.id, None).unwrap_err().code,
        "checks-not-passed"
    );
    assess(&repository, task.id, Verdict::Pass);
    assert_eq!(
        repository.done_record(task.id, None).unwrap().status,
        RecordStatus::Done
    );
    let check = repository.list_task_checks(task.id).unwrap().remove(0);
    assert_eq!(
        repository
            .assess_task_check(
                task.id,
                check.id,
                &AssessmentInput {
                    expected_version: check.version,
                    verdict: Verdict::Fail,
                    actor: "operator".into(),
                    message: "Too late".into(),
                    revision: None,
                    dirty: None,
                }
            )
            .unwrap_err()
            .code,
        "record-done"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn assessments_survive_restart_and_stale_or_invalid_writes_leave_no_history() {
    let path = std::env::temp_dir().join(format!(
        "forge-checks-{}-{}.sqlite",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = super::SqliteProjectRepository::open(&path).unwrap();
    let task = task(&repository);
    let check = repository.list_task_checks(task.id).unwrap().remove(0);
    let input = AssessmentInput {
        expected_version: check.version,
        verdict: Verdict::Pass,
        actor: "reviewer".into(),
        message: "Verified".into(),
        revision: Some("a".repeat(40)),
        dirty: Some(true),
    };
    let passed = repository
        .assess_task_check(task.id, check.id, &input)
        .unwrap();
    drop(repository);
    let repository = super::SqliteProjectRepository::open(&path).unwrap();
    assert_eq!(
        repository.list_task_checks(task.id).unwrap(),
        vec![passed.clone()]
    );
    let updates = repository.list_record_updates(task.id).unwrap();
    assert_eq!(
        repository
            .assess_task_check(task.id, check.id, &input)
            .unwrap_err()
            .code,
        "stale-version"
    );
    for invalid in [
        AssessmentInput {
            expected_version: passed.version,
            actor: " ".into(),
            ..input.clone()
        },
        AssessmentInput {
            expected_version: passed.version,
            message: "\n".into(),
            ..input.clone()
        },
        AssessmentInput {
            expected_version: passed.version,
            revision: Some("short".into()),
            ..input.clone()
        },
    ] {
        assert!(
            repository
                .assess_task_check(task.id, check.id, &invalid)
                .is_err()
        );
    }
    assert_eq!(repository.list_record_updates(task.id).unwrap(), updates);
    let fault = rusqlite::Connection::open(&path).unwrap();
    fault.execute_batch("CREATE TRIGGER reject_assessment_update BEFORE INSERT ON record_updates WHEN NEW.kind = 'check-assessed' BEGIN SELECT RAISE(ABORT, 'injected update failure'); END;").unwrap();
    assert!(
        repository
            .assess_task_check(
                task.id,
                check.id,
                &AssessmentInput {
                    expected_version: passed.version,
                    ..input.clone()
                }
            )
            .is_err()
    );
    assert_eq!(repository.list_record_updates(task.id).unwrap(), updates);
    let unchanged = repository
        .show_task_check(task.id, check.id, None, 100)
        .unwrap();
    assert_eq!(unchanged.check, passed);
    assert_eq!(unchanged.assessments.len(), 1);
    fault
        .execute_batch("DROP TRIGGER reject_assessment_update;")
        .unwrap();
    let failed = repository
        .assess_task_check(
            task.id,
            check.id,
            &AssessmentInput {
                expected_version: passed.version,
                verdict: Verdict::Fail,
                revision: None,
                dirty: None,
                ..input
            },
        )
        .unwrap();
    let page = repository
        .show_task_check(task.id, check.id, None, 1)
        .unwrap();
    assert_eq!(page.check, failed);
    assert_eq!(page.assessments, vec![passed.latest_assessment.unwrap()]);
    let next = repository
        .show_task_check(task.id, check.id, page.next_cursor, 1)
        .unwrap();
    assert_eq!(next.assessments, vec![failed.latest_assessment.unwrap()]);
    assert_eq!(next.next_cursor, None);
    assert!(
        repository
            .show_task_check(task.id, check.id, None, 0)
            .is_err()
    );
    assert!(
        repository
            .show_task_check(task.id, check.id, None, 101)
            .is_err()
    );
    drop(repository);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn concurrent_assessments_require_the_same_observed_version_to_have_one_winner() {
    let path = std::env::temp_dir().join(format!(
        "forge-concurrent-checks-{}-{}.sqlite",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = super::SqliteProjectRepository::open(&path).unwrap();
    let task = task(&repository);
    let check = repository.list_task_checks(task.id).unwrap().remove(0);
    let contender = super::SqliteProjectRepository::open(&path).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = [repository, contender]
        .into_iter()
        .map(|repository| {
            let barrier = barrier.clone();
            let input = AssessmentInput {
                expected_version: check.version,
                verdict: Verdict::Pass,
                actor: "operator".into(),
                message: "Verified".into(),
                revision: None,
                dirty: None,
            };
            std::thread::spawn(move || {
                barrier.wait();
                repository.assess_task_check(task.id, check.id, &input)
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.into_iter().find_map(Result::err).unwrap().code,
        "stale-version"
    );
    let repository = super::SqliteProjectRepository::open(&path).unwrap();
    let page = repository
        .show_task_check(task.id, check.id, None, 100)
        .unwrap();
    assert_eq!(page.assessments.len(), 1);
    assert_eq!(page.check.version, check.version + 1);
    drop(repository);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_task_opening_path_provisions_criteria_and_failed_opening_rolls_back() {
    let repository = super::SqliteProjectRepository::in_memory().unwrap();
    repository
        .register(&super::project("checks", "/repos/checks"))
        .unwrap();
    let spec = repository
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Spec,
            body: "Spec".into(),
            project_slugs: vec!["checks".into()],
            task_kind: None,
            generated_by_ids: vec![],
        })
        .unwrap();
    let wayfinder = repository
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Wayfinder,
            body: "Map".into(),
            project_slugs: vec![],
            task_kind: None,
            generated_by_ids: vec![],
        })
        .unwrap();
    for kind in [
        None,
        Some(TaskKind::Research),
        Some(TaskKind::Review),
        Some(TaskKind::Prototype),
    ] {
        let standalone = repository
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "Standalone".into(),
                project_slugs: vec![],
                task_kind: kind,
                generated_by_ids: vec![],
            })
            .unwrap();
        let child = repository
            .open_task_under_spec("Spec child", spec.id, kind, vec![])
            .unwrap();
        let map_child = repository
            .open_task_under_wayfinder("Map child", wayfinder.id, None, kind, vec![])
            .unwrap();
        for task in [standalone, child, map_child] {
            let checks = repository.list_task_checks(task.id).unwrap();
            assert_eq!(checks.len(), 1);
            assert_eq!(checks[0].task_id, task.id);
            assert_eq!(checks[0].position, 0);
            assert_eq!(checks[0].latest_assessment, None);
        }
    }
    assert_eq!(
        repository.list_task_checks(spec.id).unwrap_err().code,
        "invalid-record-type"
    );
    let first = task(&repository);
    let other = task(&repository);
    let first_check = repository.list_task_checks(first.id).unwrap().remove(0);
    assert_eq!(
        repository
            .show_task_check(other.id, first_check.id, None, 50)
            .unwrap_err()
            .code,
        "check-not-found"
    );
    let filter = RecordListFilter {
        project_slug: None,
        status: None,
        record_type: None,
        task_kind: None,
    };
    let before = repository.list_records(&filter).unwrap();
    assert!(
        repository
            .open_task_under_spec("Invalid generator", spec.id, None, vec![999_999])
            .is_err()
    );
    assert_eq!(repository.list_records(&filter).unwrap(), before);
    let store = crate::SqliteProjectRepository::in_memory().unwrap();
    for name in ["one", "two"] {
        let root = super::project(name, &format!("/checks-scope/{name}")).project_root;
        store.create_workspace(name, name, &root).unwrap();
    }
    let local = store.records_in_workspace("one").unwrap();
    let foreign = store.records_in_workspace("two").unwrap();
    let task = task(local.as_ref());
    let check = local.list_task_checks(task.id).unwrap().remove(0);
    assert_eq!(
        foreign.list_task_checks(task.id).unwrap_err().code,
        "cross-workspace-record"
    );
    assert!(foreign.show_task_check(task.id, check.id, None, 1).is_err());
    assert!(
        foreign
            .assess_task_check(
                task.id,
                check.id,
                &AssessmentInput {
                    expected_version: check.version,
                    verdict: Verdict::Pass,
                    actor: "operator".into(),
                    message: "Forbidden".into(),
                    revision: None,
                    dirty: None
                }
            )
            .is_err()
    );
    assert_eq!(local.list_task_checks(task.id).unwrap(), vec![check]);
}
