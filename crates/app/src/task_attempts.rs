//! Explicit acceptance execution, deliberately separate from normal dispatch rollout.
use crate::{
    AgentAssignment, Application, RecordType, RegistrationError, WorktreeClaimState,
    checks::Verdict, outcomes::Evidence,
};

impl Application {
    /// Observe, but never repair, a dispatched Task's deliverables.
    pub(crate) fn observe_task_evidence(&self, record: u64) -> Result<Evidence, RegistrationError> {
        let info = self.managed_worktree_info(record)?;
        if info.claim.state != WorktreeClaimState::Active
            || info
                .safety
                .facts
                .iter()
                .chain(&info.safety.blockers)
                .any(|fact| {
                    matches!(
                        fact.as_str(),
                        "dirty" | "checkout-missing" | "git-metadata-invalid"
                    )
                })
        {
            return Err(RegistrationError::new(
                "managed-worktree-invalid",
                format!(
                    "expected a clean active Managed Worktree; observed {}; investigate and explicitly resume",
                    info.safety.facts.join(", ")
                ),
            ));
        }
        let task = self.dispatch()?.show_record(record)?;
        let project = self.dispatch()?.project(&task.project_slugs[0])?;
        Ok(Evidence::Repository {
            repository: project.project_root.display().to_string(),
            commit: self.inspector.head_commit(&info.claim.path)?,
        })
    }

    /// Resume enrolled Tasks under bounded Check policy, or complete current passing work.
    /// Unclaimed Tasks do not advance. Replacements and completion require confirmed
    /// retirement/resource reconciliation; interruption is not handback.
    pub(crate) fn continue_task_check_acceptance(&self) -> Result<(), RegistrationError> {
        let repository = self.dispatch()?;
        for task in repository.list_records(&crate::RecordListFilter {
            project_slug: None,
            status: Some(crate::RecordStatus::InProgress),
            record_type: Some(RecordType::Task),
        })? {
            if crate::validate_task_dispatch_mode(&task).is_err() {
                continue;
            }
            let Some(latest) = repository
                .list_agent_assignments(task.id)?
                .into_iter()
                .next()
            else {
                continue;
            };
            let result = (|| {
                if self.retire_accepted_task_check(&latest)? {
                    match self.execute_task_check_acceptance(task.id) {
                        Ok(_) => {}
                        Err(e) if e.code == "checks-passed" => {
                            let work = self.record_work_state(task.id)?;
                            let observed = self.observe_task_evidence(task.id);
                            self.records.reconcile_task_evidence(task.id, &observed)?;
                            repository.complete_task_check_acceptance(
                                task.id,
                                work.generation,
                                &observed?,
                            )?;
                        }
                        Err(e) => return Err(e),
                    }
                }
                Ok(())
            })();
            if let Err(error) = result {
                tracing::warn!(
                    record_id = task.id,
                    code = error.code,
                    "controlled Check continuation blocked"
                );
            }
        }
        Ok(())
    }

    fn retire_accepted_task_check(
        &self,
        assignment: &AgentAssignment,
    ) -> Result<bool, RegistrationError> {
        let repository = self.dispatch()?;
        if !repository.begin_task_check_handoff(&assignment.id)? {
            return Ok(false);
        }
        let resources = repository.list_agent_assignment_resources(&assignment.id)?;
        if resources.iter().all(|r| r.retired_at_millis.is_some()) {
            return Ok(true);
        }
        let current = repository.agent_assignment(&assignment.id)?;
        let context = crate::agent_dispatch_context(repository, &current)?;
        let adapter = self
            .adapter_factory()?
            .adapter_for(&current.profile)
            .map_err(|e| {
                RegistrationError::new("agent-retirement-unavailable", format!("{e:?}"))
            })?;
        adapter
            .retire(&context, repository)
            .map_err(|e| RegistrationError::new("agent-retirement-failed", format!("{e:?}")))?;
        if repository
            .list_agent_assignment_resources(&assignment.id)?
            .iter()
            .any(|r| r.retired_at_millis.is_none() && r.resource_kind != "check-handoff")
        {
            return Err(RegistrationError::new(
                "assignment-recovery-blocked",
                "provider retirement left retained resources; reconcile before continuation",
            ));
        }
        repository.retire_agent_assignment_resource(
            &assignment.id,
            "check-handoff",
            "acceptance",
        )?;
        Ok(true)
    }

