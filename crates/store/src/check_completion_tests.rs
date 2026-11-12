use super::*;
use forge_app::checks::{AssessmentInput, InvalidationInput, Verdict};

#[test]
fn completion_racing_assessment_or_invalidation_never_commits_a_nonpassing_done_task() {
    use std::sync::{Arc, Barrier};
    // Independent connections exercise SQLite transactions, not the per-repository mutex.
    for invalidate in [false, true] {
        for _ in 0..20 {
            let path = std::env::temp_dir().join(format!(
                "forge-completion-{}-{}.sqlite",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let repository = SqliteProjectRepository::open(&path).unwrap();
            let task = repository
                .open_typed_record(&NewTypedRecord {
                    record_type: RecordType::Task,
                    body: "Verify concurrent completion".into(),
                    project_slugs: vec![],
                    generated_by_ids: vec![],
                })
                .unwrap();
            repository.start_record(task.id).unwrap();
            check_tests::assess(&repository, task.id, Verdict::Pass);
            let check = repository.list_record_checks(task.id).unwrap().remove(0);
            let before = repository.list_record_updates(task.id).unwrap().len();
            let contender = SqliteProjectRepository::open(&path).unwrap();
            let barrier = Arc::new(Barrier::new(2));
            let completion_barrier = barrier.clone();
            let completion = std::thread::spawn(move || {
                completion_barrier.wait();
                repository.done_record(task.id, None)
            });
            let mutation = std::thread::spawn(move || {
                barrier.wait();
                if invalidate {
                    contender.invalidate_record_check(
                        task.id,
                        check.id,
                        &InvalidationInput {
                            expected_version: check.version,
                            assessment_id: check.latest_assessment.unwrap().id,
                            actor: "reviewer".into(),
                            message: "Withdraw evidence".into(),
                            revision: None,
                            dirty: None,
                        },
                    )
                } else {
                    contender.assess_record_check(
                        task.id,
                        check.id,
                        &AssessmentInput {
                            expected_version: check.version,
                            verdict: Verdict::Fail,
                            actor: "reviewer".into(),
                            message: "Deliverables incomplete".into(),
                            revision: None,
                            dirty: None,
                        },
                    )
                }
            });
            let completion = completion.join().unwrap();
            let mutation = mutation.join().unwrap();
            assert!(!(completion.is_ok() && mutation.is_ok()));
            if let Err(error) = &completion {
                // Deferred completion transactions may lose a SQLite lock upgrade;
                // this must remain an atomic storage failure, never a stale pass.
                assert!(matches!(error.code, "checks-not-passed" | "storage"));
            }
            if let Err(error) = &mutation {
                assert_eq!(error.code, "record-done");
            }
            let repository = SqliteProjectRepository::open(&path).unwrap();
            let checks = repository.list_record_checks(task.id).unwrap();
            let page = repository
                .show_record_check(task.id, checks[0].id, None, 100)
                .unwrap();
            assert_eq!(checks[0].version, 2 + u64::from(mutation.is_ok()));
            assert_eq!(page.activities.len(), 1 + usize::from(mutation.is_ok()));
            assert_eq!(
                repository.list_record_updates(task.id).unwrap().len(),
                before + usize::from(completion.is_ok()) + usize::from(mutation.is_ok())
            );
            if completion.is_ok() {
                assert_eq!(checks[0].current_verdict, Some(Verdict::Pass));
                assert_eq!(
                    repository.done_record(task.id, None).unwrap_err().code,
                    "invalid-status-transition"
                );
            } else {
                assert!(mutation.is_ok());
                assert_ne!(checks[0].current_verdict, Some(Verdict::Pass));
                assert_eq!(
                    repository.done_record(task.id, None).unwrap_err().code,
                    "checks-not-passed"
                );
            }
            drop(repository);
            std::fs::remove_file(path).unwrap();
        }
    }
}
