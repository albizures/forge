use super::*;
use forge_app::retained_work::RecoveryInput;

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
pub fn run(f: &Fixture<'_, impl Fn() -> Application + Sync>, dirty_recovery: bool) {
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
    let app = application();
    let (old, _) = app.execute_task_check_acceptance(task).unwrap();
    let secrets = root.join("secrets");
    std::fs::create_dir_all(&secrets).unwrap();
    std::fs::write(secrets.join(&old.id), "secret").unwrap();
    let session = app.assignment(&secrets, &old.id, "secret").unwrap();
    session.bootstrap().unwrap();
    // Retain a real prior interruption: an Implementation Agent cannot report one itself, so the
    // operator retires it and clears the resulting hold.
    app.retire_agent_assignment(&old.id).unwrap();
    store.resume_dispatch(task).unwrap();
    let (old, _) = application().execute_task_check_acceptance(task).unwrap();
    let attempt = store.assignment_check_attempt(&old.id).unwrap().unwrap();
    std::fs::write(secrets.join(&old.id), "secret").unwrap();
    let session = app.assignment(&secrets, &old.id, "secret").unwrap();
    session.bootstrap().unwrap();
    *head.lock().unwrap() = "b".repeat(40);
    let evidence = Evidence::Repository {
        repository: root.display().to_string(),
        commit: "b".repeat(40),
    };
    let mut input = RecoveryInput {
        source_assignment: old.id.clone(),
        generation: attempt.generation,
        evidence: evidence.clone(),
        actor: "operator".into(),
        message: "Inspected retained partial implementation and external children".into(),
        confirm_old_agent_cannot_act: true,
        confirm_resources_safe: true,
    };
    let before = app.record_work_state(task).unwrap();
    let checks = app.list_record_checks(task).unwrap();
    assert!(
        app.recover_retained_work(task, &input).is_err(),
        "live owner cannot be replaced"
    );
    unavailable.store(true, Ordering::SeqCst);
    assert!(app.retire_agent_assignment(&old.id).is_err());
    assert!(
        application().recover_retained_work(task, &input).is_err(),
        "closed owner with retained resources is unsafe"
    );
    assert_eq!(app.record_work_state(task).unwrap(), before);
    unavailable.store(false, Ordering::SeqCst);
    application().retire_agent_assignment(&old.id).unwrap();
    // Unknown resources remain blockers even after known resources have been reconciled.
    store
        .add_agent_assignment_resource(&forge_app::NewAgentAssignmentResource {
            assignment_id: old.id.clone(),
            resource_kind: "provisioning-intent".into(),
            resource_id: "unknown".into(),
        })
        .unwrap();
    assert!(application().recover_retained_work(task, &input).is_err());
    store
        .retire_agent_assignment_resource(&old.id, "provisioning-intent", "unknown")
        .unwrap();
    // Ordinary resume/reconciliation cannot adopt unexplained B as trusted input.
    store.resume_dispatch(task).unwrap();
    assert_eq!(
        application()
            .execute_task_check_acceptance(task)
            .unwrap_err()
            .code,
        "evidence-drift"
    );
    let drift = app.record_work_state(task).unwrap();
    assert_eq!(drift.input, before.input);
    assert!(drift.recoveries.is_empty());
    store.resume_dispatch(task).unwrap();
    assert_eq!(
        application()
            .execute_task_check_acceptance(task)
            .unwrap_err()
            .code,
        "evidence-drift"
    );
    input.generation = app.record_work_state(task).unwrap().generation;
    let before = app.record_work_state(task).unwrap();
    let hold = store.dispatch_hold(task).unwrap();
    for variant in 0..7 {
        let mut denied = input.clone();
        match variant {
            0 => denied.actor.clear(),
            1 => denied.message = " ".into(),
            2 => denied.confirm_old_agent_cannot_act = false,
            3 => denied.confirm_resources_safe = false,
            4 => denied.generation -= 1,
            5 => denied.source_assignment = "foreign".into(),
            _ => denied.evidence = attempt.evidence.clone(),
        }
        assert!(application().recover_retained_work(task, &denied).is_err());
        assert_eq!(app.record_work_state(task).unwrap(), before);
    }
    dirty.store(dirty_recovery, Ordering::SeqCst);
    // Persistence must revalidate mutable observations even when called without the app adapter.
    let mut claim = store.worktree_claim(task).unwrap().unwrap();
    claim.path = root.join("wrong");
    assert!(
        store
            .authorize_retained_work(task, &input, &claim, None)
            .is_err()
    );
    let c = rusqlite::Connection::open(path).unwrap();
    c.execute_batch("CREATE TRIGGER deny_recovery BEFORE INSERT ON retained_work_recoveries BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(application().recover_retained_work(task, &input).is_err());
    assert_eq!(
        app.record_work_state(task).unwrap(),
        before,
        "audit failure rolls input/freshness back"
    );
    assert_eq!(store.dispatch_hold(task).unwrap(), hold);
    c.execute_batch("DROP TRIGGER deny_recovery").unwrap();
    let recovery = std::thread::scope(|scope| {
        let one = scope.spawn(|| application().recover_retained_work(task, &input).unwrap());
        let two = scope.spawn(|| application().recover_retained_work(task, &input).unwrap());
        let recovery = one.join().unwrap();
        assert_eq!(two.join().unwrap(), recovery);
        recovery
    });
    let work = application().record_work_state(task).unwrap();
    assert_eq!(recovery.retained_state.is_some(), dirty_recovery);
    assert_eq!(work.recoveries, vec![recovery.clone()]);
    assert_eq!(work.input, Some(evidence.clone()));
    assert!(work.output.is_none());
    assert_eq!(work.progress, before.progress);
    let mut recovered_checks = app.list_record_checks(task).unwrap();
    for (current, original) in recovered_checks.iter_mut().zip(&checks) {
        assert!(current.version > original.version);
        current.version = original.version;
    }
    assert_eq!(recovered_checks, checks);
    assert_eq!(store.task_base_commit(task).unwrap(), Some("a".repeat(40)));
    assert_eq!(store.list_agent_assignments(task).unwrap().len(), 2);
    assert_eq!(
        application().recover_retained_work(task, &input).unwrap(),
        recovery
    );
    let mut conflict = input.clone();
    conflict.message.push('!');
    assert_eq!(
        application()
            .recover_retained_work(task, &conflict)
            .unwrap_err()
            .code,
        "recovery-conflict"
    );
    assert!(
        application().execute_task_check_acceptance(task).is_err(),
        "hold is not cleared by authorization"
    );
    app.edit_record(
        task,
        "Updated requirements; inspect and salvage retained work",
    )
    .unwrap();
    let edited = application().record_work_state(task).unwrap();
    assert_eq!(edited.recoveries, work.recoveries);
    assert_eq!(edited.progress, work.progress);
    assert_eq!(edited.input, work.input);
    store.resume_dispatch(task).unwrap();
    unavailable.store(true, Ordering::SeqCst);
    assert!(
        application().execute_task_check_acceptance(task).is_err(),
        "provider readiness still gates replacement"
    );
    assert_eq!(launches.lock().unwrap().len(), 2);
    unavailable.store(false, Ordering::SeqCst);
    let (replacement, existing) = application().execute_task_check_acceptance(task).unwrap();
    assert!(!existing);
    let successor = store
        .assignment_check_attempt(&replacement.id)
        .unwrap()
        .unwrap();
    assert_eq!(successor.check_id, attempt.check_id);
    assert_eq!(successor.evidence, evidence);
    assert_eq!(successor.recovery, Some(recovery.clone()));
    assert_eq!(
        application().recover_retained_work(task, &input).unwrap(),
        recovery,
        "retry after restart/launch is read-only"
    );
    assert_eq!(
        application()
            .execute_task_check_acceptance(task)
            .unwrap()
            .0
            .id,
        replacement.id
    );
    assert_eq!(launches.lock().unwrap().len(), 3);
    if dirty_recovery {
        std::fs::write(secrets.join(&replacement.id), "secret").unwrap();
        let replacement_session = app.assignment(&secrets, &replacement.id, "secret").unwrap();
        let boot = replacement_session.bootstrap().unwrap();
        assert_eq!(
            boot.context.bound.check_attempt.unwrap().recovery,
            Some(recovery.clone())
        );
        let mut output = OutcomeInput {
            attempt_id: successor.id,
            generation: successor.generation,
            evidence: evidence.clone(),
            verdict: Verdict::Pass,
            actor: "ignored".into(),
            message: "Salvaged".into(),
            disposition: OutcomeDisposition::Complete,
            findings: vec![],
            verifications: vec![],
        };
        dirty.store(false, Ordering::SeqCst);
        *head.lock().unwrap() = "d".repeat(40);
        output.evidence = Evidence::Repository {
            repository: root.display().to_string(),
            commit: "d".repeat(40),
        };
        let accepted = review_path::submit(&replacement_session, &output).unwrap();
        assert_eq!(accepted.work.output, Some(output.evidence));
        // Retirement after reservation never makes the old authorization reusable.
        app.retire_agent_assignment(&replacement.id).unwrap();
        store.resume_dispatch(task).unwrap();
        let configuration = store.workspace_dispatch_configuration("test").unwrap();
        let selection = forge_app::DispatchPreflightSelection {
            task_revision: configuration.task_profile.unwrap().revision,
            grilling_revision: configuration.grilling_profile.unwrap().revision,
            binding_id: configuration.binding.unwrap().id,
            provider_generation: configuration.provider_generation,
        };
        assert_eq!(
            store
                .claim_task_check_acceptance(
                    task,
                    "duplicate-salvage",
                    &selection,
                    &evidence,
                    recovery.check_id,
                    replacement.profile.revision,
                    Some(&recovery)
                )
                .unwrap_err()
                .code,
            "recovery-source-stale"
        );
        assert_eq!(store.list_agent_assignments(task).unwrap().len(), 3);
        assert_eq!(store.task_base_commit(task).unwrap(), Some("a".repeat(40)));
    }
    assert!(
        session.context().is_err(),
        "revoked source capability never regains authority"
    );
}
