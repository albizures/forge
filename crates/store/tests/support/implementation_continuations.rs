use super::*;
use forge_app::outcomes::ContinuationHandoff;

pub struct Fixture<'a, F> {
    pub store: &'a SqliteProjectRepository,
    pub path: &'a std::path::Path,
    pub root: &'a std::path::Path,
    pub application: &'a F,
    pub task: u64,
    pub head: &'a Arc<Mutex<String>>,
    pub dirty: &'a Arc<AtomicBool>,
    pub unavailable: &'a Arc<AtomicBool>,
    pub launches: &'a Arc<Mutex<Vec<u32>>>,
}

#[allow(clippy::too_many_lines)]
pub fn run(f: &Fixture<'_, impl Fn() -> Application + Sync>, case: &str) {
    let Fixture {
        store,
        path,
        root,
        application,
        task,
        head,
        dirty,
        unavailable,
        launches,
    } = *f;
    let secrets = root.join("secrets");
    std::fs::create_dir_all(&secrets).unwrap();
    let initial_checks = store.list_record_checks(task).unwrap();
    let base = store.task_base_commit(task).unwrap();
    let rounds = if case == "continuation-total" {
        20
    } else if case == "continuation-unchanged" {
        2
    } else {
        3
    };
    for index in 0..rounds {
        if index > 0 {
            application()
                .reconcile_agent_dispatch("continuation-successor", u64::from(index) * 400_000)
                .unwrap();
        }
        let app = application(); // A fresh database connection on every attempt.
        let (assignment, _) = app.execute_task_check_acceptance(task).unwrap();
        let attempt = store
            .assignment_check_attempt(&assignment.id)
            .unwrap()
            .unwrap();
        assert_eq!(attempt.check_id, initial_checks[0].id);
        if index > 0 {
            let context = attempt.continuation.as_ref().unwrap();
            assert_eq!(context.evidence, attempt.evidence);
            assert_eq!(
                context.handoff.remaining_work,
                "Implement and run full gates"
            );
        }
        std::fs::write(secrets.join(&assignment.id), "secret").unwrap();
        let session = app.assignment(&secrets, &assignment.id, "secret").unwrap();
        session.acknowledge().unwrap();
        session.start().unwrap();
        let context = session.context().unwrap();
        assert_eq!(context.bound.check_attempt.as_ref(), Some(&attempt));
        let changed = case == "continuation-total" || case == "continuation-sequence" && index == 1;
        if changed {
            *head.lock().unwrap() = format!("{:040x}", index + 1);
        }
        let mut evidence = attempt.evidence.clone();
        if let Evidence::Repository { commit, .. } = &mut evidence {
            commit.clone_from(&head.lock().unwrap());
        }
        let input = OutcomeInput {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            evidence: evidence.clone(),
            verdict: Verdict::Fail,
            actor: "ignored".into(),
            message: "Deliberate checkpoint".into(),
            disposition: OutcomeDisposition::Continuation(ContinuationHandoff {
                stopping_reason: "Execution context budget nearly exhausted".into(),
                completed_work: "Checkpoint implemented".into(),
                verification: "Focused tests pass; full gates unrun: execution constraint".into(),
                remaining_work: "Implement and run full gates".into(),
            }),
            findings: vec![],
            verifications: vec![],
        };
        let before = app.record_work_state(task).unwrap();
        if index == 0 {
            for field in 0..4 {
                let mut invalid = input.clone();
                if let OutcomeDisposition::Continuation(h) = &mut invalid.disposition {
                    match field {
                        0 => h.stopping_reason = " ".into(),
                        1 => h.completed_work.clear(),
                        2 => h.verification.clear(),
                        _ => h.remaining_work.clear(),
                    }
                }
                assert_eq!(
                    session.submit_outcome(&invalid).unwrap_err().code,
                    "invalid-continuation"
                );
            }
            dirty.store(true, Ordering::SeqCst);
            assert_eq!(
                session.submit_outcome(&input).unwrap_err().code,
                "invalid-continuation"
            );
            dirty.store(false, Ordering::SeqCst);
            let mut mismatch = input.clone();
            if let Evidence::Repository { commit, .. } = &mut mismatch.evidence {
                *commit = "f".repeat(40);
            }
            assert_eq!(
                session.submit_outcome(&mismatch).unwrap_err().code,
                "invalid-continuation"
            );
            let mut stale = input.clone();
            stale.generation += 1;
            assert_eq!(
                session.submit_outcome(&stale).unwrap_err().code,
                "stale-generation"
            );
            assert_eq!(app.record_work_state(task).unwrap(), before);
            assert!(store.dispatch_hold(task).unwrap().is_none());
        }
        // Fail the final acceptance write, including on the limit-reaching checkpoint.
        if index == 0 || index == rounds - 1 {
            let c = rusqlite::Connection::open(path).unwrap();
            c.execute_batch("CREATE TRIGGER deny_checkpoint BEFORE UPDATE OF outcome_json ON record_attempts BEGIN SELECT RAISE(ABORT, 'injected acceptance failure'); END;").unwrap();
            assert!(session.submit_outcome(&input).is_err());
            assert_eq!(application().record_work_state(task).unwrap(), before);
            assert!(store.dispatch_hold(task).unwrap().is_none());
            c.execute_batch("DROP TRIGGER deny_checkpoint").unwrap();
        }
        if case == "continuation-unchanged" && index == rounds - 1 {
            store
                .hold_dispatch(task, "Stronger retained safety diagnostic")
                .unwrap();
        }
        let accepted = if index == 0 {
            let barrier = std::sync::Barrier::new(2);
            std::thread::scope(|scope| {
                let submit = || {
                    let app = application();
                    let session = app.assignment(&secrets, &assignment.id, "secret").unwrap();
                    barrier.wait();
                    session.submit_outcome(&input).unwrap()
                };
                let a = scope.spawn(submit);
                let b = scope.spawn(submit);
                let accepted = a.join().unwrap();
                assert_eq!(b.join().unwrap(), accepted);
                accepted
            })
        } else {
            session.submit_outcome(&input).unwrap()
        };
        assert_eq!(session.submit_outcome(&input).unwrap(), accepted);
        let state = application().record_work_state(task).unwrap();
        assert_eq!(state, accepted.work);
        assert_eq!(state.input, Some(evidence));
        assert!(state.output.is_none());
        assert_eq!(state.generation, before.generation);
        assert_eq!(state.progress.remediation_cycles, 0);
        assert!(state.progress.interrupted_checks.is_empty());
        let count = &state.progress.continuations[0];
        assert_eq!(count.total, index + 1);
        let unchanged = if changed {
            0
        } else if case == "continuation-sequence" {
            1
        } else {
            index + 1
        };
        assert_eq!(count.consecutive_unchanged, unchanged);
        assert_eq!(store.list_record_checks(task).unwrap(), initial_checks);
        assert_eq!(store.task_base_commit(task).unwrap(), base);
        assert_eq!(
            store.show_record(task).unwrap().status,
            RecordStatus::InProgress
        );
        assert_eq!(
            store.list_agent_assignments(task).unwrap().len(),
            (index + 1) as usize
        );
        // Lost cleanup response: closed ownership and retained capacity survive restart.
        if index == 0 {
            unavailable.store(true, Ordering::SeqCst);
            assert_eq!(
                application()
                    .execute_task_check_acceptance(task)
                    .unwrap_err()
                    .code,
                "agent-retirement-failed"
            );
            assert!(
                app.agent_assignment_info(&assignment.id)
                    .unwrap()
                    .capacity_reserved()
            );
            assert_eq!(launches.lock().unwrap().len(), 1);
            let replay = application();
            let closed_session = replay
                .assignment(&secrets, &assignment.id, "secret")
                .unwrap();
            assert_eq!(closed_session.submit_outcome(&input).unwrap(), accepted);
            let mut conflict = input.clone();
            conflict.message = "Different".into();
            assert_eq!(
                closed_session.submit_outcome(&conflict).unwrap_err().code,
                "outcome-conflict"
            );
            unavailable.store(false, Ordering::SeqCst);
            store
                .add_agent_assignment_resource(&forge_app::NewAgentAssignmentResource {
                    assignment_id: assignment.id.clone(),
                    resource_kind: "retained-test-resource".into(),
                    resource_id: "unknown".into(),
                })
                .unwrap();
            assert_eq!(
                application()
                    .execute_task_check_acceptance(task)
                    .unwrap_err()
                    .code,
                "assignment-recovery-blocked"
            );
            assert!(
                app.agent_assignment_info(&assignment.id)
                    .unwrap()
                    .capacity_reserved()
            );
            store
                .retire_agent_assignment_resource(
                    &assignment.id,
                    "retained-test-resource",
                    "unknown",
                )
                .unwrap();
            store
                .hold_dispatch(task, "Stronger existing safety diagnostic")
                .unwrap();
            assert_eq!(
                application()
                    .execute_task_check_acceptance(task)
                    .unwrap_err()
                    .code,
                "dispatch-held"
            );
            store.resume_dispatch(task).unwrap();
            unavailable.store(true, Ordering::SeqCst);
            assert_eq!(
                application()
                    .execute_task_check_acceptance(task)
                    .unwrap_err()
                    .code,
                "invalid-check-profile"
            );
            unavailable.store(false, Ordering::SeqCst);
            let prerequisite = store
                .open_typed_record(&NewTypedRecord {
                    record_type: RecordType::Wayfinder,
                    body: "New prerequisite".into(),
                    project_slugs: vec!["test/api".into()],
                    generated_by_ids: vec![],
                })
                .unwrap();
            // Isolated readiness fault injection: supported relationship edits are
            // already forbidden after start. Exercise the claim's defensive gate too.
            let c = rusqlite::Connection::open(path).unwrap();
            c.execute("INSERT INTO record_relationships(from_id, to_id, kind) VALUES (?1, ?2, 'depends-on')", rusqlite::params![task.cast_signed(), prerequisite.id.cast_signed()]).unwrap();
            assert_eq!(
                application()
                    .execute_task_check_acceptance(task)
                    .unwrap_err()
                    .code,
                "record-not-ready"
            );
            c.execute(
                "DELETE FROM record_relationships WHERE from_id = ?1 AND to_id = ?2",
                rusqlite::params![task.cast_signed(), prerequisite.id.cast_signed()],
            )
            .unwrap();
            assert_eq!(launches.lock().unwrap().len(), 1);
        }
        if count.exhausted() {
            if case == "continuation-unchanged" {
                assert_eq!(
                    store.dispatch_hold(task).unwrap().unwrap().reason,
                    "Stronger retained safety diagnostic"
                );
                store.resume_dispatch(task).unwrap();
            }
            assert!(
                store
                    .dispatch_hold(task)
                    .unwrap()
                    .unwrap()
                    .reason
                    .contains("Continuation limit")
            );
            assert_eq!(
                application()
                    .execute_task_check_acceptance(task)
                    .unwrap_err()
                    .code,
                "dispatch-held"
            );
            store.resume_dispatch(task).unwrap();
            application()
                .reset_task_remediation(task, "operator", "Only remediation reset")
                .unwrap();
            store.resume_dispatch(task).unwrap();
            assert_eq!(
                application()
                    .execute_task_check_acceptance(task)
                    .unwrap_err()
                    .code,
                "dispatch-held"
            );
            let app = application();
            app.edit_record(
                task,
                "Changed task body does not renew continuation allowance",
            )
            .unwrap();
            assert_eq!(
                app.record_work_state(task).unwrap().progress.continuations,
                state.progress.continuations
            );
            assert_eq!(
                app.execute_task_check_acceptance(task).unwrap_err().code,
                "dispatch-held"
            );
            assert_eq!(launches.lock().unwrap().len(), rounds as usize);
            let snapshot = root.join("continuation-snapshot.db");
            let inventory = forge_store::upgrade::snapshot(path, &snapshot).unwrap();
            assert_eq!(forge_store::upgrade::migrate(&snapshot).unwrap(), inventory);
            let copy = SqliteProjectRepository::open(&snapshot).unwrap();
            assert_eq!(
                copy.record_work_state(task).unwrap().progress.continuations,
                state.progress.continuations
            );
            let reopened = application();
            assert_eq!(
                reopened
                    .assignment(&secrets, &assignment.id, "secret")
                    .unwrap()
                    .submit_outcome(&input)
                    .unwrap(),
                accepted
            );
            verify_reset(f, initial_checks[0].id);
        }
    }
    if case == "continuation-sequence" {
        mixed_accounting(f, &secrets);
    }
}

