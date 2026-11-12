//! Explicit acceptance execution, deliberately separate from normal dispatch rollout.
use crate::{
    AgentAssignment, Application, RecordType, RegistrationError, WorktreeClaimState,
    checks::Verdict, outcomes::Evidence,
};

impl Application {
    /// Launch at most one Check-bound attempt for controlled acceptance testing.
    /// Requires a previously prepared, clean Managed Worktree. Never continues or completes work.
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
        let info = self.managed_worktree_info(record)?;
        if info.claim.state != WorktreeClaimState::Active
            || info.safety.facts.iter().any(|f| {
                matches!(
                    f.as_str(),
                    "dirty" | "checkout-missing" | "git-metadata-invalid"
                )
            })
        {
            return Err(RegistrationError::new(
                "managed-worktree-invalid",
                "controlled acceptance requires a clean active Managed Worktree",
            ));
        }
        let project = repository.project(&task.project_slugs[0])?;
        let evidence = Evidence::Repository {
            repository: project.project_root.display().to_string(),
            commit: self.inspector.head_commit(&info.claim.path)?,
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
