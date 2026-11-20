//! Evidence-based operator assertions, not runtime receipts or replay authority.
use super::Launch;
use super::effects::InterruptedEffect;
use crate::RegistrationError;
use serde::{Deserialize, Serialize};

/// Only supported external conclusions. Unknown outcomes cannot be resolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExternalOutcome {
    Applied,
    NotApplied,
}

/// Trusted operator attribution and external evidence references. Never include
/// command arguments/results, credentials or prompts in these ordinary audit fields.
/// Confirmations are explicit safety assertions, not authenticated human identity
/// or automatic proof that escaped processes stopped.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub operation_id: String,
    pub effect: InterruptedEffect,
    pub outcome: ExternalOutcome,
    pub actor: String,
    pub evidence: String,
    pub message: String,
    pub confirm_old_agent_cannot_act: bool,
    pub confirm_external_work_safe: bool,
}

impl Resolution {
    /// # Errors
    /// Rejects unsupported, missing, excessive or unconfirmed evidence. Unknown
    /// external outcomes must remain retained rather than be labelled not-applied.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        self.effect.validate()?;
        if !self.confirm_old_agent_cannot_act
            || !self.confirm_external_work_safe
            || [
                (&self.operation_id, 128),
                (&self.actor, 256),
                (&self.evidence, 4096),
                (&self.message, 4096),
            ]
            .iter()
            .any(|(text, limit)| {
                text.trim().is_empty() || text.len() > *limit || text.chars().any(char::is_control)
            })
        {
            return Err(RegistrationError::new(
                "durable-effect-resolution-invalid",
                "effect resolution requires bounded attribution, evidence and both safety confirmations",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionReceipt {
    pub resolution: Resolution,
    pub accepted_at_millis: u64,
}

/// Trusted operator service only; Assignment capabilities cannot invoke this port.
/// For new acceptance the host adapter must retain verified exclusive ownership
/// throughout the dependent commit; original accepted-result reads admit nothing. This conservative first boundary requires revocation;
/// it does not enable same-Assignment recovery or modify protected runtime receipts.
pub trait Repository: Send + Sync {
    /// Read the originally accepted result, authorizing scope/Launch first.
    /// Does not require current runtime availability or host ownership: it admits
    /// no new resolution, execution, resource retirement or safety assertion.
    /// # Errors
    /// Rejects foreign scope/binding, conflicting operation/effect reuse and invalid audits.
    fn accepted_effect_resolution(
        &self,
        workspace: &str,
        launch: &Launch,
        resolution: &Resolution,
    ) -> Result<Option<ResolutionReceipt>, RegistrationError>;

    /// Authorize exact Workspace/Launch before replay. Exact retries return the
    /// original audit; conflicting operation/effect reuse rejects. New acceptance
    /// requires a closed Assignment and retained effect. Audit and targeted effect
    /// retirement commit atomically. Holds, other resources, accepted Outcomes,
    /// repository evidence and runtime storage remain untouched. No command runs.
    /// # Errors
    /// Rejects invalid scope/binding, unsafe live authority, missing/conflicting
    /// effects or audits and storage errors. Failure preserves uncertainty.
    fn resolve_interrupted_effect(
        &self,
        workspace: &str,
        launch: &Launch,
        resolution: &Resolution,
    ) -> Result<ResolutionReceipt, RegistrationError>;
}