fn verify_reset(f: &Fixture<'_, impl Fn() -> Application + Sync>, check: u64) {
    let app = (f.application)();
    let before = app.record_work_state(f.task).unwrap();
    let hold = f.store.dispatch_hold(f.task).unwrap();
    let assignments = f.store.list_agent_assignments(f.task).unwrap();
    for (actor, message) in [(" ", "reason"), ("operator", " ")] {
        assert!(
            app.reset_check_continuations(f.task, check, actor, message)
                .is_err()
        );
    }
    assert!(
        app.reset_check_continuations(f.task, check + 10000, "operator", "reason")
            .is_err()
    );
    let unrelated = f
        .store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Unrelated Task".into(),
            project_slugs: vec!["test/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let other_check = f.store.list_record_checks(unrelated.id).unwrap()[0].id;
    assert!(
        app.reset_check_continuations(f.task, other_check, "operator", "reason")
            .is_err()
    );
    let other_root = f.root.with_extension("other-workspace");
    std::fs::create_dir(&other_root).unwrap();
    f.store
        .create_workspace("other", "Other", &other_root)
        .unwrap();
    let other = f.store.in_record_workspace("other").unwrap();
    assert!(
        other
            .reset_record_continuations(f.task, check, "operator", "reason")
            .is_err()
    );
    std::fs::remove_dir(&other_root).unwrap();
    assert_eq!(app.record_work_state(f.task).unwrap(), before);
    let c = rusqlite::Connection::open(f.path).unwrap();
    c.execute_batch("CREATE TRIGGER deny_reset BEFORE INSERT ON check_continuation_resets BEGIN SELECT RAISE(ABORT, 'injected reset audit failure'); END;").unwrap();
    assert!(
        app.reset_check_continuations(f.task, check, "operator", "reason")
            .is_err()
    );
    assert_eq!((f.application)().record_work_state(f.task).unwrap(), before);
    c.execute_batch("DROP TRIGGER deny_reset").unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let reset = || {
            let app = (f.application)();
            barrier.wait();
            app.reset_check_continuations(
                f.task,
                check,
                "operator",
                "Investigated checkpoint and renewed allowance",
            )
            .unwrap();
        };
        let a = scope.spawn(reset);
        let b = scope.spawn(reset);
        a.join().unwrap();
        b.join().unwrap();
    });
    let mut expected = before;
    let counters = &mut expected.progress.continuations[0];
    counters.total = 0;
    counters.consecutive_unchanged = 0;
    let after = (f.application)().record_work_state(f.task).unwrap();
    assert_eq!(after.progress.continuations[0].resets.len(), 2);
    counters
        .resets
        .clone_from(&after.progress.continuations[0].resets);
    assert_eq!(after, expected);
    assert_eq!(f.store.dispatch_hold(f.task).unwrap(), hold);
    assert_eq!(f.store.list_agent_assignments(f.task).unwrap(), assignments);
    assert_eq!(
        app.execute_task_check_acceptance(f.task).unwrap_err().code,
        "dispatch-held"
    );
    // Body invalidation and reset confer no authority; after the independent hold
    // and retirement gates, the ordinary path can select the same Check again.
    f.store.resume_dispatch(f.task).unwrap();
    let (successor, _) = app.execute_task_check_acceptance(f.task).unwrap();
    assert_eq!(
        f.store
            .assignment_check_attempt(&successor.id)
            .unwrap()
            .unwrap()
            .check_id,
        check
    );
}

