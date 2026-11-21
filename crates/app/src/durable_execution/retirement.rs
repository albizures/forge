//! Revoked durable execution reconciliation. Receipts are trusted host evidence,
//! never agent payloads or permission to replay interrupted external effects.
use super::Launch;
use crate::{RegistrationError, authorization_lease::Owner};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub operation_id: String,
    pub launch: Launch,
    pub owner: Owner,
}

impl Operation {
    /// # Errors
    /// Rejects incomplete immutable runtime or live-owner coordinates.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        self.launch.validate()?;
        if self.operation_id.trim().is_empty()
            || self.owner.host_pid == 0
            || self.owner.worker_pid == 0
            || [
                &self.owner.boot_id,
                &self.owner.host_start,
                &self.owner.worker_start,
                &self.owner.time_namespace,
                &self.owner.lock_device,
                &self.owner.lock_inode,
            ]
            .iter()
            .any(|v| v.trim().is_empty())
        {
            return Err(RegistrationError::new(
                "durable-retirement-invalid",
                "retirement requires immutable operation and owner identity",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u32,
    pub operation: Operation,
    pub children_absent: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Retirement {
    pub operation: Operation,
    pub reconciled_at_millis: Option<u64>,
}

/// Every operation authorizes retained Workspace/binding and revoked authority
/// before replay. External host calls stay outside these transactions.
pub trait Repository: Send + Sync {
    /// Read the last original grant only after revocation, for exact protected
    /// crash-proof reconciliation. This historical value grants no execution.
    /// # Errors
    /// Rejects foreign scope/runtime, live authority and malformed grant history.
    fn revoked_lease(
        &self,
        workspace: &str,
        launch: &Launch,
    ) -> Result<Option<crate::authorization_lease::Lease>, RegistrationError>;

    /// # Errors
    /// Rejects wrong scope, conflicting identity, live authority or unknown launch.
    fn begin_retirement(
        &self,
        workspace: &str,
        operation: &Operation,
    ) -> Result<Retirement, RegistrationError>;
    /// # Errors
    /// Rejects wrong scope/binding or unreadable storage; performs no host I/O.
    fn retirement(
        &self,
        workspace: &str,
        launch: &Launch,
    ) -> Result<Option<Retirement>, RegistrationError>;
    /// The caller holds exclusive host inspection through commit and verifies the
    /// protected receipt against the original operation. Effects remain blockers.
    /// # Errors
    /// Rejects invalid receipt, live authority, conflicting operation or storage failure.
    fn reconcile_retirement(
        &self,
        workspace: &str,
        receipt: &Receipt,
    ) -> Result<Retirement, RegistrationError>;
}

/// Liveness proof discharges only these execution resources. External effects,
/// unknown diagnostics and unrelated resource identities remain blockers.
#[must_use]
pub fn reconciles_resource(launch: &Launch, kind: &str, id: &str) -> bool {
    use crate::authorization_lease::session::Failure;
    match kind {
        "durable-runtime" => id == launch.runtime_id,
        "missing-outcome" => id == launch.attempt_id,
        "authorization-uncertain" => [
            Failure::AuthorityUnavailable,
            Failure::LeaseExpired,
            Failure::DeliveryUncertain,
            Failure::Stopped,
        ]
        .iter()
        .any(|failure| failure.code() == id),
        _ => false,
    }
}

/// # Errors
/// Retirement cannot reinterpret an unresolved launch or revive live authority.
pub fn validate_begin(
    revoked: bool,
    admission: &super::Admission,
) -> Result<(), RegistrationError> {
    if !revoked {
        return Err(RegistrationError::new(
            "assignment-not-revoked",
            "retirement requires revoked authority",
        ));
    }
    if !matches!(
        admission,
        super::Admission::Accepted { .. } | super::Admission::Rejected { .. }
    ) {
        return Err(RegistrationError::new(
            "durable-retirement-admission-unknown",
            "reconcile the original launch before retirement",
        ));
    }
    Ok(())
}

/// # Errors
/// A newly spawned lookalike cannot retire the original authorized execution.
/// Missing original grant ownership is uncertainty, not permission to adopt.
pub fn validate_owner(
    recorded: Option<&Owner>,
    requested: &Owner,
) -> Result<(), RegistrationError> {
    if recorded != Some(requested) {
        return Err(RegistrationError::new(
            "durable-retirement-owner-unverified",
            "retirement must match the originally authorized host ownership",
        ));
    }
    Ok(())
}

/// # Errors
/// Rejects unsupported or incomplete cleanup evidence.
pub fn validate_receipt(receipt: &Receipt) -> Result<(), RegistrationError> {
    if receipt.version != 1
        || !receipt.children_absent
        || receipt.operation.operation_id.trim().is_empty()
    {
        return Err(RegistrationError::new(
            "durable-retirement-unverified",
            "owned execution cleanup is not verified",
        ));
    }
    receipt.operation.validate()
}
