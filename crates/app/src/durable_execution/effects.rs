//! Interrupted execution observations never establish an external outcome.
use super::Launch;
use crate::RegistrationError;
use serde::{Deserialize, Serialize};

/// Protected runtime identities only. Command arguments and outputs stay in the
/// runtime store. Child conversations may own effects in the Assignment store.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterruptedEffect {
    pub task_id: String,
    pub conversation_id: String,
    pub call_id: String,
}

impl InterruptedEffect {
    /// # Errors
    /// Rejects unbounded or empty identities before retaining any observations.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        if [&self.task_id, &self.conversation_id, &self.call_id]
            .iter()
            .any(|id| id.trim().is_empty() || id.len() > 1024 || id.chars().any(char::is_control))
        {
            return Err(RegistrationError::new(
                "durable-effect-invalid",
                "interrupted effect requires bounded runtime identities",
            ));
        }
        Ok(())
    }
}

/// A committed interrupted intent is not a runtime receipt, even after an
/// external resolution audit. Until a specialized recovery policy can safely
/// consume it without tool/model replay, it must block execution admission.
/// # Errors
/// Rejects every nonempty observation; no current coding tool is replay-safe.
pub fn validate_execution_observation(
    effects: &[InterruptedEffect],
) -> Result<(), RegistrationError> {
    if !effects.is_empty() {
        return Err(RegistrationError::new(
            "durable-interrupted-effect-blocked",
            "committed interrupted effects block execution; inspect and resolve externally, never replay commands",
        ));
    }
    Ok(())
}

/// Specialized effect operations exclusively own their resource/audit writes.
/// # Errors
/// Rejects generic insertion of effect uncertainty or accepted resolution audits.
pub fn validate_resource_creation(kind: &str) -> Result<(), RegistrationError> {
    if matches!(kind, "interrupted-effect" | "interrupted-effect-resolution") {
        return Err(RegistrationError::new(
            "durable-effect-specialized-operation-required",
            "effect resources and audits require their specialized transactional operations",
        ));
    }
    Ok(())
}

/// External-effect uncertainty cannot be waived by provider resource cleanup.
/// # Errors
/// Interrupted effects require their own evidence-based audited resolution.
pub fn validate_resource_retirement(kind: &str) -> Result<(), RegistrationError> {
    if matches!(kind, "interrupted-effect" | "interrupted-effect-resolution") {
        return Err(RegistrationError::new(
            "durable-effect-resolution-required",
            "interrupted effects require explicit audited resolution, not resource retirement",
        ));
    }
    Ok(())
}

/// Trusted host observation, not an agent capability operation or a resolution.
/// The caller inspects committed runtime records without opening writable storage.
/// No observation authorizes replay, model requests, cleanup or worktree adoption.
pub trait Repository: Send + Sync {
    /// Atomically retain a bounded batch, exact runtime binding, recovery blocker
    /// and hold. Missing effects in later scans do NOT erase previous uncertainty.
    /// Exact retries preserve original timestamps, including after closure. Scope
    /// is authorized before replay; conflicting task identities reject the batch.
    /// # Errors
    /// Rejects foreign scope, mismatched/unadmitted runtimes, invalid or conflicting
    /// identities and persistence failures. Failure never establishes safety.
    fn retain_interrupted_effects(
        &self,
        workspace: &str,
        launch: &Launch,
        effects: &[InterruptedEffect],
    ) -> Result<(), RegistrationError>;
}
