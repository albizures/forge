//! Forge-side recovery preconditions, not a grant or an execution claim.
use crate::{
    Record, RecordStatus, RegistrationError, authorization_lease::Lease, outcomes::Attempt,
};

use super::Launch;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod authorization;
pub mod quiescence;
pub mod session;

/// A consistent read of the original binding. Retained grants are evidence only:
/// neither this value nor an unexpired old lease permits a new process to run.
/// External ownership/process cleanup, committed runtime/effect inspection and
/// worktree observations remain independent gates. Revalidate mutable facts in
/// the eventual recovery-claim transaction before committing fresh authority.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub launch: Launch,
    pub admission_id: String,
    pub attempt: Attempt,
    pub execution_directory: String,
    pub last_lease: Lease,
}

/// An immutable reservation for a trusted daemon's recovery operation. This is
/// a fence, not execution authority: host cleanup/ownership, runtime inspection,
/// worktree validation and a separately committed fresh lease are still required.
/// No timeout, daemon restart or missing reply transfers this reservation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub operation_id: String,
    pub daemon_epoch: String,
    pub launch: Launch,
    /// Commits the full candidate, including original attempt context, without
    /// copying Task bodies/handoffs into ordinary resource diagnostics.
    pub candidate_sha256: String,
}

impl Claim {
    /// Construct a content-bound reservation. This still grants no authority.
    /// # Errors
    /// Rejects invalid operation identities or serialization failures.
    pub fn for_candidate(
        candidate: &Candidate,
        operation_id: String,
        daemon_epoch: String,
    ) -> Result<Self, RegistrationError> {
        let bytes = serde_json::to_vec(candidate).map_err(|_| {
            RegistrationError::new(
                "durable-recovery-claim-invalid",
                "invalid candidate encoding",
            )
        })?;
        let claim = Self {
            operation_id,
            daemon_epoch,
            launch: candidate.launch.clone(),
            candidate_sha256: format!("{:x}", Sha256::digest(bytes)),
        };
        claim.validate()?;
        Ok(claim)
    }
    /// # Errors
    /// Rejects empty/unbounded operation identities and malformed launch bindings.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        self.launch.validate()?;
        if self.candidate_sha256.len() != 64
            || !self
                .candidate_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || [&self.operation_id, &self.daemon_epoch]
                .iter()
                .any(|s| s.trim().is_empty() || s.len() > 200 || s.chars().any(char::is_control))
        {
            return Err(RegistrationError::new(
                "durable-recovery-claim-invalid",
                "recovery requires bounded operation and daemon identities",
            ));
        }
        Ok(())
    }
}

/// Validate a reservation before verified, revoked retirement cancels it.
/// This consumes only the execution fence, not its immutable history, and never
/// permits a new owner to execute. The caller must independently verify the
/// original owner's cleanup receipt and revocation in the same transaction.
/// # Errors
/// Rejects malformed claims and claims for any other immutable runtime binding.
pub fn validate_retirement(claim: &Claim, launch: &Launch) -> Result<(), RegistrationError> {
    claim.validate()?;
    if claim.launch != *launch {
        return Err(RegistrationError::new(
            "durable-recovery-claim-conflict",
            "retirement cannot reconcile a different recovery binding",
        ));
    }
    Ok(())
}

/// Recovery reservations and grant audits are owned by specialized operations.
/// # Errors
/// Rejects generic resource insertion/retirement of recovery reservations/audits.
pub fn validate_resource_mutation(kind: &str) -> Result<(), RegistrationError> {
    if matches!(kind, "recovery-claim" | "recovery-grant") {
        return Err(RegistrationError::new(
            "durable-recovery-specialized-operation-required",
            "recovery reservations and grant audits require specialized transactional operations",
        ));
    }
    Ok(())
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

    /// Read the current candidate through exactly one active matching reservation.
    /// Unlike claim replay, this revalidates live authority and the full candidate
    /// commitment. A retired/cancelled claim or historical grant is not permission.
    /// This read neither consumes the reservation nor verifies external safety.
    /// # Errors
    /// Rejects absent/foreign/retired claims, stale commitments and live blockers.
    fn claimed_recovery_candidate(
        &self,
        workspace: &str,
        claim: &Claim,
    ) -> Result<Candidate, RegistrationError>;

    /// Reserve the original binding after atomically revalidating the candidate.
    /// The retained resource fences ordinary grants and competing claims. An
    /// exact authorized retry returns its original result even after revocation;
    /// this replay is historical evidence, never permission to execute.
    /// # Errors
    /// Rejects changed candidates, competing operations/epochs and foreign scope.
    fn claim_recovery(&self, workspace: &str, claim: &Claim) -> Result<Claim, RegistrationError>;
}
