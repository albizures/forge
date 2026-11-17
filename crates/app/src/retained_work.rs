//! Explicit operator authority to inspect and salvage interrupted Implementation work.
use crate::{Application, RegistrationError, WorktreeClaim, outcomes::Evidence};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryInput {
    pub source_assignment: String,
    pub generation: u64,
    pub evidence: Evidence,
    pub actor: String,
    pub message: String,
    pub confirm_old_agent_cannot_act: bool,
    pub confirm_resources_safe: bool,
}
impl RecoveryInput {
    /// # Errors
    /// Requires explicit safety assertions, immutable evidence and nonblank attribution.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        self.evidence.validate()?;
        if self.source_assignment.trim().is_empty()
            || self.actor.trim().is_empty()
            || self.message.trim().is_empty()
            || !self.confirm_old_agent_cannot_act
            || !self.confirm_resources_safe
            || !matches!(self.evidence, Evidence::Repository { .. })
        {
            return Err(RegistrationError::new(
                "invalid-recovery",
                "Retained-work recovery requires a source Assignment, Repository commit, actor, explanation and both explicit safety confirmations",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetainedWorkRecovery {
    pub record_id: u64,
    pub check_id: u64,
    pub request: RecoveryInput,
    pub worktree_path: std::path::PathBuf,
    pub generation: u64,
    pub created_at_millis: u64,
}

impl Application {
    /// Authorize clean retained work as salvage input, not as an accepted checkpoint or pass.
    /// Does not clear holds, reset budgets, retire resources or launch a replacement.
    /// # Errors
    /// Rejects unsafe ownership, changed observations, dirty work and conflicting retries.
    pub fn recover_retained_work(
        &self,
        record: u64,
        input: &RecoveryInput,
    ) -> Result<RetainedWorkRecovery, RegistrationError> {
        input.validate()?;
        // Replay is read-only and cannot restore mutation authority, including after dispatch.
        if let Some(previous) = self
            .record_work_state(record)?
            .recoveries
            .into_iter()
            .find(|r| r.request.source_assignment == input.source_assignment)
        {
            if previous.request == *input {
                return Ok(previous);
            }
            return Err(RegistrationError::new(
                "recovery-conflict",
                "Source Assignment already has a different recovery authorization",
            ));
        }
        let managed = self.managed_worktrees()?;
        let claim = managed.worktree_claim(record)?.ok_or_else(|| {
            RegistrationError::new(
                "managed-worktree-required",
                "Retained-work recovery requires an existing active claim",
            )
        })?;
        let expected_path = managed.worktree_path(record)?;
        if claim.path != expected_path {
            return Err(RegistrationError::new(
                "worktree-recovery-invalid",
                "Claim does not match canonical Task placement",
            ));
        }
        self.validate_retained_work(record, input, &claim)?;
        // Git/provider observations never run in the writer transaction. Recheck Git immediately
        // before committing; persistence atomically revalidates mutable Forge ownership/identity.
        self.validate_retained_work(record, input, &claim)?;
        self.records.authorize_retained_work(record, input, &claim)
    }

    fn validate_retained_work(
        &self,
        record: u64,
        input: &RecoveryInput,
        claim: &WorktreeClaim,
    ) -> Result<(), RegistrationError> {
        crate::validate_managed_worktree_placement(&claim.path)?;
        let managed = self.managed_worktrees()?;
        let task = managed.show_record(record)?;
        crate::validate_task_dispatch_mode(&task)?;
        if task.status != crate::RecordStatus::InProgress || task.project_slugs.len() != 1 {
            return Err(RegistrationError::new(
                "record-not-ready",
                "Recovery requires an In Progress single-Project Task",
            ));
        }
        let project = managed.project(&task.project_slugs[0])?;
        let base = managed.task_base_commit(record)?.ok_or_else(|| {
            RegistrationError::new(
                "task-base-commit-unavailable",
                "Task has no persisted Base Commit",
            )
        })?;
        self.worktree_adapter()?.validate_recovery(
            &project.project_root,
            &claim.path,
            &crate::stacks::task_branch(record),
            &base,
        )?;
        if self.observe_task_evidence(record)? != input.evidence {
            return Err(RegistrationError::new(
                "recovery-evidence-changed",
                "Observed clean Repository commit differs from the selected recovery evidence; preserve and inspect retained work",
            ));
        }
        Ok(())
    }
}
