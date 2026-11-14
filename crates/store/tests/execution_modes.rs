use forge_app::{
    AgentAssignmentClosureReason, AgentAssignmentState, AgentDispatchRepository, ExecutionMode,
    NewAgentAssignment, NewAgentAssignmentResource, NewTypedRecord, Project, ProjectRepository,
    RecordRepository, RecordStatus, RecordType, RecordUpdateKind, WorkspaceDispatchRepository,
    WorkspaceRepository,
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
    checks::Verdict,
    outcomes::{AttemptInput, Evidence, OutcomeDisposition, OutcomeInput},
};
use forge_store::SqliteProjectRepository;

struct Fixture {
    root: std::path::PathBuf,
    store: SqliteProjectRepository,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forge-modes-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("workspace")).unwrap();
        let store = SqliteProjectRepository::open(&root.join("store.sqlite")).unwrap();
        store
            .create_workspace("test", "Test", &root.join("workspace"))
            .unwrap();
        store
            .register(&Project {
                reference: "test/api".into(),
                workspace_id: "test".into(),
                slug: "api".into(),
                project_root: root.join("workspace"),
                origin_url: "https://example.test/api.git".into(),
            })
            .unwrap();
        std::fs::create_dir_all(root.join("other")).unwrap();
        store
            .create_workspace("other", "Other", &root.join("other"))
            .unwrap();
        Self { root, store }
    }
    fn open(&self, kind: RecordType, scoped: bool) -> forge_app::Record {
        self.store
            .in_record_workspace("test")
            .unwrap()
            .open_typed_record(&NewTypedRecord {
                record_type: kind,
                body: "Deliver work".into(),
                project_slugs: if scoped {
                    vec!["test/api".into()]
                } else {
                    vec![]
                },
                generated_by_ids: vec![],
            })
            .unwrap()
    }
    fn profiles(&self) {
        let workspace = self.root.join("workspace");
        std::fs::create_dir_all(workspace.join(".agents/skills/profile")).unwrap();
        std::fs::write(
            workspace.join(".agents/skills/profile/SKILL.md"),
            "---\nname: profile\ndescription: profile\n---\nVerify work.\n",
        )
        .unwrap();
        std::fs::create_dir_all(workspace.join(".agents/profiles")).unwrap();
        std::fs::write(workspace.join(".agents/profiles/verify.md"), "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: [read]\n---\nVerify deliverables.\n").unwrap();
        let policy = ResourceCatalog::discover(&self.root.join("operator"), &workspace)
            .unwrap()
            .resolve(&workspace, ".agents/profiles/verify.md")
            .unwrap();
        let profile = self
            .store
            .register_agent_profile("test", RecordType::Task, &policy)
            .unwrap();
        self.store
            .replace_check_template(
                "test/api",
                &TemplateReplacement {
                    expected_version: 0,
                    actor: "operator".into(),
                    message: "Pin Implementation".into(),
                    criteria: vec![CheckCriterion {
                        name: "Implementation".into(),
                        description: "Verify work".into(),
                        profile_revision: Some(profile.revision),
                    }],
                },
            )
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn change(
    store: &dyn RecordRepository,
    id: u64,
    expected: ExecutionMode,
    mode: ExecutionMode,
) -> Result<forge_app::Record, forge_app::RegistrationError> {
    store.set_record_execution_mode(id, expected, mode, "operator", "Explicit policy handoff")
}

#[test]
fn creation_defaults_and_operator_start_are_independent_of_lifecycle_and_profiles() {
    use ExecutionMode::{Dispatched, Manual};
    let f = Fixture::new();
    let s = f.store.in_record_workspace("test").unwrap();
    let generic = s
        .open_record(&forge_app::NewRecord {
            body: "Generic".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    assert_eq!(generic.execution_mode, Manual);
    s.start_record(generic.id).unwrap();
    s.done_record(generic.id, None).unwrap();
    assert_eq!(
        change(&s, generic.id, Manual, Dispatched).unwrap_err().code,
        "invalid-transition"
    );
    for kind in [
        RecordType::Task,
        RecordType::Wayfinder,
        RecordType::Initiative,
    ] {
        let record = f.open(kind, false);
        assert_eq!(record.execution_mode, Manual);
        assert_eq!(s.start_record(record.id).unwrap().execution_mode, Manual);
    }
    for kind in [
        RecordType::Spec,
        RecordType::Wayfinder,
        RecordType::Initiative,
    ] {
        assert_eq!(f.open(kind, true).execution_mode, Manual);
    }
    let task = f.open(RecordType::Task, true);
    assert_eq!(task.execution_mode, Dispatched);
    let before = s.list_record_updates(task.id).unwrap();
    assert_eq!(
        s.start_record(task.id).unwrap_err().code,
        "dispatched-start-forbidden"
    );
    assert_eq!(s.list_record_updates(task.id).unwrap(), before);
    assert_eq!(s.show_record(task.id).unwrap(), task);
    change(&s, task.id, Dispatched, Manual).unwrap();
    assert_eq!(s.start_record(task.id).unwrap().execution_mode, Manual);
    assert_eq!(
        change(&s, task.id, Manual, Dispatched).unwrap_err().code,
        "invalid-check-profile"
    );
    assert_eq!(
        s.show_record(task.id).unwrap().status,
        RecordStatus::InProgress
    );
    assert_eq!(s.show_record(task.id).unwrap().execution_mode, Manual);
    let spec = f.open(RecordType::Spec, true);
    assert_eq!(
        s.open_task_under_spec("Child", spec.id, vec![])
            .unwrap()
            .execution_mode,
        Dispatched
    );
    for scoped in [false, true] {
        let wayfinder = f.open(RecordType::Wayfinder, scoped);
        assert_eq!(
            s.open_task_under_wayfinder("Child", wayfinder.id, None, vec![])
                .unwrap()
                .execution_mode,
            if scoped { Dispatched } else { Manual }
        );
        assert_eq!(
            s.open_grilling_under_wayfinder("Decide", wayfinder.id, None, vec![])
                .unwrap()
                .execution_mode,
            Manual
        );
    }
    assert_eq!(
        change(
            &f.store.in_record_workspace("other").unwrap(),
            task.id,
            Manual,
            Dispatched
        )
        .unwrap_err()
        .code,
        "cross-workspace-record"
    );
    let non_task = f.open(RecordType::Wayfinder, false);
    assert_eq!(
        change(&s, non_task.id, Manual, Dispatched)
            .unwrap()
            .execution_mode,
        Dispatched
    );
}

#[test]
fn active_uncertain_and_retained_assignment_state_prevents_policy_handoff_until_reconciled() {
    use ExecutionMode::{Dispatched, Manual};
    let f = Fixture::new();
    let s = &f.store;
    let task = f.open(RecordType::Task, true);
    s.create_agent_assignment(&NewAgentAssignment {
        id: "old".into(),
        record_id: task.id,
        project_slug: "test/api".into(),
        profile_revision: 1,
    })
    .unwrap();
    s.hold_dispatch(task.id, "Retain safety diagnostic")
        .unwrap();
    s.schedule_agent_assignment_retry("old", AgentAssignmentState::Provisioning, 1234, 3)
        .unwrap();
    for state in [
        AgentAssignmentState::Provisioning,
        AgentAssignmentState::Working,
        AgentAssignmentState::Idle,
        AgentAssignmentState::Blocked,
        AgentAssignmentState::Unknown,
    ] {
        let current = s.agent_assignment("old").unwrap();
        if state != current.state {
            s.compare_and_set_agent_assignment_state("old", current.state, state)
                .unwrap();
        }
        let before = s.list_record_updates(task.id).unwrap();
        assert_eq!(
            change(s, task.id, Dispatched, Manual).unwrap_err().code,
            "execution-mode-unsafe-ownership"
        );
        assert_eq!(s.list_record_updates(task.id).unwrap(), before);
    }
    s.close_agent_assignment("old", AgentAssignmentClosureReason::Lost)
        .unwrap();
    assert_eq!(
        change(s, task.id, Dispatched, Manual).unwrap_err().code,
        "execution-mode-unsafe-ownership"
    );
    s.add_agent_assignment_resource(&NewAgentAssignmentResource {
        assignment_id: "old".into(),
        resource_kind: "agent".into(),
        resource_id: "retained".into(),
    })
    .unwrap();
    s.resolve_unknown_agent_resources("old").unwrap();
    assert_eq!(
        change(s, task.id, Dispatched, Manual).unwrap_err().code,
        "execution-mode-unsafe-ownership"
    );
    s.retire_agent_assignment_resource("old", "agent", "retained")
        .unwrap();
    let assignment = s.agent_assignment("old").unwrap();
    let resources = s.list_agent_assignment_resources("old").unwrap();
    let hold = s.dispatch_hold(task.id).unwrap();
    change(s, task.id, Dispatched, Manual).unwrap();
    assert_eq!(s.agent_assignment("old").unwrap(), assignment);
    assert_eq!(s.list_agent_assignment_resources("old").unwrap(), resources);
    assert_eq!(s.dispatch_hold(task.id).unwrap(), hold);
    assert_eq!(s.start_record(task.id).unwrap().execution_mode, Manual);
    let reopened = SqliteProjectRepository::open(&f.root.join("store.sqlite")).unwrap();
    assert_eq!(
        reopened.show_record(task.id).unwrap().execution_mode,
        Manual
    );
    assert_eq!(reopened.agent_assignment("old").unwrap(), assignment);
    assert_eq!(reopened.dispatch_hold(task.id).unwrap(), hold);
}

#[test]
#[allow(clippy::too_many_lines)]
fn manual_completion_requires_confirmed_retirement_and_resource_reconciliation() {
    let f = Fixture::new();
    let s = &f.store;
    let task = f.open(RecordType::Task, true);
    change(s, task.id, ExecutionMode::Dispatched, ExecutionMode::Manual).unwrap();
    s.start_record(task.id).unwrap();
    let check = s.list_record_checks(task.id).unwrap().remove(0);
    let evidence = Evidence::Artifact {
        uri: "urn:complete:report".into(),
        sha256: "a".repeat(64),
    };
    s.begin_record_attempt(
        task.id,
        &AttemptInput {
            id: "complete".into(),
            check_id: check.id,
            generation: 1,
            evidence: evidence.clone(),
            actor: "operator".into(),
        },
    )
    .unwrap();
    s.submit_record_outcome(
        task.id,
        &OutcomeInput {
            attempt_id: "complete".into(),
            generation: 1,
            evidence,
            verdict: Verdict::Pass,
            actor: "operator".into(),
            message: "Verified report".into(),
            disposition: OutcomeDisposition::Complete,
            findings: vec![],
            verifications: vec![],
        },
    )
    .unwrap();
    assert_eq!(
        s.show_record(task.id).unwrap().status,
        RecordStatus::InProgress
    );
    s.create_agent_assignment(&NewAgentAssignment {
        id: "completion-owner".into(),
        record_id: task.id,
        project_slug: "test/api".into(),
        profile_revision: 1,
    })
    .unwrap();
    let blocked = || {
        let before = s.list_record_updates(task.id).unwrap();
        assert_eq!(
            s.done_record(task.id, None).unwrap_err().code,
            "execution-mode-unsafe-ownership"
        );
        assert_eq!(s.list_record_updates(task.id).unwrap(), before);
        assert_eq!(
            s.show_record(task.id).unwrap().status,
            RecordStatus::InProgress
        );
    };
    for state in [
        AgentAssignmentState::Provisioning,
        AgentAssignmentState::Working,
        AgentAssignmentState::Idle,
        AgentAssignmentState::Blocked,
        AgentAssignmentState::Unknown,
    ] {
        let current = s.agent_assignment("completion-owner").unwrap();
        if current.state != state {
            s.compare_and_set_agent_assignment_state("completion-owner", current.state, state)
                .unwrap();
        }
        blocked();
    }
    s.close_agent_assignment("completion-owner", AgentAssignmentClosureReason::Lost)
        .unwrap();
    blocked();
    let reopened = SqliteProjectRepository::open(&f.root.join("store.sqlite")).unwrap();
    assert_eq!(
        reopened.done_record(task.id, None).unwrap_err().code,
        "execution-mode-unsafe-ownership"
    );
    s.add_agent_assignment_resource(&NewAgentAssignmentResource {
        assignment_id: "completion-owner".into(),
        resource_kind: "agent".into(),
        resource_id: "retained".into(),
    })
    .unwrap();
    s.resolve_unknown_agent_resources("completion-owner")
        .unwrap();
    blocked();
    s.retire_agent_assignment_resource("completion-owner", "agent", "retained")
        .unwrap();
    s.hold_dispatch(task.id, "Retain operator safety diagnostic")
        .unwrap();
    let assignment = s.agent_assignment("completion-owner").unwrap();
    let resources = s
        .list_agent_assignment_resources("completion-owner")
        .unwrap();
    let checks = s.list_record_checks(task.id).unwrap();
    assert_eq!(
        s.done_record(task.id, Some("Verified manually"))
            .unwrap()
            .status,
        RecordStatus::Done
    );
    assert_eq!(s.agent_assignment("completion-owner").unwrap(), assignment);
    assert_eq!(
        s.list_agent_assignment_resources("completion-owner")
            .unwrap(),
        resources
    );
    assert_eq!(s.list_record_checks(task.id).unwrap(), checks);
    assert_eq!(
        s.dispatch_hold(task.id).unwrap().unwrap().reason,
        "Retain operator safety diagnostic"
    );
}

#[test]
fn dispatched_task_cannot_complete_through_operator_done() {
    let f = Fixture::new();
    let task = f.open(RecordType::Task, true);
    f.store
        .create_agent_assignment(&NewAgentAssignment {
            id: "historical".into(),
            record_id: task.id,
            project_slug: "test/api".into(),
            profile_revision: 1,
        })
        .unwrap();
    f.store.start_owned_record(task.id, "historical").unwrap();
    let before = f.store.list_record_updates(task.id).unwrap();
    assert_eq!(
        f.store.done_record(task.id, None).unwrap_err().code,
        "dispatched-completion-forbidden"
    );
    assert_eq!(f.store.list_record_updates(task.id).unwrap(), before);
}

#[test]
fn confirmed_retirement_without_resources_allows_explicit_manual_policy() {
    let f = Fixture::new();
    let task = f.open(RecordType::Task, true);
    f.store
        .create_agent_assignment(&NewAgentAssignment {
            id: "retired".into(),
            record_id: task.id,
            project_slug: "test/api".into(),
            profile_revision: 1,
        })
        .unwrap();
    f.store
        .close_agent_assignment("retired", AgentAssignmentClosureReason::Retired)
        .unwrap();
    change(
        &f.store,
        task.id,
        ExecutionMode::Dispatched,
        ExecutionMode::Manual,
    )
    .unwrap();
    assert_eq!(
        f.store.start_record(task.id).unwrap().execution_mode,
        ExecutionMode::Manual
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn between_attempt_mode_change_invalidates_current_work_but_preserves_history_findings_and_holds() {
    use ExecutionMode::{Dispatched, Manual};
    let f = Fixture::new();
    f.profiles();
    let s = &f.store;
    let task = f.open(RecordType::Task, true);
    change(s, task.id, Dispatched, Manual).unwrap();
    s.start_record(task.id).unwrap();
    let check = s.list_record_checks(task.id).unwrap().remove(0);
    let work = s.record_work_state(task.id).unwrap();
    let evidence = Evidence::Artifact {
        uri: "urn:deliverable".into(),
        sha256: "a".repeat(64),
    };
    s.begin_record_attempt(
        task.id,
        &AttemptInput {
            id: "manual".into(),
            check_id: check.id,
            generation: work.generation,
            evidence: evidence.clone(),
            actor: "operator".into(),
        },
    )
    .unwrap();
    let before = s.list_record_updates(task.id).unwrap();
    assert_eq!(
        change(s, task.id, Manual, Dispatched).unwrap_err().code,
        "attempt-active"
    );
    assert_eq!(s.list_record_updates(task.id).unwrap(), before);
    let outcome = OutcomeInput {
        attempt_id: "manual".into(),
        generation: work.generation,
        evidence,
        verdict: Verdict::Pass,
        actor: "operator".into(),
        message: "Verified".into(),
        disposition: OutcomeDisposition::Complete,
        findings: vec![],
        verifications: vec![],
    };
    let accepted = s.submit_record_outcome(task.id, &outcome).unwrap();
    s.raise_record_finding(
        task.id,
        &forge_app::findings::FindingRaiseInput {
            severity: forge_app::findings::Severity::Major,
            blocking: Some(true),
            check_id: None,
            assessment_id: None,
            actor: "reviewer".into(),
            message: "Retain unresolved concern".into(),
            revision: None,
            dirty: None,
        },
    )
    .unwrap();
    s.hold_dispatch(task.id, "Operator safety hold").unwrap();
    let hold = s.dispatch_hold(task.id).unwrap();
    let findings = s.list_record_findings(task.id, None, 100).unwrap();
    let before_work = s.record_work_state(task.id).unwrap();
    let before_checks = s.list_record_checks(task.id).unwrap();
    let before_updates = s.list_record_updates(task.id).unwrap();
    let fault = rusqlite::Connection::open(f.root.join("store.sqlite")).unwrap();
    fault.execute_batch("CREATE TRIGGER fail_mode_change BEFORE INSERT ON record_updates WHEN NEW.kind = 'execution-mode-changed' BEGIN SELECT RAISE(ABORT, 'mode update fault'); END;").unwrap();
    assert_eq!(
        change(s, task.id, Manual, Dispatched).unwrap_err().code,
        "storage"
    );
    assert_eq!(s.show_record(task.id).unwrap().execution_mode, Manual);
    assert_eq!(s.record_work_state(task.id).unwrap(), before_work);
    assert_eq!(s.list_record_checks(task.id).unwrap(), before_checks);
    assert_eq!(s.list_record_updates(task.id).unwrap(), before_updates);
    assert_eq!(
        s.list_record_findings(task.id, None, 100).unwrap(),
        findings
    );
    assert_eq!(s.dispatch_hold(task.id).unwrap(), hold);
    fault
        .execute_batch("DROP TRIGGER fail_mode_change")
        .unwrap();
    change(s, task.id, Manual, Dispatched).unwrap();
    assert_eq!(
        s.record_work_state(task.id).unwrap().generation,
        work.generation + 1
    );
    assert_eq!(
        s.begin_record_attempt(
            task.id,
            &AttemptInput {
                id: "forbidden-manual".into(),
                check_id: check.id,
                generation: s.record_work_state(task.id).unwrap().generation,
                evidence: outcome.evidence.clone(),
                actor: "operator".into(),
            }
        )
        .unwrap_err()
        .code,
        "manual-mode-required"
    );
    let after = s.list_record_checks(task.id).unwrap().remove(0);
    assert_eq!(
        (after.id, after.profile_revision, after.name),
        (check.id, check.profile_revision, check.name)
    );
    assert!(after.current_verdict.is_none());
    let page = s.show_record_check(task.id, check.id, None, 100).unwrap();
    assert_eq!(page.assessments.len(), 1);
    assert!(page.assessments[0].invalidation.is_some());
    assert_eq!(
        s.list_record_findings(task.id, None, 100).unwrap(),
        findings
    );
    assert_eq!(findings.findings.len(), 1);
    assert_eq!(s.dispatch_hold(task.id).unwrap(), hold);
    assert_eq!(
        change(s, task.id, Manual, Dispatched).unwrap_err().code,
        "stale-execution-mode"
    );
    assert_eq!(
        s.submit_record_outcome(task.id, &outcome).unwrap(),
        accepted
    );
    let reopened = SqliteProjectRepository::open(&f.root.join("store.sqlite")).unwrap();
    let updates = reopened.list_record_updates(task.id).unwrap();
    assert!(
        matches!(&updates[0].kind, RecordUpdateKind::ExecutionModeChanged { from: Manual, to: Dispatched, actor, message } if actor == "operator" && message == "Explicit policy handoff")
    );
    assert_eq!(
        reopened.show_record(task.id).unwrap().execution_mode,
        Dispatched
    );
    assert_eq!(
        reopened.submit_record_outcome(task.id, &outcome).unwrap(),
        accepted
    );
    change(&reopened, task.id, Dispatched, Manual).unwrap();
    assert!(
        reopened.list_record_checks(task.id).unwrap()[0]
            .current_verdict
            .is_none()
    );
    assert_eq!(reopened.dispatch_hold(task.id).unwrap(), hold);
}
