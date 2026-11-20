//! Forge-side recovery preconditions, not a grant or an execution claim.
use crate::{
    Record, RecordStatus, RegistrationError, authorization_lease::Lease, outcomes::Attempt,
};

use super::Launch;

/// A consistent read of the original binding. Retained grants are evidence only:
/// neither this value nor an unexpired old lease permits a new process to run.
/// External ownership/process cleanup, committed runtime/effect inspection and
/// worktree observations remain independent gates. Revalidate mutable facts in
/// the eventual recovery-claim transaction before committing fresh authority.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub launch: Launch,
    pub admission_id: String,
    pub attempt: Attempt,
    pub execution_directory: String,
    pub last_lease: Lease,
}

/// Policy for the original attempt, not adoption of observed implementation
/// progress as new evidence. The store supplies current facts in one snapshot.
/// # Errors
/// Rejects unsupported mode, lifecycle or mismatched/stale bound identities.
pub fn validate_current_attempt(
    task: &Record,
    attempt: &Attempt,
    launch: &Launch,
    current_generation: u64,
) -> Result<(), RegistrationError> {
    crate::validate_task_dispatch_mode(task)?;
    if !matches!(task.status, RecordStatus::Open | RecordStatus::InProgress)
        || attempt.record_id != task.id
        || attempt.id != launch.attempt_id
        || attempt.actor != launch.assignment_id
    {
        return Err(RegistrationError::new(
            "durable-recovery-denied",
            "recovery requires the original live Task and bound Assignment attempt",
        ));
    }
    if attempt.generation != current_generation {
        return Err(RegistrationError::new(
            "durable-binding-stale",
            "recovery cannot resume an invalidated Check attempt",
        ));
    }
    Ok(())
}

/// Trusted daemon persistence port, not an operator or Assignment capability API.
/// Inspection never claims execution, clears blockers, issues a lease, increments
/// attempts, adopts repository evidence or consumes a Continuation allowance.
pub trait Repository: Send + Sync {
    /// Read the current Forge-side candidate from one transaction snapshot.
    /// Source profile files and model authentication are not consulted here.
    /// # Errors
    /// Rejects foreign, stale, closed, blocked, incompletely admitted or competing
    /// bindings, missing retained execution evidence and storage failures.
    fn recovery_candidate(
        &self,
        workspace: &str,
        launch: &Launch,
    ) -> Result<Candidate, RegistrationError>;
}
