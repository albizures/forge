#[allow(clippy::too_many_lines)]
async fn verify_clean_recovery_rpc(
    daemon: &Daemon, store: &SqliteProjectRepository, task: u64,
    checkout: &std::path::Path, outcome: &serde_json::Value, dirty: bool, database: &std::path::Path,
) {
    use forge_app::{AgentDispatchRepository, RecordRepository, WorkspaceDispatchRepository};
    use forge_rpc::RecordApiServer;
    let selection = forge_rpc::RecordSelection { workspace: Some("test".into()), ..Default::default() };
    let input = forge_app::retained_work::RecoveryInput {
        source_assignment: "bound-check".into(), generation: 1,
        evidence: serde_json::from_value(outcome["outcome"]["evidence"].clone()).unwrap(),
        actor: "operator".into(), message: "Inspected retained work and stopped external children".into(),
        confirm_old_agent_cannot_act: true, confirm_resources_safe: true,
    };
    let request = forge_rpc::SelectedRecordRequest { selection: selection.clone(), record: forge_rpc::RecoverRetainedWorkRequest { record: task, recovery: input.clone() } };
    let before = store.record_work_state(task).unwrap();
    let unbound = assignment_rpc(daemon.clone(), "assignment.recoverRetainedWork", serde_json::json!({"assignmentId":"bound-check", "secret":"check-secret", "recovery":input})).await;
    assert!(unbound.get("error").is_some());
    assert!(RecordApiServer::recover_retained_work(daemon, request.clone()).await.is_err());
    // Isolated retirement fixture: persistent resource facts represent verified provider absence.
    store.add_agent_assignment_resource(&forge_app::NewAgentAssignmentResource { assignment_id: "bound-check".into(), resource_kind: "retirement-pending".into(), resource_id: "operator".into() }).unwrap();
    store.close_agent_assignment("bound-check", forge_app::AgentAssignmentClosureReason::Retired).unwrap();
    assert!(RecordApiServer::recover_retained_work(daemon, request.clone()).await.is_err());
    for resource in store.list_agent_assignment_resources("bound-check").unwrap() {
        store.retire_agent_assignment_resource("bound-check", &resource.resource_kind, &resource.resource_id).unwrap();
    }
    let mut wrong_scope = request.clone();
    wrong_scope.selection.workspace = Some("missing".into());
    assert!(RecordApiServer::recover_retained_work(daemon, wrong_scope).await.is_err());
    let mut wrong_commit = request.clone();
    wrong_commit.record.recovery.evidence = before.input.clone().unwrap();
    assert!(RecordApiServer::recover_retained_work(daemon, wrong_commit).await.is_err());
    if dirty {
        super::git(checkout, &["config", "status.showUntrackedFiles", "no"]).unwrap();
        std::fs::write(checkout.join("deliverable.txt"), "Staged retained work\n").unwrap();
        super::git(checkout, &["add", "deliverable.txt"]).unwrap();
        std::fs::write(checkout.join("deliverable.txt"), "Unstaged retained work\n").unwrap();
        std::fs::write(checkout.join("untracked.txt"), b"Untracked retained work\0\xff").unwrap();
    }
    let mut wrong_repository = request.clone();
    if let forge_app::outcomes::Evidence::Repository { repository, .. } = &mut wrong_repository.record.recovery.evidence { repository.push_str("-other"); }
    assert!(RecordApiServer::recover_retained_work(daemon, wrong_repository).await.is_err());
    // Substitute only isolated fixture Git metadata to exercise actual Repository identity.
    let other = checkout.parent().unwrap().join("other-repository");
    std::fs::create_dir_all(&other).unwrap();
    super::git(&other, &["init"]).unwrap();
    let git_file = checkout.join(".git");
    let metadata = std::fs::read(&git_file).unwrap();
    std::fs::write(&git_file, format!("gitdir: {}\n", other.join(".git").display())).unwrap();
    assert!(RecordApiServer::recover_retained_work(daemon, request.clone()).await.is_err());
    std::fs::write(git_file, metadata).unwrap();
    #[cfg(unix)]
    {
        let saved = checkout.with_extension("retained-fixture");
        std::fs::rename(checkout, &saved).unwrap();
        std::os::unix::fs::symlink(&saved, checkout).unwrap();
        assert!(RecordApiServer::recover_retained_work(daemon, request.clone()).await.is_err());
        std::fs::remove_file(checkout).unwrap();
        std::fs::rename(saved, checkout).unwrap();
        if dirty {
            let link = checkout.join("retained-link");
            std::os::unix::fs::symlink("/etc/passwd", &link).unwrap();
            let error = RecordApiServer::recover_retained_work(daemon, request.clone()).await.unwrap_err();
            assert!(error.to_string().contains("Cannot safely observe retained state"));
            assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
            assert_eq!(store.record_work_state(task).unwrap(), before);
            std::fs::remove_file(link).unwrap();
        }
    }
    let expected_branch = format!("feat/forge-{task}");
    super::git(checkout, &["branch", "-m", "wrong-branch"]).unwrap();
    assert!(RecordApiServer::recover_retained_work(daemon, request.clone()).await.is_err());
    super::git(checkout, &["branch", "-m", &expected_branch]).unwrap();
    assert_eq!(store.record_work_state(task).unwrap(), before);
    let (one, two) = tokio::join!(
        RecordApiServer::recover_retained_work(daemon, request.clone()),
        RecordApiServer::recover_retained_work(daemon, request.clone()),
    );
    let recovery = one.unwrap();
    assert_eq!(two.unwrap(), recovery);
    let work = store.record_work_state(task).unwrap();
    assert_eq!(recovery.retained_state.is_some(), dirty);
    assert_eq!(work.recoveries, vec![recovery.clone()]);
    assert_eq!(work.input, Some(input.evidence));
    assert!(work.output.is_none());
    assert_eq!(work.progress, before.progress);
    assert!(store.list_record_checks(task).unwrap()[0].current_verdict.is_none());
    assert!(store.dispatch_hold(task).unwrap().is_some());
    assert_eq!(store.list_agent_assignments(task).unwrap().len(), 1);
    let inspected = RecordApiServer::record_work_state(daemon, forge_rpc::SelectedRecordRequest { selection, record: forge_rpc::RecordWorkStateRequest { record: task } }).await.unwrap();
    assert_eq!(inspected, work);
    assert_eq!(RecordApiServer::recover_retained_work(daemon, request.clone()).await.unwrap(), recovery);
    let mut conflict = request.clone(); conflict.record.recovery.message.push('!');
    assert!(RecordApiServer::recover_retained_work(daemon, conflict).await.is_err());
    assert_eq!(store.record_work_state(task).unwrap(), work);
    if !dirty {
        // Clean salvage must revalidate branch identity after authorization too.
        super::git(checkout, &["branch", "-m", "changed-after-authorization"]).unwrap();
        store.resume_dispatch(task).unwrap();
        let error = daemon.application.execute_task_check_acceptance(task).unwrap_err();
        assert_eq!(error.code, "evidence-drift");
        assert_eq!(store.list_agent_assignments(task).unwrap().len(), 1);
        assert_eq!(store.record_work_state(task).unwrap().input, work.input);
        super::git(checkout, &["branch", "-m", &expected_branch]).unwrap();
        let commit = outcome["outcome"]["evidence"]["commit"].as_str().unwrap();
        super::git(&other, &["fetch", checkout.to_str().unwrap(), commit]).unwrap();
        super::git(&other, &["checkout", "-b", &expected_branch, commit]).unwrap();
        let metadata = std::fs::read(checkout.join(".git")).unwrap();
        std::fs::write(checkout.join(".git"), format!("gitdir: {}\n", other.join(".git").display())).unwrap();
        store.resume_dispatch(task).unwrap();
        assert_eq!(daemon.application.execute_task_check_acceptance(task).unwrap_err().code, "evidence-drift");
        assert_eq!(store.list_agent_assignments(task).unwrap().len(), 1);
        std::fs::write(checkout.join(".git"), metadata).unwrap();
    }
    if dirty {
        assert_eq!(std::fs::read(checkout.join("untracked.txt")).unwrap(), b"Untracked retained work\0\xff");
        assert_eq!(super::git(checkout, &["show", ":deliverable.txt"]).unwrap(), "Staged retained work");
        assert_eq!(std::fs::read_to_string(checkout.join("deliverable.txt")).unwrap(), "Unstaged retained work\n");
        // Reopen at authorization boundary; exact retries cannot rebind altered bytes.
        let reopened = SqliteProjectRepository::open(database).unwrap();
        assert_eq!(reopened.record_work_state(task).unwrap(), work);
        assert_eq!(super::retained_state::observe(checkout).unwrap(), recovery.retained_state.clone().unwrap());
        for (file, bytes) in [("untracked.txt", b"changed".as_slice()), ("deliverable.txt", b"changed".as_slice())] {
            let original = std::fs::read(checkout.join(file)).unwrap();
            std::fs::write(checkout.join(file), bytes).unwrap();
            store.resume_dispatch(task).unwrap();
            let error = daemon.application.execute_task_check_acceptance(task).unwrap_err();
            assert_eq!(error.code, "evidence-drift");
            assert!(store.dispatch_hold(task).unwrap().unwrap().reason.contains("Authorized retained bytes or index changed"));
            assert_eq!(store.list_agent_assignments(task).unwrap().len(), 1);
            assert_eq!(RecordApiServer::recover_retained_work(daemon, request.clone()).await.unwrap(), recovery);
            std::fs::write(checkout.join(file), original).unwrap();
        }
        // Staged-only drift is detected even if working bytes and HEAD remain identical.
        super::git(checkout, &["add", "deliverable.txt"]).unwrap();
        store.resume_dispatch(task).unwrap();
        assert_eq!(daemon.application.execute_task_check_acceptance(task).unwrap_err().code, "evidence-drift");
        // Restore only the deliberately mutated fixture, then reserve the authorized successor.
        std::fs::write(checkout.join("deliverable.txt"), "Staged retained work\n").unwrap();
        super::git(checkout, &["add", "deliverable.txt"]).unwrap();
        std::fs::write(checkout.join("deliverable.txt"), "Unstaged retained work\n").unwrap();
        assert_eq!(super::retained_state::observe(checkout).unwrap(), recovery.retained_state.clone().unwrap());
        store.resume_dispatch(task).unwrap();
        let configuration = store.workspace_dispatch_configuration("test").unwrap();
        let revision = store.agent_assignment("bound-check").unwrap().profile.revision;
        let selection = forge_app::DispatchPreflightSelection {
            task_revision: configuration.task_profile.unwrap().revision,
            grilling_revision: configuration.grilling_profile.unwrap().revision,
            binding_id: configuration.binding.unwrap().id,
            provider_generation: configuration.provider_generation,
        };
        let mut stale = recovery.clone();
        stale.retained_state.as_mut().unwrap().sha256 = "0".repeat(64);
        for expected in [None, Some(&stale)] {
            assert_eq!(store.claim_task_check_acceptance(task, "denied-salvage", &selection, &request.record.recovery.evidence, recovery.check_id, revision, expected).unwrap_err().code, "recovery-source-stale");
            assert_eq!(store.list_agent_assignments(task).unwrap().len(), 1);
        }
        store.claim_task_check_acceptance(task, "salvage-successor", &selection, &request.record.recovery.evidence, recovery.check_id, revision, Some(&recovery)).unwrap();
        let secret_root = daemon.capability_secret_root.as_ref().unwrap();
        std::fs::write(secret_root.join("salvage-successor"), "salvage-secret").unwrap();
        let credentials = serde_json::json!({"assignmentId":"salvage-successor", "secret":"salvage-secret"});
        let boot = assignment_rpc(daemon.clone(), "assignment.bootstrap", credentials.clone()).await;
        assert_eq!(boot["result"]["retainedWorkRecovery"], serde_json::to_value(&recovery).unwrap(), "{boot}");
        assert_eq!(std::fs::read(checkout.join("untracked.txt")).unwrap(), b"Untracked retained work\0\xff");
        assert_eq!(super::git(checkout, &["show", ":deliverable.txt"]).unwrap(), "Staged retained work");
        assert_eq!(std::fs::read_to_string(checkout.join("deliverable.txt")).unwrap(), "Unstaged retained work\n");
        let attempt = store.assignment_check_attempt("salvage-successor").unwrap().unwrap();
        let mut output = outcome["outcome"].clone();
        output["attempt_id"] = "salvage-successor".into();
        output["generation"] = attempt.generation.into();
        let mut submission = credentials;
        submission["outcome"] = output;
        super::git(checkout, &["add", "deliverable.txt", "untracked.txt"]).unwrap();
        super::git(checkout, &["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "Salvaged output"]).unwrap();
        let commit = super::git(checkout, &["rev-parse", "HEAD"]).unwrap();
        submission["outcome"]["evidence"]["commit"] = commit.clone().into();
        let accepted = assignment_rpc(daemon.clone(), "assignment.outcomeSubmit", submission).await;
        assert_eq!(accepted["result"]["evidence"]["commit"], commit, "{accepted}");
        assert_ne!(accepted["result"]["evidence"], outcome["outcome"]["evidence"]);
    }
}
