//! Read-only repository progress verification, not evidence adoption or a grant.
use super::{Candidate, Claim};
use crate::{RegistrationError, outcomes::Evidence, retained_work::RetainedState};
use serde::Serialize;
use std::path::PathBuf;

/// One current claimed Forge snapshot, independent of mutable source profiles.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Binding {
    pub candidate: Candidate,
    pub project_root: PathBuf,
    pub worktree_path: PathBuf,
    pub base_commit: String,
    pub check_name: String,
}

fn denied() -> RegistrationError {
    RegistrationError::new(
        "durable-recovery-worktree-denied",
        "original worktree or evidence cannot be verified; preserve retained work and capacity",
    )
}

impl Binding {
    /// Validate the persisted repository identity before external observation.
    /// # Errors
    /// Rejects unsupported evidence, placement mismatch and incomplete bindings.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        let Evidence::Repository { repository, .. } = &self.candidate.attempt.evidence else {
            return Err(denied());
        };
        self.candidate.attempt.evidence.validate()?;
        Evidence::Repository {
            repository: repository.clone(),
            commit: self.base_commit.clone(),
        }
        .validate()?;
        if !self.project_root.is_absolute()
            || !self.worktree_path.is_absolute()
            || self.worktree_path.to_str() != Some(&self.candidate.execution_directory)
            || self.project_root.to_str() != Some(repository)
            || self.check_name.trim().is_empty()
        {
            return Err(denied());
        }
        Ok(())
    }

    /// Implementation progress is not a checkpoint. Other Checks must still
    /// inspect their exact clean bound input, never an advanced or dirty checkout.
    /// # Errors
    /// Rejects changed review/publication evidence and unknown content identity.
    pub fn validate_observation(&self, observed: &Observation) -> Result<(), RegistrationError> {
        self.validate()?;
        observed.head.validate()?;
        let Evidence::Repository { repository, .. } = &observed.head else {
            return Err(denied());
        };
        if self.project_root.to_str() != Some(repository)
            || observed.retained.sha256.len() != 64
            || !observed
                .retained
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || (self.check_name != "Implementation"
                && (observed.dirty || observed.head != self.candidate.attempt.evidence))
        {
            return Err(denied());
        }
        Ok(())
    }
}

/// HEAD and content/index identity stay separate from accepted Forge evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Observation {
    pub head: Evidence,
    pub dirty: bool,
    pub retained: RetainedState,
}

/// Read-only Git/filesystem boundary. Implementations must verify canonical
/// placement, repository/Task branch identity and ancestry of BOTH the persisted
/// Base Commit and original attempt input, then fingerprint retained bytes/index
/// without repair, optional index refresh writes, hooks, fetches or commits.
pub trait Inspector {
    /// # Errors
    /// Missing metadata, unsupported filesystem state or unsafe ancestry is unknown.
    fn observe(&self, binding: &Binding) -> Result<Observation, RegistrationError>;
}

/// Read every binding fact and the active claim in one consistent snapshot.
pub trait Repository {
    /// # Errors
    /// Rejects foreign/stale/cancelled claims and unavailable original bindings.
    fn claimed_worktree_binding(
        &self,
        workspace: &str,
        claim: &Claim,
    ) -> Result<Binding, RegistrationError>;
}

/// Retained observation only, not authority or a substitute for execution locks.
/// The caller must retain verified waiting-owner exclusivity throughout use.
#[derive(Debug)]
pub struct Inspection {
    workspace: String,
    claim: Claim,
    binding: Binding,
    observation: Observation,
}

impl Inspection {
    #[must_use]
    pub fn binding(&self) -> &Binding {
        &self.binding
    }

    #[must_use]
    pub fn observation(&self) -> &Observation {
        &self.observation
    }

    /// Revalidate after other external I/O, without adopting later progress.
    /// # Errors
    /// Any changed Forge or external observation invalidates this snapshot.
    pub fn verify(
        &self,
        repository: &dyn Repository,
        inspector: &dyn Inspector,
    ) -> Result<(), RegistrationError> {
        if repository.claimed_worktree_binding(&self.workspace, &self.claim)? != self.binding {
            return Err(denied());
        }
        let observed = inspector.observe(&self.binding)?;
        self.binding.validate_observation(&observed)?;
        if observed != self.observation
            || repository.claimed_worktree_binding(&self.workspace, &self.claim)? != self.binding
        {
            return Err(denied());
        }
        Ok(())
    }
}

/// Observe twice with live post-I/O snapshot validation. This never changes an
/// attempt, freshness, accepted evidence, salvage authorization or worktree bytes.
/// External writers still must be excluded: repeated reads are not a host lock.
/// # Errors
/// Rejects unknown/changed worktree state and stale original Forge bindings.
pub fn inspect(
    repository: &dyn Repository,
    inspector: &dyn Inspector,
    workspace: &str,
    claim: &Claim,
) -> Result<Inspection, RegistrationError> {
    let binding = repository.claimed_worktree_binding(workspace, claim)?;
    binding.validate()?;
    let observation = inspector.observe(&binding)?;
    binding.validate_observation(&observation)?;
    let inspection = Inspection {
        workspace: workspace.into(),
        claim: claim.clone(),
        binding,
        observation,
    };
    inspection.verify(repository, inspector)?;
    Ok(inspection)
}