fn mixed_accounting(f: &Fixture<'_, impl Fn() -> Application>, secrets: &std::path::Path) {
    let counters = (f.application)()
        .record_work_state(f.task)
        .unwrap()
        .progress
        .continuations;
    for stage in 0..4 {
        let app = (f.application)();
        let (assignment, _) = app.execute_task_check_acceptance(f.task).unwrap();
        let attempt = f
            .store
            .assignment_check_attempt(&assignment.id)
            .unwrap()
            .unwrap();
        std::fs::write(secrets.join(&assignment.id), "secret").unwrap();
        let session = app.assignment(secrets, &assignment.id, "secret").unwrap();
        session.acknowledge().unwrap();
        session.start().unwrap();
        session.context().unwrap();
        let mut evidence = attempt.evidence;
        if stage == 1 {
            *f.head.lock().unwrap() = "c".repeat(40);
            if let Evidence::Repository { commit, .. } = &mut evidence {
                commit.clone_from(&f.head.lock().unwrap());
            }
        }
        let input = OutcomeInput {
            attempt_id: attempt.id,
            generation: attempt.generation,
            evidence,
            verdict: if stage == 2 {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            actor: String::new(),
            message: "Independent interruption/remediation/review policy".into(),
            disposition: match stage {
                0 => OutcomeDisposition::Interrupted,
                3 => OutcomeDisposition::Handback,
                _ => OutcomeDisposition::Complete,
            },
            findings: vec![],
            verifications: vec![],
        };
        session.submit_outcome(&input).unwrap();
        let state = (f.application)().record_work_state(f.task).unwrap();
        assert_eq!(state.progress.continuations, counters);
        assert_eq!(state.progress.remediation_cycles, u32::from(stage > 0));
        if stage == 0 {
            assert_eq!(state.progress.interrupted_checks[0].interruptions, 1);
        }
        if stage == 3 {
            assert!(state.progress.stop_reason.unwrap().contains("No progress"));
            assert!(state.generation > attempt.generation);
        }
    }
}
