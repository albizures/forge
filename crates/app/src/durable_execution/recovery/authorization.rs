//! Fresh recovery permission follows external safety verification and a separate
//! transactional commit. This is not renewal or a runnable recovery controller.
use super::{Candidate, Claim};
use crate::{
    RegistrationError,
    authorization_lease::{LEASE_MILLIS, Lease, Observation},
};
use serde::{Deserialize, Serialize};

fn denied() -> RegistrationError {
    RegistrationError::new(
        "durable-recovery-authorization-denied",
        "recovery requires verified safety and fresh original-binding authorization",
    )
}

/// Trusted host boundary, not an agent/operator payload. Implementations must
/// prove the predecessor's descendant execution safe, hold exclusive canonical
/// ownership through commit/delivery, inspect the exact pending original runtime
/// admission and all committed effects, and validate applicable repository
/// freshness/retained-work policy without adopting or repairing dirty progress.
/// Before returning, persist protected evidence keyed by the claim operation and
/// its original binding; its digest becomes part of the immutable grant audit.
/// Missing proof, unavailable components and unresolved effects are errors.
/// Observation identifies a newly spawned waiting owner in its current boot/time
/// namespace. Neither a free flock nor an old PID/lease proves predecessor safety.
pub trait RecoveryHost {
    /// Perform external verification outside any database transaction.
    /// # Errors
    /// Rejects any uncertainty; retained diagnostics/fences must not be cleared.
    fn verify(
        &mut self,
        candidate: &Candidate,
        claim: &Claim,
    ) -> Result<Verification, RegistrationError>;
}

/// Trusted adapter result, not a serialized authority payload. The digest links
/// protected, retained external proof; it is not itself proof of process absence.
pub struct Verification {
    pub observation: Observation,
    pub evidence_sha256: String,
}

/// Bounded audit identity; no Task body, prompt, capability or handoff contents.
/// A retained grant is historical evidence, not restart permission.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub claim: Claim,
    pub lease: Lease,
    pub evidence_sha256: String,
}

/// Can only be constructed after the trusted external verification boundary.
/// Not serializable/deserializable: retained audits cannot reconstruct authority.
/// Dropping this value neither releases the claim nor proves process cleanup.
pub struct Prepared {
    candidate: Candidate,
    grant: Grant,
}

impl Prepared {
    /// Snapshot original Forge state, verify external preconditions, then propose
    /// a fresh bounded lease. This alone commits/delivers nothing. The caller must
    /// keep verified exclusive ownership through the repository commit/delivery.
    /// # Errors
    /// Rejects foreign candidate commitments, old epochs/owners and invalid clocks.
    pub fn verify(
        candidate: Candidate,
        claim: Claim,
        host: &mut dyn RecoveryHost,
    ) -> Result<Self, RegistrationError> {
        if Claim::for_candidate(
            &candidate,
            claim.operation_id.clone(),
            claim.daemon_epoch.clone(),
        )? != claim
        {
            return Err(denied());
        }
        if candidate.last_lease.sequence == 0
            || candidate.last_lease.launch != candidate.launch
            || candidate.last_lease.generation != candidate.attempt.generation
            || candidate.last_lease.daemon_epoch == claim.daemon_epoch
        {
            return Err(denied());
        }
        let sequence = candidate
            .last_lease
            .sequence
            .checked_add(1)
            .ok_or_else(denied)?;
        let mut predecessor = candidate.last_lease.clone();
        predecessor.sequence = 1;
        predecessor.validate(None, predecessor.issued_boottime_millis)?;
        let verification = host.verify(&candidate, &claim)?;
        let observation = verification.observation;
        let now = observation.boottime_millis;
        let lease = Lease {
            launch: candidate.launch.clone(),
            generation: candidate.attempt.generation,
            owner: observation.owner,
            daemon_epoch: claim.daemon_epoch.clone(),
            sequence,
            issued_boottime_millis: now,
            expires_boottime_millis: now.checked_add(LEASE_MILLIS).ok_or_else(denied)?,
        };
        let prepared = Self {
            candidate,
            grant: Grant {
                claim,
                lease,
                evidence_sha256: verification.evidence_sha256,
            },
        };
        prepared.validate(now)?;
        Ok(prepared)
    }

    #[must_use]
    pub fn candidate(&self) -> &Candidate {
        &self.candidate
    }

    #[must_use]
    pub fn grant(&self) -> &Grant {
        &self.grant
    }

    /// Validate a new owner/epoch transition without treating an expired old
    /// grant as renewable. Sequence is Assignment-wide immutable history; a fresh
    /// consumer will require a specialized recovery handshake, not ordinary
    /// sequence-one admission or replay of a retained grant.
    /// # Errors
    /// Rejects changed bindings, reused owners/epochs and stale proposed grants.
    pub fn validate(&self, now: u64) -> Result<(), RegistrationError> {
        let old = &self.candidate.last_lease;
        let lease = &self.grant.lease;
        let claim = &self.grant.claim;
        let digest = &self.grant.evidence_sha256;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(denied());
        }
        if Claim::for_candidate(
            &self.candidate,
            claim.operation_id.clone(),
            claim.daemon_epoch.clone(),
        )? != *claim
            || old.launch != self.candidate.launch
            || old.generation != self.candidate.attempt.generation
            || lease.launch != old.launch
            || lease.generation != old.generation
            || lease.daemon_epoch != claim.daemon_epoch
            || lease.daemon_epoch == old.daemon_epoch
            || old.sequence.checked_add(1) != Some(lease.sequence)
            || (lease.owner.boot_id == old.owner.boot_id
                && lease.owner.time_namespace == old.owner.time_namespace
                && lease.issued_boottime_millis < old.issued_boottime_millis)
            || (lease.owner.boot_id == old.owner.boot_id
                && (lease.owner.host_pid == old.owner.host_pid
                    && lease.owner.host_start == old.owner.host_start
                    || lease.owner.worker_pid == old.owner.worker_pid
                        && lease.owner.worker_start == old.owner.worker_start))
        {
            return Err(denied());
        }
        // Validate fresh bounded authority independently of the old deadline.
        // No old CLOCK_BOOTTIME value is compared across a changed boot.
        let mut fresh = lease.clone();
        fresh.sequence = 1;
        fresh.validate(None, now)
    }
}

/// Trusted daemon persistence boundary. No external calls inside its transaction.
/// This is not an Assignment capability action or an operator API.
pub trait Repository: Send + Sync {
    /// Atomically revalidate the claimed candidate and commit its first fresh
    /// recovery lease, an immutable audit, and claim retirement together. Exact
    /// retries are deliverable only while live current authority still permits
    /// the identical grant; historical audits never authorize a later restart.
    /// # Errors
    /// Rejects stale/closed/blocked bindings, conflicting retries, cancelled claims
    /// and expired grants. A failed return must never be delivered as permission.
    fn grant_recovery(
        &self,
        workspace: &str,
        prepared: &Prepared,
        now: u64,
    ) -> Result<Grant, RegistrationError>;
}
