//! Read-only internal subprocess contract. Not an operator method, a recovery
//! handshake, process-safety proof or execution authority.
use forge_app::{RegistrationError, durable_execution::Launch};
use serde::{Deserialize, Serialize};

pub const MAX_REQUEST_BYTES: usize = 16384;
pub const MAX_RESPONSE_BYTES: u64 = 32768;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u32,
    pub launch: Launch,
    pub admission_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PendingStatus {
    Queued,
    Placed,
}

/// Upstream identifiers and pending state only, never input or tool contents.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingInput {
    pub root: u64,
    pub admission_id: String,
    pub status: PendingStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub protocol: u32,
    pub launch: Launch,
    pub candidate: PendingInput,
}
impl Response {
    /// Validate identity/version and the pinned runtime's positive JS-safe ID.
    /// This is wire validation, not evidence that executing recovery is safe.
    /// # Errors
    /// Rejects a foreign binding, admission, version or unrepresentable root ID.
    pub fn validate(&self, launch: &Launch, admission_id: &str) -> Result<(), RegistrationError> {
        if self.protocol != super::RUNNER_PROTOCOL_VERSION
            || self.launch != *launch
            || self.candidate.admission_id != admission_id
            || self.candidate.admission_id.trim().is_empty()
            || self.candidate.root == 0
            || self.candidate.root > 9_007_199_254_740_991
        {
            return Err(RegistrationError::new(
                "runner-recovery-inspection-invalid",
                "expected exact original pending-input identity and compatible recovery inspection",
            ));
        }
        Ok(())
    }
}
