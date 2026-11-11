use super::SqliteProjectRepository as Repository;
use super::*;
use forge_app::{
    checks::{InvalidationInput, Verdict},
    findings::{FindingRaiseInput, FindingResolveInput, FindingState, Severity},
};

fn task(repository: &dyn RecordRepository) -> Record {
    repository
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver reviewed work".into(),
            project_slugs: vec![],
            task_kind: None,
            generated_by_ids: vec![],
        })
        .unwrap()
}
fn raise(severity: Severity) -> FindingRaiseInput {
    FindingRaiseInput {
        severity,
        blocking: None,
        check_id: None,
        assessment_id: None,
        actor: "reviewer".into(),
        message: "Missing validation".into(),
        revision: Some("a".repeat(40)),
        dirty: Some(false),
    }
}
fn resolve(version: u64) -> FindingResolveInput {
    FindingResolveInput {
        expected_version: version,
        actor: "verifier".into(),
        message: "Verified the correction".into(),
        revision: Some("b".repeat(40)),
        dirty: None,
    }
}
#[test]
fn blocking_findings_combine_with_checks_and_only_verified_resolution_unblocks() {
    let repository = Repository::in_memory().unwrap();
    let task = task(&repository);
    repository.start_record(task.id).unwrap();
    let major = repository
        .raise_task_finding(task.id, &raise(Severity::Major))
        .unwrap();
    let critical = repository
        .raise_task_finding(task.id, &raise(Severity::Critical))
        .unwrap();
    let minor = repository
        .raise_task_finding(task.id, &raise(Severity::Minor))
        .unwrap();
    assert!(major.blocking && critical.blocking && !minor.blocking);
    let check = repository.list_task_checks(task.id).unwrap().remove(0);
    let error = repository.done_record(task.id, None).unwrap_err();
    assert!(
        error
            .message
            .contains(&format!("Check IDs: [{}]", check.id))
    );
    assert!(
        error
            .message
            .contains(&format!("Finding IDs: [{}, {}]", major.id, critical.id))
    );
    check_tests::assess(&repository, task.id, Verdict::Pass);
    assert_eq!(
        repository
            .show_task_finding(task.id, major.id, None, 10)
            .unwrap()
            .finding
            .state,
        FindingState::Open
    );
    assert_eq!(
        repository.done_record(task.id, None).unwrap_err().code,
        "completion-blocked"
    );
    for finding in [major, critical] {
        let resolved = repository
            .resolve_task_finding(task.id, finding.id, &resolve(1))
            .unwrap();
        assert_eq!(resolved.state, FindingState::Resolved);
        assert_eq!(resolved.version, 2);
        assert_eq!(resolved.raised, finding.raised);
        let before = repository.list_record_updates(task.id).unwrap();
        assert_eq!(
            repository
                .resolve_task_finding(task.id, finding.id, &resolve(1))
                .unwrap_err()
                .code,
            "stale-version"
        );
        assert_eq!(
            repository
                .resolve_task_finding(task.id, finding.id, &resolve(2))
                .unwrap_err()
                .code,
            "invalid-transition"
        );
        assert_eq!(repository.list_record_updates(task.id).unwrap(), before);
        let page = repository
            .show_task_finding(task.id, finding.id, None, 1)
            .unwrap();
        assert_eq!(page.activities.len(), 1);
        assert_eq!(page.activities[0], finding.raised);
        let next = repository
            .show_task_finding(task.id, finding.id, page.next_cursor, 1)
            .unwrap();
        assert_eq!(next.activities[0], resolved.latest_activity);
        assert!(next.next_cursor.is_none());
        assert!(before.iter().any(|u| u.kind
            == RecordUpdateKind::FindingActivity {
                activity_id: resolved.latest_activity.id
            }));
    }
    let before = repository.list_record_updates(task.id).unwrap().len();
    repository.done_record(task.id, None).unwrap();
    assert_eq!(
        repository.list_record_updates(task.id).unwrap().len(),
        before + 1
    );
    assert_eq!(
        repository
            .raise_task_finding(task.id, &raise(Severity::Minor))
            .unwrap_err()
            .code,
        "record-done"
    );
    assert_eq!(
        repository
            .resolve_task_finding(task.id, minor.id, &resolve(1))
            .unwrap()
            .state,
        FindingState::Resolved
    );
}
#[test]
fn origins_are_same_task_and_invalidated_assessments_remain_historical_origins() {
    let repository = Repository::in_memory().unwrap();
    let first = task(&repository);
    let second = task(&repository);
    check_tests::assess(&repository, first.id, Verdict::Fail);
    let check = repository.list_task_checks(first.id).unwrap().remove(0);
    let assessment = check.latest_assessment.unwrap();
    repository
        .invalidate_task_check(
            first.id,
            check.id,
            &InvalidationInput {
                expected_version: check.version,
                assessment_id: assessment.id,
                actor: "reviewer".into(),
                message: "Withdrawn".into(),
                revision: None,
                dirty: None,
            },
        )
        .unwrap();
    let input = FindingRaiseInput {
        assessment_id: Some(assessment.id),
        ..raise(Severity::Minor)
    };
    let finding = repository.raise_task_finding(first.id, &input).unwrap();
    assert_eq!(finding.check_id, Some(check.id));
    assert_eq!(finding.assessment_id, Some(assessment.id));
    let other_check = repository.list_task_checks(second.id).unwrap().remove(0).id;
    let before = repository.list_record_updates(first.id).unwrap();
    for input in [
        FindingRaiseInput {
            check_id: Some(other_check),
            ..input.clone()
        },
        FindingRaiseInput {
            check_id: Some(u64::MAX),
            assessment_id: None,
            ..input.clone()
        },
        FindingRaiseInput {
            actor: " ".into(),
            ..input.clone()
        },
        FindingRaiseInput {
            revision: Some("abc".into()),
            ..input.clone()
        },
    ] {
        assert!(repository.raise_task_finding(first.id, &input).is_err());
    }
    assert_eq!(repository.list_record_updates(first.id).unwrap(), before);
    assert_eq!(
        repository
            .raise_task_finding(second.id, &input)
            .unwrap_err()
            .code,
        "invalid-finding-origin"
    );
    assert_eq!(
        repository
            .show_task_finding(second.id, finding.id, None, 10)
            .unwrap_err()
            .code,
        "finding-not-found"
    );
    assert_eq!(
        repository
            .list_task_findings(first.id, None, 0)
            .unwrap_err()
            .code,
        "invalid-page"
    );
    assert_eq!(
        repository
            .list_task_findings(first.id, Some(u64::MAX), 10)
            .unwrap_err()
            .code,
        "invalid-page"
    );
    let explicit = repository
        .raise_task_finding(
            first.id,
            &FindingRaiseInput {
                blocking: Some(false),
                ..raise(Severity::Critical)
            },
        )
        .unwrap();
    assert!(!explicit.blocking);
    let page = repository.list_task_findings(first.id, None, 1).unwrap();
    assert_eq!(page.findings, [finding]);
    let next = repository
        .list_task_findings(first.id, page.next_cursor, 1)
        .unwrap();
    assert_eq!(next.findings, [explicit]);
    assert!(next.next_cursor.is_none());
}
#[test]
fn finding_and_record_update_roll_back_together() {
    let repository = Repository::in_memory().unwrap();
    let task = task(&repository);
    let before = repository.list_record_updates(task.id).unwrap();
    repository.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_finding_log BEFORE INSERT ON record_updates WHEN NEW.kind = 'finding-activity' BEGIN SELECT RAISE(ABORT, 'injected log failure'); END;").unwrap();
    assert_eq!(
        repository
            .raise_task_finding(task.id, &raise(Severity::Major))
            .unwrap_err()
            .code,
        "storage"
    );
    assert!(
        repository
            .list_task_findings(task.id, None, 10)
            .unwrap()
            .findings
            .is_empty()
    );
    assert_eq!(repository.list_record_updates(task.id).unwrap(), before);
    repository
        .connection
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_finding_log")
        .unwrap();
    let finding = repository
        .raise_task_finding(task.id, &raise(Severity::Major))
        .unwrap();
    repository.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_finding_log BEFORE INSERT ON record_updates WHEN NEW.kind = 'finding-activity' BEGIN SELECT RAISE(ABORT, 'injected log failure'); END;").unwrap();
    assert!(
        repository
            .resolve_task_finding(task.id, finding.id, &resolve(1))
            .is_err()
    );
    let page = repository
        .show_task_finding(task.id, finding.id, None, 10)
        .unwrap();
    assert_eq!(page.finding, finding);
    assert_eq!(page.activities.len(), 1);
}
#[test]
fn completion_racing_blocker_creation_never_commits_a_blocked_done_task() {
    use std::sync::{Arc, Barrier};
    for _ in 0..20 {
        let path = std::env::temp_dir().join(format!(
            "forge-finding-race-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repository = Repository::open(&path).unwrap();
        let task = task(&repository);
        repository.start_record(task.id).unwrap();
        check_tests::assess(&repository, task.id, Verdict::Pass);
        let other = Repository::open(&path).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let b = barrier.clone();
        let completion = std::thread::spawn(move || {
            b.wait();
            repository.done_record(task.id, None)
        });
        let mutation = std::thread::spawn(move || {
            barrier.wait();
            other.raise_task_finding(task.id, &raise(Severity::Critical))
        });
        let completion = completion.join().unwrap();
        let mutation = mutation.join().unwrap();
        assert!(!(completion.is_ok() && mutation.is_ok()));
        if let Err(error) = &mutation {
            assert_eq!(error.code, "record-done");
        }
        let repository = Repository::open(&path).unwrap();
        let findings = repository.list_task_findings(task.id, None, 10).unwrap();
        assert_eq!(findings.findings.len(), usize::from(mutation.is_ok()));
        if mutation.is_ok() {
            assert_eq!(
                repository.done_record(task.id, None).unwrap_err().code,
                "completion-blocked"
            );
        }
        drop(repository);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn findings_survive_restart_and_application_compares_evidence_without_git() {
    use forge_app::{Application, RepositoryInspector, checks::RevisionComparison};
    struct NoGit;
    impl RepositoryInspector for NoGit {
        fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
            panic!("Finding evidence must not execute Git")
        }
    }
    let path = std::env::temp_dir().join(format!(
        "forge-finding-durable-{}-{}.sqlite",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let project = project("alpha", "/finding-durable");
    let repository = super::super::SqliteProjectRepository::open(&path).unwrap();
    repository
        .create_workspace("test", "Test", &project.project_root)
        .unwrap();
    let repository = repository.in_record_workspace("test").unwrap();
    repository.register(&project).unwrap();
    let task = repository
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Review evidence".into(),
            project_slugs: vec!["alpha".into()],
            task_kind: None,
            generated_by_ids: vec![],
        })
        .unwrap();
    let finding = repository
        .raise_task_finding(task.id, &raise(Severity::Major))
        .unwrap();
    let resolved = repository
        .resolve_task_finding(task.id, finding.id, &resolve(1))
        .unwrap();
    drop(repository);
    let repository = Repository::open(&path).unwrap();
    assert_eq!(
        repository
            .show_task_finding(task.id, finding.id, None, 10)
            .unwrap()
            .finding,
        resolved
    );
    let app = Application::new(Box::new(repository), Box::new(NoGit));
    let page = app
        .show_task_finding(task.id, finding.id, None, 10, Some(&"a".repeat(40)))
        .unwrap();
    assert_eq!(page.finding.raised.comparison, RevisionComparison::Matching);
    assert_eq!(
        page.finding.latest_activity.comparison,
        RevisionComparison::Mismatching
    );
    assert_eq!(page.activities[0].comparison, RevisionComparison::Matching);
    assert_eq!(
        app.list_task_findings(task.id, None, 10, None)
            .unwrap()
            .findings[0],
        resolved
    );
    assert_eq!(
        app.show_task_finding(task.id, finding.id, None, 10, Some("bad"))
            .unwrap_err()
            .code,
        "invalid-revision"
    );
    drop(app);
    let repository = Repository::open(&path).unwrap();
    let other_root = path.with_extension("other-root");
    std::fs::create_dir(&other_root).unwrap();
    repository
        .create_workspace("other", "Other", &other_root)
        .unwrap();
    let other = repository.in_record_workspace("other").unwrap();
    let before = repository.list_record_updates(task.id).unwrap();
    assert!(other.list_task_findings(task.id, None, 10).is_err());
    assert!(
        other
            .show_task_finding(task.id, finding.id, None, 10)
            .is_err()
    );
    assert!(
        other
            .raise_task_finding(task.id, &raise(Severity::Minor))
            .is_err()
    );
    assert!(
        other
            .resolve_task_finding(task.id, finding.id, &resolve(2))
            .is_err()
    );
    assert_eq!(repository.list_record_updates(task.id).unwrap(), before);
    drop(other);
    drop(repository);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(other_root).unwrap();
}

#[test]
fn concurrent_resolutions_retain_one_activity_and_reject_the_stale_writer() {
    use std::sync::{Arc, Barrier};
    let path = std::env::temp_dir().join(format!(
        "forge-finding-stale-{}-{}.sqlite",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = Repository::open(&path).unwrap();
    let task = task(&repository);
    let finding = repository
        .raise_task_finding(task.id, &raise(Severity::Major))
        .unwrap();
    let before = repository.list_record_updates(task.id).unwrap().len();
    let other = Repository::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let b = barrier.clone();
    let a = std::thread::spawn(move || {
        b.wait();
        repository.resolve_task_finding(task.id, finding.id, &resolve(1))
    });
    let b = std::thread::spawn(move || {
        barrier.wait();
        other.resolve_task_finding(task.id, finding.id, &resolve(1))
    });
    let (a, b) = (a.join().unwrap(), b.join().unwrap());
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(a.err().or_else(|| b.err()).unwrap().code, "stale-version");
    let repository = Repository::open(&path).unwrap();
    let page = repository
        .show_task_finding(task.id, finding.id, None, 100)
        .unwrap();
    assert_eq!(page.activities.len(), 2);
    assert_eq!(page.finding.version, 2);
    assert_eq!(
        repository.list_record_updates(task.id).unwrap().len(),
        before + 1
    );
    drop(repository);
    std::fs::remove_file(path).unwrap();
}