    /// Launch at most one Check-bound attempt for controlled acceptance testing.
    /// Requires a previously prepared, clean Managed Worktree. Continues only after safe
    /// retirement of a passing, interrupted or freshness-invalidated attempt. Completion is
    /// a separate reconciliation gate, never a side effect of outcome acceptance.
    /// # Errors
    /// Rejects unsupported owners, unsafe resources, unavailable pinned profiles and stale inputs.
    #[allow(clippy::too_many_lines)]
    pub fn execute_task_check_acceptance(
        &self,
        record: u64,
    ) -> Result<(AgentAssignment, bool), RegistrationError> {
        let repository = self.dispatch()?;
        let task = repository.show_record(record)?;
        if task.record_type != RecordType::Task {
            return Err(RegistrationError::new(
                "unsupported-record-type",
                "controlled Check acceptance supports only Tasks",
            ));
        }
        crate::validate_task_dispatch_mode(&task)?;
        if let Some(latest) = repository
            .list_agent_assignments(record)?
            .into_iter()
            .next()
        {
            self.retire_accepted_task_check(&latest)?;
        }
        if let Some(existing) = repository
            .list_agent_assignments(record)?
            .into_iter()
            .find(|a| a.closed_at_millis.is_none())
        {
            if repository.assignment_check_attempt(&existing.id)?.is_none() {
                return Err(RegistrationError::new(
                    "assignment-recovery-blocked",
                    "an unbound Assignment owns this Task",
                ));
            }
            return Ok((existing, true));
        }
        let observed = self.observe_task_evidence(record);
        self.records.reconcile_task_evidence(record, &observed)?;
        let evidence = observed?;
        let check = self
            .list_record_checks(record)?
            .into_iter()
            .find(|check| check.current_verdict != Some(Verdict::Pass))
            .ok_or_else(|| {
                RegistrationError::new(
                    "checks-passed",
                    "all Checks currently pass; no attempt is authorized",
                )
            })?;
        let revision = check.profile_revision.ok_or_else(|| {
            RegistrationError::new(
                "invalid-check-profile",
                "next Check has no pinned Agent Profile revision",
            )
        })?;
        let profile = repository
            .agent_profile(RecordType::Task, revision)
            .map_err(|e| RegistrationError::new("invalid-check-profile", e.message))?;
        if profile.launch_policy.is_none() {
            return Err(RegistrationError::new(
                "invalid-check-profile",
                "pinned revision has no registered launch policy",
            ));
        }
        let factory = self.adapter_factory()?;
        factory.validate_preflight(&profile).map_err(|_| {
            RegistrationError::new(
                "invalid-check-profile",
                format!(
                    "Check {} pinned revision {revision} is unusable; restore its prerequisites",
                    check.id
                ),
            )
        })?;
        let configuration = repository.workspace_dispatch_configuration(&task.workspace_id)?;
        let (Some(task_profile), Some(grilling_profile), Some(binding)) = (
            configuration.task_profile,
            configuration.grilling_profile,
            configuration.binding,
        ) else {
            return Err(RegistrationError::new(
                "workspace-dispatch-incomplete",
                "configure Workspace dispatch and verify its provider binding",
            ));
        };
        if !configuration.provider_verified {
            return Err(RegistrationError::new(
                "provider-verification-stale",
                "verify Workspace binding at the current provider endpoint",
            ));
        }
        let selection = crate::DispatchPreflightSelection {
            task_revision: task_profile.revision,
            grilling_revision: grilling_profile.revision,
            provider_generation: configuration.provider_generation,
            binding_id: binding.id.clone(),
        };
        // Probe the pinned adapter, never replace its revision with a live Workspace selection.
        let adapter = factory.adapter_for(&profile).map_err(|_| {
            RegistrationError::new(
                "invalid-check-profile",
                "pinned profile adapter unavailable",
            )
        })?;
        adapter.probe().map_err(|_| {
            RegistrationError::new(
                "agent-dispatch-unavailable",
                "pinned profile provider unavailable",
            )
        })?;
        if adapter
            .verify_workspace(&binding.id)
            .map_err(|_| {
                RegistrationError::new(
                    "agent-dispatch-unavailable",
                    "cannot verify exact provider Workspace",
                )
            })?
            .id
            != binding.id
        {
            return Err(RegistrationError::new(
                "agent-dispatch-unavailable",
                "provider Workspace identity mismatch",
            ));
        }
        let (assignment, existing) = repository.claim_task_check_acceptance(
            record,
            &crate::next_dispatch_assignment_id(),
            &selection,
            &evidence,
            check.id,
            revision,
        )?;
        if !existing {
            crate::reconcile_assignment(repository, adapter.as_ref(), &assignment, true)?;
        }
        Ok((repository.agent_assignment(&assignment.id)?, existing))
    }
}
