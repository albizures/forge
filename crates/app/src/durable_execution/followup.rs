//! Operator follow-ups: durable, idempotent records bound to one Assignment attempt.
//! A record is input intent only. It is never an Outcome, lease, recovery permission,
//! profile/Check/evidence change or proof that the runtime received or acted on it.
use crate::RegistrationError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;
pub const MAX_OPERATION_ID_BYTES: usize = 128;

/// Only identity and text. Unknown fields (profile, Check, evidence, commit,
/// generation) are rejected by deserialization, so a request cannot rebind work.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub assignment_id: String,
    /// Operator-chosen stable identity. Reuse the same value to retry safely.
    pub operation_id: String,
    pub message: String,
}

impl Request {
    /// # Errors
    /// Rejects empty/oversized/unsafe identities and messages before any I/O.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        let id_ok = !self.operation_id.is_empty()
            && self.operation_id.len() <= MAX_OPERATION_ID_BYTES
            && self
                .operation_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'));
        let message_ok = !self.message.trim().is_empty()
            && self.message.len() <= MAX_MESSAGE_BYTES
            && !self.message.contains('\0');
        if self.assignment_id.trim().is_empty() || !id_ok || !message_ok {
            return Err(RegistrationError::new(
                "followup-request-invalid",
                "use an Assignment ID, an operation ID of letters, digits, '-', '_', '.', ':' (max 128 bytes) and a non-empty message of at most 16 KiB",
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn digest(&self) -> String {
        Sha256::digest(self.message.as_bytes())
            .iter()
            .fold(String::new(), |mut s, b| {
                use std::fmt::Write;
                let _ = write!(s, "{b:02x}");
                s
            })
    }
}

/// Immutable binding captured when the follow-up was recorded.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub attempt_id: String,
    pub check_id: u64,
    pub generation: u64,
    pub profile_revision: u32,
    pub runtime_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Receipt {
    pub assignment_id: String,
    pub operation_id: String,
    /// Per-Assignment order of first recording, starting at 1.
    pub sequence: u32,
    pub message_sha256: String,
    pub binding: Binding,
    pub recorded_at_millis: u64,
    /// True when an exact retry returned the original record without a new one.
    pub replayed: bool,
}

pub trait Repository: Send + Sync {
    /// Record in one transaction. An exact retry returns the original receipt
    /// (even after the Assignment later closed); reuse with different content
    /// conflicts. New input requires an open, accepted, fresh-bound Assignment
    /// with no retained interrupted-effect/authorization/recovery/retirement/
    /// provisioning uncertainty. Missing-Outcome Assignments are allowed.
    /// # Errors
    /// Rejects unauthorized scope, conflicts, revoked/stale/blocked targets and storage failures.
    fn record_followup(
        &self,
        workspace: &str,
        request: &Request,
    ) -> Result<Receipt, RegistrationError>;
    /// Recorded follow-ups in first-recorded order, after authorizing scope.
    /// # Errors
    /// Rejects unauthorized scope and invalid retained rows.
    fn followups(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<Vec<Receipt>, RegistrationError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(op: &str, msg: &str) -> Request {
        Request {
            assignment_id: "a".into(),
            operation_id: op.into(),
            message: msg.into(),
        }
    }
    #[test]
    fn validation_bounds_identity_and_message() {
        request("op-1", "use another commit").validate().unwrap();
        for bad in [
            request("", "m"),
            request("a b", "m"),
            request("a/b", "m"),
            request(&"x".repeat(129), "m"),
            request("op", " "),
            request("op", "a\0b"),
            request("op", &"x".repeat(MAX_MESSAGE_BYTES + 1)),
        ] {
            assert_eq!(bad.validate().unwrap_err().code, "followup-request-invalid");
        }
    }
    #[test]
    fn unknown_binding_fields_are_rejected_not_ignored() {
        for field in [
            "profile_revision",
            "check_id",
            "commit",
            "evidence",
            "generation",
        ] {
            let mut value =
                serde_json::json!({"assignment_id":"a","operation_id":"o","message":"m"});
            value[field] = 1.into();
            assert!(serde_json::from_value::<Request>(value).is_err(), "{field}");
        }
    }
    #[test]
    fn digest_is_stable_content_identity() {
        assert_eq!(request("a", "x").digest(), request("b", "x").digest());
        assert_ne!(request("a", "x").digest(), request("a", "y").digest());
        assert_eq!(request("a", "x").digest().len(), 64);
    }
}
