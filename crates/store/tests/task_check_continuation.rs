use forge_app::{
    AgentDispatchRepository, Application, DispatchWorkspace,
    ExecutionMode::{Dispatched, Manual},
    ManagedWorktreeRepository, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordStatus, RecordType, WorkspaceDispatchRepository, WorkspaceRepository,
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
    checks::Verdict,
    outcomes::{OutcomeDisposition, OutcomeInput},
};
use forge_store::SqliteProjectRepository;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
#[path = "support/task_attempt_adapters.rs"]
mod adapters;
use adapters::{Local, Provider};

#[test]
#[allow(clippy::too_many_lines)]
fn controlled_checks_continue_only_after_retirement_and_survive_restart() {
    let root = std::env::temp_dir().join(format!(
        "forge-continuation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join(".agents/skills/work")).unwrap();
    std::fs::write(
        root.join(".agents/skills/work/SKILL.md"),
        "---\nname: work\ndescription: work\n---\nWork",
    )
    .unwrap();
    std::fs::write(
        root.join(".agents/skills/work/forge-agent.md"),
        "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\ntools: [read]\n---\nWork",
    )
    .unwrap();
    let path = root.join("store.sqlite");
    let store = SqliteProjectRepository::open(&path).unwrap();
    store.create_workspace("test", "Test", &root).unwrap();
    store
        .register(&Project {
            reference: "test/api".into(),
            workspace_id: "test".into(),
            slug: "api".into(),
            project_root: root.clone(),
            origin_url: "https://example.test/api.git".into(),
        })
        .unwrap();
    let policy = ResourceCatalog::discover(&root.join("operator"), &root)
        .unwrap()
        .resolve(&root, ".agents/skills/work/forge-agent.md")
        .unwrap();
    let pinned = store
        .register_agent_profile("test", RecordType::Task, &policy)
        .unwrap();
    store
        .select_workspace_profile("test", RecordType::Task, Some(pinned.revision))
        .unwrap();
    store
        .select_workspace_profile("test", RecordType::Grilling, Some(1))
        .unwrap();
    let generation = store
        .workspace_dispatch_configuration("test")
        .unwrap()
        .provider_generation;
    store
        .retain_verified_workspace_binding(
            "test",
            Some(&DispatchWorkspace {
                id: "provider".into(),
            }),
            generation,
        )
        .unwrap();
    store
        .replace_check_template(
            "test/api",
            &TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Pin".into(),
                criteria: ["Implementation", "Review 1", "Review 2"]
                    .map(|name| CheckCriterion {
                        name: name.into(),
                        description: name.into(),
                        profile_revision: Some(pinned.revision),
                    })
                    .into(),
            },
        )
        .unwrap();
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            project_slugs: vec!["test/api".into()],
            body: "Deliver work".into(),
            generated_by_ids: vec![],
        })
        .unwrap();
    store.reserve_worktree_claim(task.id).unwrap();
    store.activate_worktree_claim(task.id).unwrap();
    let launches = Arc::new(Mutex::new(Vec::new()));
    let unavailable = Arc::new(AtomicBool::new(false));
    let application = || {
        Application::with_managed_worktrees_and_agent_dispatch(
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(Local),
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(Provider {
                unavailable: unavailable.clone(),
                launches: launches.clone(),
            }),
            Box::new(Local),
        )
        .with_workspace_repository(Box::new(SqliteProjectRepository::open(&path).unwrap()))
    };
    let app = application();
    let (mut assignment, _) = app.execute_task_check_acceptance(task.id).unwrap();
    assert_eq!(
        app.start_record(task.id).unwrap_err().code,
        "dispatched-start-forbidden"
    );
    let checks = app.list_record_checks(task.id).unwrap();
    let secrets = root.join("secrets");
    std::fs::create_dir_all(&secrets).unwrap();
    for (index, check) in checks.iter().enumerate() {
        assert_eq!(launches.lock().unwrap().len(), index + 1);
        let attempt = store
            .assignment_check_attempt(&assignment.id)
            .unwrap()
            .unwrap();
        assert_eq!(attempt.check_id, check.id);
        assert_eq!(assignment.profile.revision, pinned.revision);
        std::fs::write(secrets.join(&assignment.id), "secret").unwrap();
        let session = app.assignment(&secrets, &assignment.id, "secret").unwrap();
        session.acknowledge().unwrap();
        session.start().unwrap();
        session.context().unwrap();
        let input = OutcomeInput {
            attempt_id: assignment.id.clone(),
            generation: attempt.generation,
            evidence: attempt.evidence,
            verdict: Verdict::Pass,
            actor: "ignored".into(),
            message: "Passed".into(),
            disposition: OutcomeDisposition::Complete,
            findings: vec![],
            verifications: vec![],
        };
        session.submit_outcome(&input).unwrap();
        assert!(
            store
                .agent_assignment(&assignment.id)
                .unwrap()
                .closed_at_millis
                .is_none()
        );
        assert_eq!(launches.lock().unwrap().len(), index + 1);
        if index == 0 {
            // A provider cleanup returning Ok is insufficient while retained facts remain.
            store
                .add_agent_assignment_resource(&forge_app::NewAgentAssignmentResource {
                    assignment_id: assignment.id.clone(),
                    resource_kind: "provider-pane".into(),
                    resource_id: "old-pane".into(),
                })
                .unwrap();
            unavailable.store(true, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(
                app.execute_task_check_acceptance(task.id).unwrap_err().code,
                "agent-retirement-failed"
            );
            assert_eq!(launches.lock().unwrap().len(), 1);
            assert!(
                app.agent_assignment_info(&assignment.id)
                    .unwrap()
                    .capacity_reserved()
            );
            unavailable.store(false, std::sync::atomic::Ordering::SeqCst);
            application()
                .reconcile_agent_dispatch("blocked", 0)
                .unwrap();
            assert!(
                store
                    .agent_assignment(&assignment.id)
                    .unwrap()
                    .closed_at_millis
                    .is_some()
            );
            assert_eq!(launches.lock().unwrap().len(), 1);
            assert_eq!(
                app.execute_task_check_acceptance(task.id).unwrap_err().code,
                "assignment-recovery-blocked"
            );
            assert!(
                store
                    .list_agent_assignment_resources(&assignment.id)
                    .unwrap()
                    .iter()
                    .any(|r| r.resource_kind == "check-handoff" && r.retired_at_millis.is_none())
            );
            store
                .retire_agent_assignment_resource(&assignment.id, "provider-pane", "old-pane")
                .unwrap();
        }
        if index == 1 {
            store
                .hold_dispatch(task.id, "operator investigation")
                .unwrap();
            application()
                .reconcile_agent_dispatch("held", 760_000)
                .unwrap();
            assert_eq!(launches.lock().unwrap().len(), 2);
            assert_eq!(
                app.execute_task_check_acceptance(task.id).unwrap_err().code,
                "dispatch-held"
            );
            assert!(store.dispatch_hold(task.id).unwrap().is_some());
            store.resume_dispatch(task.id).unwrap();
            let barrier = std::sync::Barrier::new(2);
            let results = std::thread::scope(|scope| {
                let requests: Vec<_> = (0..2)
                    .map(|_| {
                        scope.spawn(|| {
                            let app = application();
                            barrier.wait();
                            app.execute_task_check_acceptance(task.id).unwrap()
                        })
                    })
                    .collect();
                requests
                    .into_iter()
                    .map(|r| r.join().unwrap())
                    .collect::<Vec<_>>()
            });
            assert_eq!(results[0].0.id, results[1].0.id);
            assert_eq!(results.iter().filter(|(_, existing)| !existing).count(), 1);
            assert_eq!(launches.lock().unwrap().len(), 3);
        }
        // A new Application/store connection resumes from durable acceptance, not memory.
        application()
            .reconcile_agent_dispatch("continuation", (index as u64 + 1) * 400_000)
            .unwrap();
        assert!(
            store
                .agent_assignment(&assignment.id)
                .unwrap()
                .closed_at_millis
                .is_some()
        );
        assert!(
            store
                .list_agent_assignment_resources(&assignment.id)
                .unwrap()
                .iter()
                .all(|r| r.retired_at_millis.is_some())
        );
        if index < 2 {
            assignment = store
                .list_agent_assignments(task.id)
                .unwrap()
                .into_iter()
                .find(|a| a.closed_at_millis.is_none())
                .expect("next Check launched");
        }
    }
    assert_eq!(launches.lock().unwrap().len(), 3);
    assert_eq!(
        app.show_record(task.id).unwrap().status,
        RecordStatus::InProgress
    );
    assert_eq!(
        app.execute_task_check_acceptance(task.id).unwrap_err().code,
        "checks-passed"
    );
    assert_eq!(
        app.dispatch_record(task.id).unwrap_err().code,
        "task-dispatch-unsupported-contract"
    );
    // In Progress alone is not durable Forge-managed ownership evidence.
    let unmanaged = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            project_slugs: vec!["test/api".into()],
            body: "Manual work".into(),
            generated_by_ids: vec![],
        })
        .unwrap();
    store.reserve_worktree_claim(unmanaged.id).unwrap();
    store.activate_worktree_claim(unmanaged.id).unwrap();
    store
        .set_record_execution_mode(unmanaged.id, Dispatched, Manual, "operator", "Manual start")
        .unwrap();
    store.start_record(unmanaged.id).unwrap();
    store
        .set_record_execution_mode(
            unmanaged.id,
            Manual,
            Dispatched,
            "operator",
            "Handoff requires its later gate",
        )
        .unwrap();
    assert_eq!(
        app.execute_task_check_acceptance(unmanaged.id)
            .unwrap_err()
            .code,
        "record-not-ready"
    );
    assert_eq!(launches.lock().unwrap().len(), 3);
    std::fs::remove_dir_all(root).unwrap();
}
