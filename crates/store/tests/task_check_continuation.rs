use forge_app::{
    AgentDispatchRepository, Application, DispatchWorkspace,
    ExecutionMode::{Dispatched, Manual},
    ManagedWorktreeAdapter, ManagedWorktreeRepository, NewTypedRecord, Project, ProjectRepository,
    RecordRepository, RecordStatus, RecordType, RegistrationError, RepositoryInspector,
    WorkspaceDispatchRepository, WorkspaceRepository, WorktreeSafetyAssessment,
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
    checks::Verdict,
    outcomes::{Evidence, OutcomeDisposition, OutcomeInput},
};
use forge_store::SqliteProjectRepository;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
#[path = "support/task_attempt_adapters.rs"]
mod adapters;
use adapters::Provider;

struct Head(Arc<Mutex<String>>);
impl RepositoryInspector for Head {
    fn inspect(&self, _: &std::path::Path) -> Result<Project, RegistrationError> {
        unreachable!()
    }
    fn head_commit(&self, _: &std::path::Path) -> Result<String, RegistrationError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

struct Checkout(Arc<AtomicBool>);
impl ManagedWorktreeAdapter for Checkout {
    fn create(
        &self,
        _: &std::path::Path,
        _: &std::path::Path,
        _: &str,
        _: &str,
    ) -> Result<(), RegistrationError> {
        unreachable!()
    }
    fn remove(&self, _: &std::path::Path, _: bool) -> Result<(), RegistrationError> {
        unreachable!()
    }
    fn assess(&self, path: &std::path::Path) -> WorktreeSafetyAssessment {
        let mut assessment = adapters::Local.assess(path);
        if self.0.load(std::sync::atomic::Ordering::SeqCst) {
            assessment.facts.push("dirty".into());
            assessment.removable = false;
        } else {
            assessment.facts.extend(["clean".into(), "ahead:1".into()]);
        }
        assessment
    }
    fn run_setup_hook(
        &self,
        _: &std::path::Path,
        _: &str,
        _: &str,
        _: u64,
        _: &str,
        _: &str,
    ) -> Result<(), RegistrationError> {
        unreachable!()
    }
}

#[test]
fn dirty_implementation_creates_a_durable_hold_without_an_assessment() {
    run_case("dirty");
}

#[test]
fn controlled_checks_continue_only_after_retirement_and_survive_restart() {
    run_case("success");
}

#[test]
fn review_mutation_is_held_and_invalidates_historical_passes() {
    run_case("review-mutation");
}

#[test]
fn description_edit_retires_before_same_commit_fresh_implementation() {
    run_case("description-edit");
}

#[test]
fn external_commit_after_acceptance_cannot_launch_a_review() {
    run_case("external-commit");
}

#[test]
fn unlinked_blocker_requires_an_explicit_review_owner_even_after_hold_clearance() {
    run_case("unlinked");
}

#[test]
fn all_passing_checks_with_a_residual_blocker_are_held_not_completed() {
    run_case("residual");
}

#[test]
fn review_handback_remediates_and_reruns_both_original_checks() {
    run_case("handback");
}

#[test]
fn review_handback_can_verify_an_overlooked_fix_at_the_same_commit() {
    run_case("same-commit-handback");
}

#[allow(clippy::too_many_lines)]
fn handback(
    store: &SqliteProjectRepository,
    path: &std::path::Path,
    application: &impl Fn() -> Application,
    head: &Mutex<String>,
    unavailable: &AtomicBool,
    change_commit: bool,
) {
    use forge_app::{
        findings::{FindingState, Severity},
        outcomes::{FindingVerification, OutcomeFinding},
    };
    let app = application();
    let task = store
        .list_records(&forge_app::RecordListFilter {
            project_slug: None,
            status: None,
            record_type: Some(RecordType::Task),
        })
        .unwrap()[0]
        .id;
    let checks = app.list_record_checks(task).unwrap();
    let secrets = path.parent().unwrap().join("secrets");
    std::fs::create_dir_all(&secrets).unwrap();
    let mut finding = None;
    let mut replay = None;
    let mut stale = None;
    for index in 0..6 {
        let (assignment, _) = app.execute_task_check_acceptance(task).unwrap();
        let attempt = store
            .assignment_check_attempt(&assignment.id)
            .unwrap()
            .unwrap();
        assert_eq!(attempt.check_id, checks[index % 3].id);
        assert_eq!(
            Some(assignment.profile.revision),
            checks[index % 3].profile_revision
        );
        assert_eq!(attempt.generation, if index < 3 { 1 } else { 2 });
        std::fs::write(secrets.join(&assignment.id), "secret").unwrap();
        let session = app.assignment(&secrets, &assignment.id, "secret").unwrap();
        session.acknowledge().unwrap();
        session.start().unwrap();
        session.context().unwrap();
        if index == 3 && change_commit {
            *head.lock().unwrap() = "b".repeat(40);
        }
        let mut evidence = attempt.evidence.clone();
        if let Evidence::Repository { commit, .. } = &mut evidence {
            commit.clone_from(&head.lock().unwrap());
        }
        let mut input = OutcomeInput {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            evidence,
            verdict: Verdict::Pass,
            actor: "ignored".into(),
            message: "Assessed deliverable".into(),
            disposition: OutcomeDisposition::Complete,
            findings: vec![],
            verifications: vec![],
        };
        if index == 1 {
            stale = Some((assignment.id.clone(), input.clone()));
        }
        if index == 2 {
            input.verdict = Verdict::Fail;
            input.disposition = OutcomeDisposition::Handback;
            input.findings.push(OutcomeFinding {
                severity: Severity::Major,
                blocking: Some(true),
                message: "Report omits required result".into(),
            });
            // Fail after Assessment, Finding and invalidation writes, before acceptance commits.
            let fault = rusqlite::Connection::open(path).unwrap();
            fault.execute_batch("CREATE TRIGGER reject_outcome BEFORE UPDATE OF result_json ON record_attempts WHEN NEW.result_json IS NOT NULL BEGIN SELECT RAISE(ABORT, 'injected acceptance failure'); END;").unwrap();
            let before = app.list_record_checks(task).unwrap();
            let updates = store.list_record_updates(task).unwrap();
            assert!(session.submit_outcome(&input).is_err());
            assert_eq!(app.list_record_checks(task).unwrap(), before);
            assert_eq!(store.list_record_updates(task).unwrap(), updates);
            assert!(
                app.list_record_findings(task, None, 100, None)
                    .unwrap()
                    .findings
                    .is_empty()
            );
            assert_eq!(app.record_work_state(task).unwrap().generation, 1);
            assert!(store.dispatch_hold(task).unwrap().is_none());
            fault.execute_batch("DROP TRIGGER reject_outcome").unwrap();
        }
        if index == 3 || index == 5 {
            let raised: &forge_app::findings::Finding = finding.as_ref().unwrap();
            input.verifications.push(FindingVerification {
                finding_id: raised.id,
                expected_version: raised.version,
                message: "Verified against current deliverable".into(),
            });
            if index == 3 {
                let before = store.list_record_updates(task).unwrap();
                assert_eq!(
                    session.submit_outcome(&input).unwrap_err().code,
                    "unauthorized-verification"
                );
                assert_eq!(store.list_record_updates(task).unwrap(), before);
                input.verifications.clear();
            } else {
                let mut unresolved = input.clone();
                unresolved.verifications.clear();
                let before = store.list_record_updates(task).unwrap();
                assert_eq!(
                    session.submit_outcome(&unresolved).unwrap_err().code,
                    "unresolved-check-findings"
                );
                assert_eq!(store.list_record_updates(task).unwrap(), before);
                assert!(
                    app.list_record_checks(task).unwrap()[2]
                        .current_verdict
                        .is_none()
                );
            }
        }
        let accepted = session.submit_outcome(&input).unwrap();
        assert_eq!(session.submit_outcome(&input).unwrap(), accepted);
        if index == 2 {
            finding = Some(accepted.findings[0].clone());
            replay = Some((assignment.id.clone(), input.clone(), accepted.clone()));
            assert_eq!(accepted.work.generation, 2);
            assert!(
                app.list_record_checks(task)
                    .unwrap()
                    .iter()
                    .all(|c| c.current_verdict.is_none())
            );
            assert!(
                app.list_record_checks(task).unwrap().iter().all(|c| c
                    .latest_assessment
                    .as_ref()
                    .unwrap()
                    .invalidation
                    .is_some())
            );
            // Linked reviewer Findings must allow remediation without operator clearance.
            assert!(store.dispatch_hold(task).unwrap().is_none());
            let (old_id, old) = stale.as_ref().unwrap();
            let old_session = app.assignment(&secrets, old_id, "secret").unwrap();
            let mut late = old.clone();
            late.message = "Late changed submission".into();
            assert!(old_session.submit_outcome(&late).is_err());
            // Handback is accepted, but cleanup failure still reserves ownership/capacity.
            unavailable.store(true, std::sync::atomic::Ordering::SeqCst);
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
            assert!(
                store
                    .list_agent_assignments(task)
                    .unwrap()
                    .iter()
                    .all(|a| a.closed_at_millis.is_some())
            );
            unavailable.store(false, std::sync::atomic::Ordering::SeqCst);
        }
        // New connections reconcile retirement and start the next original Check.
        application()
            .reconcile_agent_dispatch("handback", (index as u64 + 1) * 400_000)
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
    }
    let (id, input, accepted) = replay.unwrap();
    let restarted = application();
    assert_eq!(
        restarted
            .assignment(&secrets, &id, "secret")
            .unwrap()
            .submit_outcome(&input)
            .unwrap(),
        accepted
    );
    let after = restarted.list_record_checks(task).unwrap();
    assert!(
        after
            .iter()
            .all(|c| c.current_verdict == Some(Verdict::Pass))
    );
    assert_eq!(
        after
            .iter()
            .map(|c| (c.id, c.profile_revision))
            .collect::<Vec<_>>(),
        checks
            .iter()
            .map(|c| (c.id, c.profile_revision))
            .collect::<Vec<_>>()
    );
    let finding = restarted
        .show_record_finding(task, finding.unwrap().id, None, 100, None)
        .unwrap();
    assert_eq!(finding.finding.state, FindingState::Resolved);
    assert_eq!(finding.finding.verification_check_id, Some(checks[2].id));
    assert_eq!(store.list_agent_assignments(task).unwrap().len(), 6);
    assert_eq!(
        restarted.show_record(task).unwrap().status,
        RecordStatus::InProgress
    );
    assert!(store.dispatch_hold(task).unwrap().is_none());
}

#[allow(clippy::too_many_lines)]
fn run_case(case: &str) {
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
    let head = Arc::new(Mutex::new("a".repeat(40)));
    let dirty = Arc::new(AtomicBool::new(false));
    let application = || {
        Application::with_managed_worktrees_and_agent_dispatch(
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(Checkout(dirty.clone())),
            Box::new(SqliteProjectRepository::open(&path).unwrap()),
            Box::new(Provider {
                unavailable: unavailable.clone(),
                launches: launches.clone(),
            }),
            Box::new(Head(head.clone())),
        )
        .with_workspace_repository(Box::new(SqliteProjectRepository::open(&path).unwrap()))
    };
    if matches!(case, "handback" | "same-commit-handback") {
        handback(
            &store,
            &path,
            &application,
            &head,
            &unavailable,
            case == "handback",
        );
        std::fs::remove_dir_all(root).unwrap();
        return;
    }
    let app = application();
    if case == "unlinked" {
        use forge_app::findings::{
            FindingMutation, FindingMutationInput, FindingRaiseInput, FindingResolveInput, Severity,
        };
        let raised = app
            .raise_record_finding(
                task.id,
                &FindingRaiseInput {
                    severity: Severity::Major,
                    blocking: Some(true),
                    check_id: None,
                    assessment_id: None,
                    actor: "operator".into(),
                    message: "Needs a verification owner".into(),
                    revision: None,
                    dirty: None,
                },
            )
            .unwrap();
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        store.resume_dispatch(task.id).unwrap();
        assert_eq!(
            app.execute_task_check_acceptance(task.id).unwrap_err().code,
            "dispatch-held"
        );
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        assert!(launches.lock().unwrap().is_empty());
        let checks = app.list_record_checks(task.id).unwrap();
        app.mutate_record_finding(
            task.id,
            raised.id,
            &FindingMutationInput {
                change: FindingMutation::AssignVerification {
                    check_id: checks[2].id,
                },
                explanation: FindingResolveInput {
                    expected_version: raised.version,
                    actor: "operator".into(),
                    message: "Review 2 owns verification".into(),
                    revision: None,
                    dirty: None,
                },
            },
        )
        .unwrap();
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        store.resume_dispatch(task.id).unwrap();
        application()
            .execute_task_check_acceptance(task.id)
            .unwrap();
        assert_eq!(launches.lock().unwrap().len(), 1);
        std::fs::remove_dir_all(root).unwrap();
        return;
    }
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
        if index == 0 {
            // Authorized Implementation output B is not external drift from input A.
            *head.lock().unwrap() = "b".repeat(40);
        } else {
            assert!(
                matches!(&attempt.evidence, Evidence::Repository { commit, .. } if commit == &"b".repeat(40))
            );
        }
        let mut output = attempt.evidence.clone();
        if let Evidence::Repository { commit, .. } = &mut output {
            *commit = "b".repeat(40);
        }
        let mut input = OutcomeInput {
            attempt_id: assignment.id.clone(),
            generation: attempt.generation,
            evidence: output,
            verdict: Verdict::Pass,
            actor: "ignored".into(),
            message: "Passed".into(),
            disposition: OutcomeDisposition::Complete,
            findings: vec![],
            verifications: vec![],
        };
        if index == 0 && case == "residual" {
            input.findings.push(forge_app::outcomes::OutcomeFinding {
                severity: forge_app::findings::Severity::Major,
                blocking: Some(true),
                message: "Implementation-owned residual blocker".into(),
            });
        }
        if index == 0 && case == "dirty" {
            dirty.store(true, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(
                session.submit_outcome(&input).unwrap_err().code,
                "evidence-drift"
            );
            assert!(store.dispatch_hold(task.id).unwrap().is_some());
            assert!(
                app.list_record_checks(task.id).unwrap()[0]
                    .latest_assessment
                    .is_none()
            );
            application()
                .reconcile_agent_dispatch("dirty", 400_000)
                .unwrap();
            assert_eq!(launches.lock().unwrap().len(), 1);
            assert!(store.dispatch_hold(task.id).unwrap().is_some());
            assert!(dirty.load(std::sync::atomic::Ordering::SeqCst));
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
        if index == 1 && matches!(case, "review-mutation" | "description-edit") {
            if case == "review-mutation" {
                *head.lock().unwrap() = "c".repeat(40);
                assert_eq!(
                    session.submit_outcome(&input).unwrap_err().code,
                    "evidence-drift"
                );
                assert!(store.dispatch_hold(task.id).unwrap().is_some());
            } else {
                app.edit_record(task.id, "Changed requirements").unwrap();
            }
            assert_eq!(
                session.submit_outcome(&input).unwrap_err().code,
                "stale-generation"
            );
            let work = app.record_work_state(task.id).unwrap();
            assert_eq!(work.generation, attempt.generation + 1);
            assert!(
                app.list_record_checks(task.id)
                    .unwrap()
                    .iter()
                    .all(|c| c.current_verdict.is_none())
            );
            assert!(
                app.list_record_checks(task.id).unwrap()[0]
                    .latest_assessment
                    .is_some()
            );
            unavailable.store(true, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(
                application()
                    .execute_task_check_acceptance(task.id)
                    .unwrap_err()
                    .code,
                "agent-retirement-failed"
            );
            assert_eq!(launches.lock().unwrap().len(), 2);
            unavailable.store(false, std::sync::atomic::Ordering::SeqCst);
            application()
                .reconcile_agent_dispatch("freshness", 800_000)
                .unwrap();
            if case == "review-mutation" {
                assert!(store.dispatch_hold(task.id).unwrap().is_some());
                assert_eq!(launches.lock().unwrap().len(), 2);
                store.resume_dispatch(task.id).unwrap();
            }
            let (replacement, _) = application()
                .execute_task_check_acceptance(task.id)
                .unwrap();
            let fresh = store
                .assignment_check_attempt(&replacement.id)
                .unwrap()
                .unwrap();
            assert_eq!(fresh.check_id, checks[0].id);
            assert_eq!(fresh.generation, attempt.generation + 1);
            assert_eq!(replacement.profile.revision, pinned.revision);
            assert!(
                matches!(fresh.evidence, Evidence::Repository { commit, .. } if commit == head.lock().unwrap().as_str())
            );
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
        if index == 0 {
            let mut false_output = input.clone();
            false_output.evidence = attempt.evidence.clone();
            assert_eq!(
                session.submit_outcome(&false_output).unwrap_err().code,
                "evidence-mismatch"
            );
            assert!(store.dispatch_hold(task.id).unwrap().is_none());
            assert!(
                app.list_record_checks(task.id).unwrap()[0]
                    .latest_assessment
                    .is_none()
            );
        }
        let accepted = session.submit_outcome(&input).unwrap();
        if case == "residual" {
            assert_eq!(store.dispatch_hold(task.id).unwrap().is_some(), index == 2);
        }
        if index == 0 && case == "external-commit" {
            *head.lock().unwrap() = "c".repeat(40);
            assert_eq!(session.submit_outcome(&input).unwrap(), accepted);
            assert_eq!(
                application()
                    .execute_task_check_acceptance(task.id)
                    .unwrap_err()
                    .code,
                "evidence-drift"
            );
            assert_eq!(launches.lock().unwrap().len(), 1);
            assert!(store.dispatch_hold(task.id).unwrap().is_some());
            assert_eq!(
                app.record_work_state(task.id).unwrap().generation,
                attempt.generation + 1
            );
            assert!(
                app.list_record_checks(task.id)
                    .unwrap()
                    .iter()
                    .all(|c| c.current_verdict.is_none())
            );
            assert_eq!(session.submit_outcome(&input).unwrap(), accepted);
            assert_eq!(
                session.context().err().unwrap().code,
                "assignment-capability-denied"
            );
            let restarted = application();
            let replay = restarted
                .assignment(&secrets, &assignment.id, "secret")
                .unwrap();
            assert_eq!(replay.submit_outcome(&input).unwrap(), accepted);
            let mut conflicting = input.clone();
            conflicting.message = "Different result".into();
            assert_eq!(
                replay.submit_outcome(&conflicting).unwrap_err().code,
                "outcome-conflict"
            );
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
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
        if case == "residual" {
            "dispatch-held"
        } else {
            "checks-passed"
        }
    );
    assert_eq!(
        app.dispatch_record(task.id).unwrap_err().code,
        "task-dispatch-unsupported-contract"
    );
    if case == "residual" {
        let finding = app
            .list_record_findings(task.id, None, 100, None)
            .unwrap()
            .findings
            .remove(0);
        let hold = store.dispatch_hold(task.id).unwrap().unwrap();
        assert!(hold.reason.contains(&finding.id.to_string()));
        store.resume_dispatch(task.id).unwrap();
        assert_eq!(
            application()
                .execute_task_check_acceptance(task.id)
                .unwrap_err()
                .code,
            "dispatch-held"
        );
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        application()
            .reconcile_agent_dispatch("residual", 2_000_000)
            .unwrap();
        assert_eq!(
            app.show_record(task.id).unwrap().status,
            RecordStatus::InProgress
        );
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        assert_eq!(launches.lock().unwrap().len(), 3);
        std::fs::remove_dir_all(root).unwrap();
        return;
    }
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
