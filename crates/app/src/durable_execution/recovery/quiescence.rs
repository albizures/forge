//! Claim-bound predecessor shutdown, not retirement, revocation or permission to
//! resume. Only a directly owned dedicated host can produce descendant proof.
use super::{Candidate, Claim};
use crate::{RegistrationError, authorization_lease::Lease};
use serde::{Deserialize, Serialize};

fn denied() -> RegistrationError {
    RegistrationError::new(
        "durable-recovery-quiescence-invalid",
        "recovery shutdown requires the exact reserved candidate and predecessor grant",
    )
}

/// Internal daemon-to-owned-host operation. Persist the recovery reservation
/// before delivery; a serialized value is not evidence that this has happened.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub claim: Claim,
    pub predecessor: Lease,
}

impl Operation {
    /// # Errors
    /// Rejects foreign commitments and incomplete original grant bindings.
    pub fn for_candidate(candidate: &Candidate, claim: Claim) -> Result<Self, RegistrationError> {
        if Claim::for_candidate(
            candidate,
            claim.operation_id.clone(),
            claim.daemon_epoch.clone(),
        )? != claim
            || candidate.last_lease.launch != candidate.launch
            || candidate.last_lease.generation != candidate.attempt.generation
        {
            return Err(denied());
        }
        let operation = Self {
            claim,
            predecessor: candidate.last_lease.clone(),
        };
        operation.validate()?;
        Ok(operation)
    }

    /// Defensive host boundary, not current Forge authorization. The host must
    /// additionally match its last accepted grant and directly owned process.
    /// # Errors
    /// Rejects malformed claims, changed binding/epoch or missing grant history.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        self.claim.validate()?;
        if self.predecessor.launch != self.claim.launch
            || self.predecessor.daemon_epoch == self.claim.daemon_epoch
            || self.predecessor.sequence == 0
        {
            return Err(denied());
        }
        // Shutdown is allowed after expiry; do not interpret old permission as
        // renewable authority. Validate only its original bounded representation.
        let mut original = self.predecessor.clone();
        original.sequence = 1;
        original.validate(None, original.issued_boottime_millis)
    }
}

/// Protected original-host evidence only. Never clears a claim, capacity, hold,
/// uncertain effects or authorization. Fresh exclusive inspection and a separate
/// Forge grant remain required before writable recovery.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u32,
    pub operation: Operation,
    pub children_absent: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_binding_is_exact_and_does_not_require_live_old_permission() {
        let control: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../runner/test/recovery-control.json"
        ))
        .unwrap();
        let mut op = Operation {
            claim: serde_json::from_value(control["grant"]["claim"].clone()).unwrap(),
            predecessor: serde_json::from_value(control["grant"]["lease"].clone()).unwrap(),
        };
        assert!(op.validate().is_err());
        op.predecessor.daemon_epoch = "predecessor".into();
        op.validate().unwrap();
        // Its old absolute deadline is not recomputed or extended by shutdown.
        assert_eq!(op.predecessor.expires_boottime_millis, 31000);
        for change in [
            |o: &mut Operation| o.predecessor.launch.attempt_id.push('x'),
            |o: &mut Operation| o.predecessor.sequence = 0,
            |o: &mut Operation| o.predecessor.expires_boottime_millis += 1,
            |o: &mut Operation| o.claim.candidate_sha256.clear(),
        ] {
            let mut invalid = op.clone();
            change(&mut invalid);
            assert!(invalid.validate().is_err());
        }
    }
}
