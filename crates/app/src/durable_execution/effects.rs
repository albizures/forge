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

/// External-effect uncertainty cannot be waived by provider resource cleanup.
/// # Errors
/// Interrupted effects require their own evidence-based audited resolution.
pub fn validate_resource_retirement(kind: &str) -> Result<(), RegistrationError> {
    if kind == "interrupted-effect" {
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
