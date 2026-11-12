use super::*;
use forge_app::findings::{
    Finding, FindingAction, FindingMutation as Change, FindingMutationInput, FindingResponse,
};

fn mutate(
    repo: &dyn RecordRepository,
    finding: &Finding,
    change: Change,
) -> Result<Finding, RegistrationError> {
    repo.mutate_record_finding(
        finding.record_id,
        finding.id,
        &FindingMutationInput {
            change,
            explanation: resolve(finding.version),
        },
    )
}
fn path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "forge-cycle-{label}-{}-{}.sqlite",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
#[test]
fn successive_cycles_retain_responses_dispositions_amendments_and_notes_after_restart() {
    let path = path("restart");
    let repo = Repository::open(&path).unwrap();
    let task = task(&repo);
    repo.start_record(task.id).unwrap();
    check_tests::assess(&repo, task.id, Verdict::Pass);
    let raised = repo
        .raise_record_finding(task.id, &raise(Severity::Critical))
        .unwrap();
    let mut f = raised.clone();
    for response in [FindingResponse::Fixed, FindingResponse::Rejected] {
        f = mutate(&repo, &f, Change::Respond { response }).unwrap();
        assert_eq!(f.state, FindingState::AwaitingReview);
        assert_eq!(f.latest_activity.response, Some(response));
        assert!(f.blocking);
        assert_eq!(
            repo.done_record(task.id, None).unwrap_err().code,
            "completion-blocked"
        );
    }
    f = mutate(
        &repo,
        &f,
        Change::Amend {
            severity: Some(Severity::Minor),
            blocking: None,
        },
    )
    .unwrap();
    assert!(f.blocking); // Severity never re-defaults blocking.
    assert_eq!(f.state, FindingState::AwaitingReview);
    let amendment = f.latest_activity.amendment.clone().unwrap();
    assert_eq!(amendment.old_severity, Severity::Critical);
    assert_eq!(amendment.new_severity, Severity::Minor);
    assert!(amendment.old_blocking && amendment.new_blocking);
    f = mutate(&repo, &f, Change::Resolve).unwrap();
    f = mutate(&repo, &f, Change::Note).unwrap();
    assert_eq!(f.state, FindingState::Resolved);
    let before = repo.list_record_updates(task.id).unwrap();
    assert_eq!(
        mutate(
            &repo,
            &f,
            Change::Respond {
                response: FindingResponse::Fixed
            }
        )
        .unwrap_err()
        .code,
        "invalid-transition"
    );
    assert_eq!(repo.list_record_updates(task.id).unwrap(), before);
    f = mutate(&repo, &f, Change::Reopen).unwrap();
    assert_eq!(f.state, FindingState::Open);
    f = mutate(&repo, &f, Change::Dismiss).unwrap(); // Direct open disposition.
    assert_eq!(f.state, FindingState::Dismissed);
    f = mutate(&repo, &f, Change::Note).unwrap();
    assert_eq!(f.state, FindingState::Dismissed);
    repo.done_record(task.id, None).unwrap(); // A note cannot undo dismissal for completion.
    drop(repo);
    let repo = Repository::open(&path).unwrap();
    let page = repo.show_record_finding(task.id, f.id, None, 100).unwrap();
    assert_eq!(page.finding, f);
    assert_eq!(page.finding.raised, raised.raised);
    assert_eq!(page.activities.len(), 9);
    assert_eq!(page.activities[1].response, Some(FindingResponse::Fixed));
    assert_eq!(page.activities[2].response, Some(FindingResponse::Rejected));
    assert_eq!(page.activities[3].amendment.as_ref(), Some(&amendment));
    assert!(
        page.activities
            .iter()
            .all(|a| a.actor == "reviewer" || a.actor == "verifier")
    );
    assert_eq!(page.activities[8].revision, Some("b".repeat(40)));
    let mut cursor = None;
    let mut ids = vec![];
    loop {
        let page = repo.show_record_finding(task.id, f.id, cursor, 2).unwrap();
        ids.extend(page.activities.iter().map(|a| a.id));
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        ids,
        page.activities.iter().map(|a| a.id).collect::<Vec<_>>()
    );
    drop(repo);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn done_permits_nonblocking_review_but_freezes_amendments_and_blocking_reopening() {
    let repo = Repository::in_memory().unwrap();
    let task = task(&repo);
    repo.start_record(task.id).unwrap();
    check_tests::assess(&repo, task.id, Verdict::Pass);
    let mut minor = repo
        .raise_record_finding(task.id, &raise(Severity::Minor))
        .unwrap();
    let mut major = repo
        .raise_record_finding(task.id, &raise(Severity::Major))
        .unwrap();
    major = mutate(&repo, &major, Change::Dismiss).unwrap();
    minor = mutate(
        &repo,
        &minor,
        Change::Respond {
            response: FindingResponse::Rejected,
        },
    )
    .unwrap();
    repo.done_record(task.id, None).unwrap();
    for change in [
        Change::Amend {
            severity: Some(Severity::Major),
            blocking: None,
        },
        Change::Amend {
            severity: None,
            blocking: Some(false),
        },
    ] {
        let before = repo.list_record_updates(task.id).unwrap();
        assert_eq!(
            mutate(&repo, &minor, change).unwrap_err().code,
            "record-done"
        );
        assert_eq!(repo.list_record_updates(task.id).unwrap(), before);
    }
    assert_eq!(
        mutate(&repo, &major, Change::Reopen).unwrap_err().code,
        "record-done"
    );
    major = mutate(&repo, &major, Change::Note).unwrap();
    assert_eq!(major.state, FindingState::Dismissed);
    for change in [
        Change::Respond {
            response: FindingResponse::Fixed,
        },
        Change::Resolve,
        Change::Note,
        Change::Reopen,
        Change::Respond {
            response: FindingResponse::Rejected,
        },
        Change::Dismiss,
        Change::Reopen,
    ] {
        minor = mutate(&repo, &minor, change).unwrap();
        assert!(!minor.blocking);
    }
    assert_eq!(minor.state, FindingState::Open);
    assert_eq!(
        repo.show_record(task.id).unwrap().status,
        RecordStatus::Done
    );
    assert_eq!(
        repo.raise_record_finding(task.id, &raise(Severity::Minor))
            .unwrap_err()
            .code,
        "record-done"
    );
}
#[test]
fn invalid_and_stale_review_mutations_leave_no_entities_activity_or_updates() {
    let repo = Repository::in_memory().unwrap();
    let task = task(&repo);
    let f = repo
        .raise_record_finding(task.id, &raise(Severity::Major))
        .unwrap();
    let before = repo.list_record_updates(task.id).unwrap();
    let other = super::task(&repo);
    assert_eq!(
        repo.mutate_record_finding(
            other.id,
            f.id,
            &FindingMutationInput {
                change: Change::Note,
                explanation: resolve(1)
            }
        )
        .unwrap_err()
        .code,
        "finding-not-found"
    );
    assert_eq!(
        mutate(
            &repo,
            &f,
            Change::Amend {
                severity: None,
                blocking: None
            }
        )
        .unwrap_err()
        .code,
        "invalid-finding"
    );
    assert_eq!(
        mutate(&repo, &f, Change::Reopen).unwrap_err().code,
        "invalid-transition"
    );
    for explanation in [
        FindingResolveInput {
            actor: " ".into(),
            ..resolve(1)
        },
        FindingResolveInput {
            message: "\n".into(),
            ..resolve(1)
        },
        FindingResolveInput {
            revision: Some("bad".into()),
            ..resolve(1)
        },
    ] {
        assert!(
            repo.mutate_record_finding(
                task.id,
                f.id,
                &FindingMutationInput {
                    change: Change::Note,
                    explanation
                }
            )
            .is_err()
        );
    }
    assert_eq!(repo.list_record_updates(task.id).unwrap(), before);
    let updated = mutate(&repo, &f, Change::Note).unwrap();
    let before = repo.list_record_updates(task.id).unwrap();
    for change in [
        Change::Respond {
            response: FindingResponse::Fixed,
        },
        Change::Resolve,
        Change::Dismiss,
        Change::Reopen,
        Change::Amend {
            severity: None,
            blocking: Some(false),
        },
        Change::Note,
    ] {
        assert_eq!(mutate(&repo, &f, change).unwrap_err().code, "stale-version");
    }
    assert_eq!(repo.list_record_updates(task.id).unwrap(), before);
    assert_eq!(
        repo.show_record_finding(task.id, f.id, None, 100)
            .unwrap()
            .finding,
        updated
    );
}
#[test]
fn independent_connections_serialize_every_review_action_with_observed_versions() {
    use std::sync::{Arc, Barrier};
    for change in [
        Change::Respond {
            response: FindingResponse::Fixed,
        },
        Change::Resolve,
        Change::Dismiss,
        Change::Reopen,
        Change::Amend {
            severity: Some(Severity::Minor),
            blocking: Some(false),
        },
        Change::Note,
    ] {
        let path = path("concurrent");
        let repo = Repository::open(&path).unwrap();
        let task = task(&repo);
        let mut f = repo
            .raise_record_finding(task.id, &raise(Severity::Major))
            .unwrap();
        if change == Change::Reopen {
            f = mutate(&repo, &f, Change::Resolve).unwrap();
        }
        let before = repo.list_record_updates(task.id).unwrap().len();
        let other = Repository::open(&path).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let b = barrier.clone();
        let first = f.clone();
        let a = std::thread::spawn(move || {
            b.wait();
            mutate(&repo, &first, change)
        });
        let b = std::thread::spawn(move || {
            barrier.wait();
            mutate(&other, &f, Change::Note)
        });
        let (a, b) = (a.join().unwrap(), b.join().unwrap());
        assert_ne!(a.is_ok(), b.is_ok());
        assert_eq!(
            a.as_ref().err().or_else(|| b.as_ref().err()).unwrap().code,
            "stale-version"
        );
        let winner = a.ok().or_else(|| b.ok()).unwrap();
        let repo = Repository::open(&path).unwrap();
        let page = repo
            .show_record_finding(task.id, winner.id, None, 100)
            .unwrap();
        assert_eq!(page.finding, winner);
        assert_eq!(page.activities.len() as u64, winner.version);
        assert_eq!(repo.list_record_updates(task.id).unwrap().len(), before + 1);
        drop(repo);
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn completion_racing_amend_or_reopen_never_leaves_a_done_task_with_a_blocker() {
    use std::sync::{Arc, Barrier};
    for reopen in [false, true] {
        for _ in 0..10 {
            let path = path("done-race");
            let repo = Repository::open(&path).unwrap();
            let task = task(&repo);
            repo.start_record(task.id).unwrap();
            check_tests::assess(&repo, task.id, Verdict::Pass);
            let mut f = repo
                .raise_record_finding(
                    task.id,
                    &raise(if reopen {
                        Severity::Major
                    } else {
                        Severity::Minor
                    }),
                )
                .unwrap();
            if reopen {
                f = mutate(&repo, &f, Change::Resolve).unwrap();
            }
            let other = Repository::open(&path).unwrap();
            let barrier = Arc::new(Barrier::new(2));
            let b = barrier.clone();
            let a = std::thread::spawn(move || {
                b.wait();
                repo.done_record(task.id, None)
            });
            let b = std::thread::spawn(move || {
                barrier.wait();
                mutate(
                    &other,
                    &f,
                    if reopen {
                        Change::Reopen
                    } else {
                        Change::Amend {
                            severity: None,
                            blocking: Some(true),
                        }
                    },
                )
            });
            let (done, changed) = (a.join().unwrap(), b.join().unwrap());
            assert_ne!(done.is_ok(), changed.is_ok());
            match done {
                Ok(_) => assert_eq!(changed.unwrap_err().code, "record-done"),
                Err(error) => assert_eq!(error.code, "completion-blocked"),
            }
            let repo = Repository::open(&path).unwrap();
            let f = &repo.list_record_findings(task.id, None, 10).unwrap().findings[0];
            if repo.show_record(task.id).unwrap().status == RecordStatus::Done {
                assert!(!f.blocking || f.state == FindingState::Resolved);
            }
            drop(repo);
            std::fs::remove_file(path).unwrap();
        }
    }
}
#[test]
fn failed_update_log_rolls_back_amendment_and_activity() {
    let repo = Repository::in_memory().unwrap();
    let task = task(&repo);
    let f = repo
        .raise_record_finding(task.id, &raise(Severity::Major))
        .unwrap();
    let before = repo.list_record_updates(task.id).unwrap();
    repo.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_review_update BEFORE INSERT ON record_updates BEGIN SELECT RAISE(ABORT, 'injected update failure'); END;").unwrap();
    assert_eq!(
        mutate(
            &repo,
            &f,
            Change::Amend {
                severity: Some(Severity::Minor),
                blocking: Some(false)
            }
        )
        .unwrap_err()
        .code,
        "storage"
    );
    assert_eq!(
        repo.show_record_finding(task.id, f.id, None, 100)
            .unwrap()
            .finding,
        f
    );
    assert_eq!(repo.list_record_updates(task.id).unwrap(), before);
    assert_eq!(f.latest_activity.action, FindingAction::Raise);
}
