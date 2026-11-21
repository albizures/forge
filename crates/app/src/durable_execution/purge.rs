//! Explicit deletion of retired runtime history. Forge's audit (Assignment, Outcomes,
//! Assessments, migration/recovery facts, retirement) is never deleted, and a purge
//! never creates, restarts or replays execution.
use super::Launch;
use crate::RegistrationError;
use serde::{Deserialize, Serialize};

pub const MAX_OPERATION_ID_BYTES: usize = 128;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub assignment_id: String,
    /// Operator-chosen stable identity. Reuse the same value to retry safely.
    pub operation_id: String,
}

impl Request {
    /// # Errors
    /// Rejects empty or unsafe identities before any I/O.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        let ok = !self.operation_id.is_empty()
            && self.operation_id.len() <= MAX_OPERATION_ID_BYTES
            && self
                .operation_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'));
        if self.assignment_id.trim().is_empty() || !ok {
            return Err(RegistrationError::new(
                "runtime-purge-request-invalid",
                "use an Assignment ID and an operation ID of letters, digits, '-', '_', '.', ':' (max 128 bytes)",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// Deletion was authorized and may be partial. Retry with the same operation ID.
    Pending,
    /// Every runtime file and the directory are gone, or were already absent.
    Purged,
    /// Deletion stopped; the diagnostic says why. Remaining storage is retained.
    Retained,
}

impl State {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Purged => "purged",
            Self::Retained => "retained",
        }
    }

    /// # Errors
    /// Unknown stored values are invalid storage, not a default.
    pub fn parse(value: &str) -> Result<Self, RegistrationError> {
        match value {
            "pending" => Ok(Self::Pending),
            "purged" => Ok(Self::Purged),
            "retained" => Ok(Self::Retained),
            _ => Err(RegistrationError::new(
                "durable-storage-invalid",
                "unknown runtime purge state",
            )),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Record {
    pub assignment_id: String,
    pub operation_id: String,
    pub state: State,
    /// Fixed non-secret reason code when deletion stopped.
    pub diagnostic: Option<String>,
    pub requested_at_millis: u64,
    pub completed_at_millis: Option<u64>,
}

/// Result of the external filesystem step, reported back to Forge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Finish {
    Purged,
    Retained { code: String },
}

pub trait Repository: Send + Sync {
    /// Authorize and journal intent in one transaction. An exact retry returns the
    /// original record (even after later changes); a different operation conflicts.
    /// New intent requires a closed Assignment, reconciled retirement (or a rejected
    /// admission), no unretired resources and no Dispatch Hold. No filesystem I/O.
    /// # Errors
    /// Rejects foreign scope or binding, conflicts and ineligible Assignments.
    fn begin_purge(
        &self,
        workspace: &str,
        launch: &Launch,
        operation_id: &str,
    ) -> Result<Record, RegistrationError>;
    /// Record the filesystem result for the journaled operation.
    /// # Errors
    /// Rejects an unknown or different operation and a finished purge's change.
    fn finish_purge(
        &self,
        workspace: &str,
        launch: &Launch,
        operation_id: &str,
        finish: &Finish,
    ) -> Result<Record, RegistrationError>;
    /// # Errors
    /// Rejects foreign scope or invalid retained rows.
    fn purge_record(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<Option<Record>, RegistrationError>;
}
