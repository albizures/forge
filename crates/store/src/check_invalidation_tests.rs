use super::*;
use forge_app::{Application, RepositoryInspector};
use rusqlite::Connection;

mod test_directory {
    pub struct Directory(std::path::PathBuf);
    impl Directory {
        pub fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    pub fn tempdir() -> std::io::Result<Directory> {
        let path = std::env::temp_dir().join(format!(
            "forge-invalidation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path)?;
        Ok(Directory(path))
    }
}

struct FakeInspector;
impl RepositoryInspector for FakeInspector {
    fn inspect(&self, _: &std::path::Path) -> Result<Project, RegistrationError> {
        panic!("evidence views must never inspect Git")
    }
}
use forge_app::checks::{AssessmentInput, InvalidationInput, RevisionComparison, Verdict};

fn task(repository: &dyn RecordRepository, projects: Vec<String>) -> Record {
    repository
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver work".into(),
            project_slugs: projects,
            task_kind: None,
            generated_by_ids: vec![],
        })
        .unwrap()
}
fn assessment(version: u64, verdict: Verdict) -> AssessmentInput {
    AssessmentInput {
        expected_version: version,
        verdict,
        actor: "reviewer".into(),
        message: "Inspected deliverables".into(),
        revision: Some("a".repeat(40)),
        dirty: Some(false),
    }
}
fn invalidation(check: &forge_app::checks::TaskCheck) -> InvalidationInput {
    InvalidationInput {
        expected_version: check.version,
        assessment_id: check.latest_assessment.as_ref().unwrap().id,
        actor: "operator".into(),
        message: "Evidence no longer sufficient".into(),
        revision: Some("b".repeat(40)),
        dirty: None,
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn invalidation_is_pending_without_fallback_and_retains_bounded_durable_activity() {
    let directory = test_directory::tempdir().unwrap();
    let path = directory.path().join("checks.sqlite");
    let repository = SqliteProjectRepository::open(&path).unwrap();
    let task = task(&repository, vec![]);
    repository.start_record(task.id).unwrap();
    let check = repository.list_task_checks(task.id).unwrap().remove(0);
    let passed = repository
        .assess_task_check(task.id, check.id, &assessment(1, Verdict::Pass))
        .unwrap();
    let failed = repository
        .assess_task_check(task.id, check.id, &assessment(2, Verdict::Fail))
        .unwrap();
    let before = repository.list_record_updates(task.id).unwrap();
    let historical = InvalidationInput {
        expected_version: failed.version,
        ..invalidation(&passed)
    };
    assert_eq!(
        repository
            .invalidate_task_check(task.id, check.id, &historical)
            .unwrap_err()
            .code,
        "invalid-transition"
    );
    assert_eq!(
        repository
            .invalidate_task_check(task.id, check.id, &invalidation(&passed))
            .unwrap_err()
            .code,
        "stale-version"
    );
    assert_eq!(repository.list_record_updates(task.id).unwrap(), before);
    let pending = repository
        .invalidate_task_check(task.id, check.id, &invalidation(&failed))
        .unwrap();
    assert_eq!(pending.version, 4);
    assert_eq!(pending.current_verdict, None);
    let latest = pending.latest_assessment.as_ref().unwrap();
    assert_eq!(latest.verdict, Verdict::Fail);
    assert_eq!(
        latest.message,
        failed.latest_assessment.as_ref().unwrap().message
    );
    assert_eq!(latest.invalidation.as_ref().unwrap().dirty, None);
    assert_eq!(
        repository.done_record(task.id, None).unwrap_err().code,
        "checks-not-passed"
    );
    let repeated = InvalidationInput {
        expected_version: pending.version,
        ..invalidation(&failed)
    };
    let updates = repository.list_record_updates(task.id).unwrap();
    assert_eq!(
        repository
            .invalidate_task_check(task.id, check.id, &repeated)
            .unwrap_err()
            .code,
        "invalid-transition"
    );
    assert_eq!(repository.list_record_updates(task.id).unwrap(), updates);
    assert!(
        matches!(updates.first().unwrap().kind, RecordUpdateKind::CheckInvalidated { assessment_id } if assessment_id == latest.id)
    );
    assert_eq!(
        repository
            .show_task_check(task.id, check.id, Some(u64::MAX), 1)
            .unwrap_err()
            .code,
        "invalid-page"
    );
    for limit in [0, 101] {
        assert_eq!(
            repository
                .show_task_check(task.id, check.id, None, limit)
                .unwrap_err()
                .code,
            "invalid-page"
        );
    }
    let mut cursor = None;
    let mut activity_ids = vec![];
    let mut assessment_ids = vec![];
    let mut invalidations = vec![];
    loop {
        let page = repository
            .show_task_check(task.id, check.id, cursor, 1)
            .unwrap();
        assert_eq!(page.activities.len(), 1);
        activity_ids.push(page.activities[0].id);
        assessment_ids.extend(page.assessments.iter().map(|a| a.id));
        invalidations.extend(page.activities.into_iter().filter_map(|a| a.invalidation));
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(activity_ids.len(), 3);
    assert!(activity_ids.windows(2).all(|ids| ids[0] < ids[1]));
    assert_eq!(
        assessment_ids,
        [passed.latest_assessment.unwrap().id, latest.id]
    );
    assert_eq!(invalidations, [latest.invalidation.clone().unwrap()]);
    assert!(
        repository
            .show_task_check(task.id, check.id, Some(*activity_ids.last().unwrap()), 1)
            .unwrap()
            .activities
            .is_empty()
    );
    drop(repository);
    let repository = SqliteProjectRepository::open(&path).unwrap();
    assert_eq!(
        repository.list_task_checks(task.id).unwrap().as_slice(),
        std::slice::from_ref(&pending)
    );
    let passed = repository
        .assess_task_check(
            task.id,
            check.id,
            &assessment(pending.version, Verdict::Pass),
        )
        .unwrap();
    assert_eq!(passed.current_verdict, Some(Verdict::Pass));
    repository.done_record(task.id, None).unwrap();
    assert_eq!(
        repository
            .invalidate_task_check(task.id, check.id, &invalidation(&passed))
            .unwrap_err()
            .code,
        "record-done"
    );
    assert_eq!(
        repository
            .assess_task_check(
                task.id,
                check.id,
                &assessment(passed.version, Verdict::Fail)
            )
            .unwrap_err()
            .code,
        "record-done"
    );
}

#[test]
fn invalidation_validation_workspace_and_update_failure_are_atomic() {
    let directory = test_directory::tempdir().unwrap();
    let path = directory.path().join("checks.sqlite");
    let store = crate::SqliteProjectRepository::open(&path).unwrap();
    std::fs::create_dir(directory.path().join("home")).unwrap();
    std::fs::create_dir(directory.path().join("foreign")).unwrap();
    store
        .create_workspace("test", "Test", &directory.path().join("home"))
        .unwrap();
    let repository = store.in_record_workspace("test").unwrap();
    let task = task(&repository, vec![]);
    let check = repository.list_task_checks(task.id).unwrap().remove(0);
    let passed = repository
        .assess_task_check(task.id, check.id, &assessment(check.version, Verdict::Pass))
        .unwrap();
    let input = invalidation(&passed);
    let before = repository
        .show_task_check(task.id, check.id, None, 100)
        .unwrap();
    let updates = repository.list_record_updates(task.id).unwrap();
    for invalid in [
        InvalidationInput {
            actor: " ".into(),
            ..input.clone()
        },
        InvalidationInput {
            message: "\n".into(),
            ..input.clone()
        },
        InvalidationInput {
            revision: Some("abc".into()),
            ..input.clone()
        },
        InvalidationInput {
            revision: Some("z".repeat(40)),
            ..input.clone()
        },
    ] {
        assert!(
            repository
                .invalidate_task_check(task.id, check.id, &invalid)
                .is_err()
        );
    }
    let other = self::task(&repository, vec![]);
    assert_eq!(
        repository
            .invalidate_task_check(other.id, check.id, &input)
            .unwrap_err()
            .code,
        "check-not-found"
    );
    repository
        .create_workspace("foreign", "Foreign", &directory.path().join("foreign"))
        .unwrap();
    let foreign = repository.records_in_workspace("foreign").unwrap();
    assert!(
        foreign
            .invalidate_task_check(task.id, check.id, &input)
            .is_err()
    );
    let fault = Connection::open(&path).unwrap();
    fault.execute_batch("CREATE TRIGGER reject_invalidation BEFORE INSERT ON record_updates WHEN NEW.kind = 'check-invalidated' BEGIN SELECT RAISE(ABORT, 'injected update failure'); END;").unwrap();
    assert_eq!(
        repository
            .invalidate_task_check(task.id, check.id, &input)
            .unwrap_err()
            .code,
        "storage"
    );
    assert_eq!(
        repository
            .show_task_check(task.id, check.id, None, 100)
            .unwrap(),
        before
    );
    assert_eq!(repository.list_record_updates(task.id).unwrap(), updates);
    fault
        .execute_batch("DROP TRIGGER reject_invalidation")
        .unwrap();
    repository
        .invalidate_task_check(task.id, check.id, &input)
        .unwrap();
}

#[test]
fn invalidation_racing_new_assessment_has_one_observed_version_winner() {
    use std::sync::{Arc, Barrier};
    let directory = test_directory::tempdir().unwrap();
    let path = directory.path().join("checks.sqlite");
    let repository = SqliteProjectRepository::open(&path).unwrap();
    let task = task(&repository, vec![]);
    let check = repository.list_task_checks(task.id).unwrap().remove(0);
    let passed = repository
        .assess_task_check(task.id, check.id, &assessment(1, Verdict::Pass))
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles = (0..2)
        .map(|operation| {
            let repository = SqliteProjectRepository::open(&path).unwrap();
            let barrier = barrier.clone();
            let input = invalidation(&passed);
            std::thread::spawn(move || {
                barrier.wait();
                if operation == 0 {
                    repository.invalidate_task_check(task.id, check.id, &input)
                } else {
                    repository.assess_task_check(task.id, check.id, &assessment(2, Verdict::Fail))
                }
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().find_map(|r| r.as_ref().err()).unwrap().code,
        "stale-version"
    );
    let page = repository
        .show_task_check(task.id, check.id, None, 100)
        .unwrap();
    assert_eq!(page.check.version, 3);
    assert_eq!(page.activities.len(), 2);
}

#[test]
fn application_comparison_is_explicit_contextual_and_never_changes_authority() {
    let repository = SqliteProjectRepository::in_memory().unwrap();
    repository
        .register(&project("alpha", "/not-a-git-repository"))
        .unwrap();
    let project_task = task(&repository, vec!["alpha".into()]);
    let projectless = task(&repository, vec![]);
    for task in [&project_task, &projectless] {
        let check = repository.list_task_checks(task.id).unwrap().remove(0);
        let passed = repository
            .assess_task_check(task.id, check.id, &assessment(1, Verdict::Pass))
            .unwrap();
        repository
            .invalidate_task_check(task.id, check.id, &invalidation(&passed))
            .unwrap();
    }
    let app = Application::new(Box::new(repository), Box::new(FakeInspector));
    for (task, expected) in [
        (&project_task, RevisionComparison::Matching),
        (&projectless, RevisionComparison::Unknown),
    ] {
        let checks = app
            .compare_task_checks(task.id, Some(&"A".repeat(40)))
            .unwrap();
        let check = &checks[0];
        assert_eq!(check.current_verdict, None);
        let latest = check.latest_assessment.as_ref().unwrap();
        assert_eq!(latest.comparison, expected);
        assert_eq!(latest.dirty, Some(false));
        assert_eq!(latest.invalidation.as_ref().unwrap().dirty, None);
        let page = app
            .compare_task_check(task.id, check.id, None, 100, Some(&"b".repeat(40)))
            .unwrap();
        assert_eq!(
            page.assessments[0].comparison,
            if task.id == project_task.id {
                RevisionComparison::Mismatching
            } else {
                RevisionComparison::Unknown
            }
        );
        assert_eq!(
            page.activities[1].invalidation.as_ref().unwrap().comparison,
            expected
        );
        assert_eq!(
            app.list_task_checks(task.id).unwrap()[0]
                .latest_assessment
                .as_ref()
                .unwrap()
                .comparison,
            RevisionComparison::Unknown
        );
    }
    assert_eq!(
        app.compare_task_checks(project_task.id, Some("short"))
            .unwrap_err()
            .code,
        "invalid-revision"
    );
    let check = app.list_task_checks(project_task.id).unwrap().remove(0);
    let mut input = assessment(check.version, Verdict::Pass);
    input.revision = None;
    input.dirty = None;
    app.assess_task_check(project_task.id, check.id, &input)
        .unwrap();
    let check = app
        .compare_task_checks(project_task.id, Some(&"a".repeat(40)))
        .unwrap()
        .remove(0);
    assert_eq!(check.current_verdict, Some(Verdict::Pass));
    assert_eq!(
        check.latest_assessment.unwrap().comparison,
        RevisionComparison::Unknown
    );
}
