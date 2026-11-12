use forge_app::checks::Verdict;
use forge_app::outcomes::{AttemptInput, Evidence, OutcomeInput, WorkInvalidationInput};
use forge_app::{NewTypedRecord, RecordRepository, RecordType, WorkspaceRepository};
use forge_store::SqliteProjectRepository;

struct Database(std::path::PathBuf);
impl Database {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forge-outcomes-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("manual")).unwrap();
        std::fs::create_dir_all(root.join("foreign")).unwrap();
        let db = Self(root);
        let store = SqliteProjectRepository::open(&db.path()).unwrap();
        for name in ["manual", "foreign"] {
            store
                .create_workspace(name, name, &db.0.join(name))
                .unwrap();
        }
        db
    }
    fn path(&self) -> std::path::PathBuf {
        self.0.join("forge.db")
    }
    fn open(&self) -> SqliteProjectRepository {
        SqliteProjectRepository::open(&self.path())
            .unwrap()
            .in_record_workspace("manual")
            .unwrap()
    }
    fn attempt(&self) -> (u64, OutcomeInput) {
        let store = self.open();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "Write report".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        store.start_record(task.id).unwrap();
        let check = store.list_record_checks(task.id).unwrap().remove(0);
        store
            .begin_record_attempt(
                task.id,
                &AttemptInput {
                    id: "manual-1".into(),
                    check_id: check.id,
                    generation: 1,
                    evidence: evidence('a'),
                    actor: "operator".into(),
                },
            )
            .unwrap();
        (
            task.id,
            OutcomeInput {
                attempt_id: "manual-1".into(),
                generation: 1,
                evidence: evidence('b'),
                verdict: Verdict::Pass,
                actor: "operator".into(),
                message: "Report complete".into(),
                disposition: forge_app::outcomes::OutcomeDisposition::Complete,
                findings: vec![],
                verifications: vec![],
            },
        )
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn evidence(value: char) -> Evidence {
    Evidence::Artifact {
        uri: "urn:deliverable:report".into(),
        sha256: value.to_string().repeat(64),
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn manual_artifact_outcome_is_bound_and_replays_after_restart() {
    let db = Database::new();
    let (task, input) = db.attempt();
    let store = db.open();
    let check = store.list_record_checks(task).unwrap().remove(0);
    let before = store.list_record_updates(task).unwrap();
    assert_eq!(
        store
            .begin_record_attempt(
                task,
                &AttemptInput {
                    id: "manual-1".into(),
                    check_id: check.id,
                    generation: 1,
                    evidence: evidence('b'),
                    actor: "operator".into()
                }
            )
            .unwrap_err()
            .code,
        "attempt-conflict"
    );
    for evidence in [
        Evidence::Artifact {
            uri: " ".into(),
            sha256: "a".repeat(64),
        },
        Evidence::Artifact {
            uri: "urn:report".into(),
            sha256: "mutable".into(),
        },
        Evidence::Repository {
            repository: "project".into(),
            commit: "main".into(),
        },
    ] {
        assert_eq!(
            store
                .submit_record_outcome(
                    task,
                    &OutcomeInput {
                        evidence,
                        ..input.clone()
                    }
                )
                .unwrap_err()
                .code,
            "invalid-evidence"
        );
    }
    assert_eq!(store.list_record_updates(task).unwrap(), before);
    assert_eq!(store.list_record_checks(task).unwrap(), vec![check]);
    let accepted = store.submit_record_outcome(task, &input).unwrap();
    assert_eq!(accepted.check.current_verdict, Some(Verdict::Pass));
    assert_eq!(accepted.attempt.evidence, evidence('a'));
    assert_eq!(accepted.evidence, evidence('b'));
    assert_eq!(
        accepted
            .check
            .latest_assessment
            .as_ref()
            .unwrap()
            .generation,
        1
    );
    assert_eq!(
        store.record_work_state(task).unwrap().output,
        Some(evidence('b'))
    );
    assert_eq!(store.submit_record_outcome(task, &input).unwrap(), accepted);
    store.done_record(task, None).unwrap();
    drop(store);
    assert_eq!(
        db.open().submit_record_outcome(task, &input).unwrap(),
        accepted
    );
    let conflict = OutcomeInput {
        message: "different".into(),
        ..input.clone()
    };
    assert_eq!(
        db.open()
            .submit_record_outcome(task, &conflict)
            .unwrap_err()
            .code,
        "outcome-conflict"
    );
    let foreign = SqliteProjectRepository::open(&db.path())
        .unwrap()
        .in_record_workspace("foreign")
        .unwrap();
    assert_eq!(
        foreign
            .submit_record_outcome(task, &input)
            .unwrap_err()
            .code,
        "cross-workspace-record"
    );
    assert_eq!(
        db.open().list_record_checks(task).unwrap(),
        vec![accepted.check]
    );
}

#[test]
fn concurrent_exact_replay_has_one_immutable_assessment_and_conflicts_have_one_winner() {
    use std::sync::{Arc, Barrier};
    for conflict in [false, true] {
        let db = Database::new();
        let (task, input) = db.attempt();
        let barrier = Arc::new(Barrier::new(2));
        let threads = [db.open(), db.open()]
            .into_iter()
            .enumerate()
            .map(|(index, store)| {
                let barrier = barrier.clone();
                let mut input = input.clone();
                if conflict && index == 1 {
                    input.verdict = Verdict::Fail;
                    input.message = "Incomplete report".into();
                }
                std::thread::spawn(move || {
                    barrier.wait();
                    store.submit_record_outcome(task, &input)
                })
            })
            .collect::<Vec<_>>();
        let results = threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            if conflict { 1 } else { 2 }
        );
        if conflict {
            assert_eq!(
                results.iter().find_map(|r| r.as_ref().err()).unwrap().code,
                "outcome-conflict"
            );
        } else {
            assert_eq!(results[0].as_ref().unwrap(), results[1].as_ref().unwrap());
        }
        let store = db.open();
        let check = store.list_record_checks(task).unwrap().remove(0);
        let page = store.show_record_check(task, check.id, None, 100).unwrap();
        assert_eq!(page.assessments.len(), 1);
        assert_eq!(page.activities.len(), 1);
        assert_eq!(check.version, 2);
    }
}

#[test]
fn racing_invalidation_and_outcome_never_revives_a_pass_and_restart_retains_freshness() {
    use std::sync::{Arc, Barrier};
    for _ in 0..10 {
        let db = Database::new();
        let (task, input) = db.attempt();
        let store = db.open();
        let contender = db.open();
        let barrier = Arc::new(Barrier::new(2));
        let other_barrier = barrier.clone();
        let outcome = input.clone();
        let submit = std::thread::spawn(move || {
            other_barrier.wait();
            store.submit_record_outcome(task, &outcome)
        });
        let invalidate = std::thread::spawn(move || {
            barrier.wait();
            contender.invalidate_record_work(
                task,
                &WorkInvalidationInput {
                    generation: 1,
                    evidence: evidence('a'),
                    actor: "operator".into(),
                    message: "Same artifact, new verification".into(),
                },
            )
        });
        let accepted = submit.join().unwrap();
        invalidate.join().unwrap().unwrap();
        let store = db.open();
        let state = store.record_work_state(task).unwrap();
        assert_eq!(state.generation, 2);
        assert_eq!(state.output, None);
        assert_eq!(
            state.latest_invalidation.unwrap().message,
            "Same artifact, new verification"
        );
        let check = store.list_record_checks(task).unwrap().remove(0);
        assert_eq!(check.current_verdict, None);
        if let Ok(accepted) = accepted {
            assert_eq!(store.submit_record_outcome(task, &input).unwrap(), accepted);
        } else {
            assert_eq!(
                store.submit_record_outcome(task, &input).unwrap_err().code,
                "stale-generation"
            );
        }
        assert_eq!(
            store.done_record(task, None).unwrap_err().code,
            "checks-not-passed"
        );
    }
}
