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

/// Content-bound salvage input, deliberately separate from clean commit evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetainedState {
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetainedWorkRecovery {
    pub record_id: u64,
    pub check_id: u64,
    pub request: RecoveryInput,
    pub worktree_path: std::path::PathBuf,
    pub generation: u64,
    pub created_at_millis: u64,
    #[serde(default)]
    pub retained_state: Option<RetainedState>,
}

impl Application {
    /// Authorize retained work as salvage input, not as an accepted checkpoint or pass.
    /// Does not clear holds, reset budgets, retire resources or launch a replacement.
    /// # Errors
    /// Rejects unsafe ownership, changed/unknown observations and conflicting retries.
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
        let observed = self.validate_retained_work(record, input, &claim)?;
        // External observations remain outside the writer transaction. Exclude external writers.
        if self.validate_retained_work(record, input, &claim)? != observed {
            return Err(RegistrationError::new(
                "recovery-state-changed",
                "Retained state changed during observation; exclude external writers and inspect again",
            ));
        }
        self.records
            .authorize_retained_work(record, input, &claim, observed.as_ref())
    }

    fn validate_retained_work(
        &self,
        record: u64,
        input: &RecoveryInput,
        claim: &WorktreeClaim,
    ) -> Result<Option<RetainedState>, RegistrationError> {
        crate::validate_managed_worktree_placement(&claim.path)?;
        let managed = self.managed_worktrees()?;
        if claim.path != managed.worktree_path(record)?
            || claim.state != crate::WorktreeClaimState::Active
        {
            return Err(RegistrationError::new(
                "worktree-recovery-invalid",
                "Claim does not match active canonical Task placement",
            ));
        }
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
        let (observed, dirty) = self.observe_retained_evidence(record, &project)?;
        let retained = if dirty {
            Some(self.worktree_adapter()?.retained_state(&claim.path)?)
        } else {
            None
        };
        if observed != input.evidence {
            return Err(RegistrationError::new(
                "recovery-evidence-changed",
                "Observed Repository HEAD differs from selected recovery evidence; preserve and inspect retained work",
            ));
        }
        Ok(retained)
    }

    /// Observe, without repairing, the retained checkout's HEAD and whether it is dirty.
    /// A dirty checkout's HEAD is selection input only, never clean checkpoint evidence.
    pub(crate) fn observe_retained_evidence(
        &self,
        record: u64,
        project: &crate::Project,
    ) -> Result<(Evidence, bool), RegistrationError> {
        let info = self.managed_worktree_info(record)?;
        if info.safety.facts.iter().any(|f| f == "dirty") {
            return Ok((
                Evidence::Repository {
                    repository: project.project_root.display().to_string(),
                    commit: self.inspector.head_commit(&info.claim.path)?,
                },
                true,
            ));
        }
        Ok((self.observe_task_evidence(record)?, false))
    }

    /// Revalidate clean and dirty salvage identity before reserving the first replacement.
    pub(crate) fn observe_recovery_input(
        &self,
        record: u64,
    ) -> Result<Option<RetainedWorkRecovery>, RegistrationError> {
        let Some(latest) = self
            .dispatch()?
            .list_agent_assignments(record)?
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        let work = self.record_work_state(record)?;
        let Some(recovery) = work.recoveries.last().filter(|r| {
            r.request.source_assignment == latest.id
                && work.input.as_ref() == Some(&r.request.evidence)
        }) else {
            return Ok(None);
        };
        let claim = self
            .managed_worktrees()?
            .worktree_claim(record)?
            .ok_or_else(|| {
                RegistrationError::new("managed-worktree-required", "Recovery claim missing")
            })?;
        if self.validate_retained_work(record, &recovery.request, &claim)?
            != recovery.retained_state
        {
            return Err(RegistrationError::new(
                "recovery-state-changed",
                "Authorized retained bytes or index changed; preserve evidence and inspect before new authorization",
            ));
        }
        Ok(Some(recovery.clone()))
    }
}
