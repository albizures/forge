#[allow(clippy::too_many_lines)]
async fn verify_clean_recovery_rpc(
    daemon: &Daemon, store: &SqliteProjectRepository, task: u64,
    checkout: &std::path::Path, outcome: &serde_json::Value,
) {
    use forge_app::{AgentDispatchRepository, RecordRepository};
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
    for untracked in [false, true] {
        let file = checkout.join(if untracked { "untracked.txt" } else { "deliverable.txt" });
        std::fs::write(&file, "Retained dirty work\n").unwrap();
        let error = RecordApiServer::recover_retained_work(daemon, request.clone()).await.unwrap_err();
        assert!(error.to_string().contains("clean active Managed Worktree"), "{error:?}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "Retained dirty work\n");
        assert_eq!(store.record_work_state(task).unwrap(), before);
        // Fixture restoration only; the product never cleans dirty work to authorize recovery.
        if untracked { std::fs::remove_file(file).unwrap(); }
        else { std::fs::write(file, "Implemented\n").unwrap(); }
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
    let mut conflict = request; conflict.record.recovery.message.push('!');
    assert!(RecordApiServer::recover_retained_work(daemon, conflict).await.is_err());
    assert_eq!(store.record_work_state(task).unwrap(), work);
}
