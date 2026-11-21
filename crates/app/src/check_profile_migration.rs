//! Explicit, auditable replacement of a Check's *effective* Agent Profile revision.
//!
//! This is the narrow exception to immutable Check pins. The original pin, the Check's
//! identity/description/order, every Assignment's bound profile and all Assessments
//! stay as history; a migration only adds an effective selection and invalidates work
//! freshness. A changed template or Workspace default never migrates an old Check.
use crate::RegistrationError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_OPERATION_ID_BYTES: usize = 128;
pub const MAX_SELECTIONS: usize = 64;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub check_id: u64,
    pub profile_revision: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub task_id: u64,
    /// Operator-chosen stable identity. Reuse the same value to retry safely.
    pub operation_id: String,
    /// Work generation the operator inspected; a stale value rejects the migration.
    pub expected_generation: u64,
    pub selections: Vec<Selection>,
    pub actor: String,
    pub message: String,
}

impl Request {
    /// # Errors
    /// Rejects malformed identities, empty/duplicate/oversized selections and blank attribution.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        let id_ok = !self.operation_id.is_empty()
            && self.operation_id.len() <= MAX_OPERATION_ID_BYTES
            && self
                .operation_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'));
        let mut ids: Vec<u64> = self.selections.iter().map(|s| s.check_id).collect();
        ids.sort_unstable();
        ids.dedup();
        let selections_ok = !self.selections.is_empty()
            && self.selections.len() <= MAX_SELECTIONS
            && ids.len() == self.selections.len()
            && self.selections.iter().all(|s| s.profile_revision > 0);
        if !id_ok
            || !selections_ok
            || self.actor.trim().is_empty()
            || self.message.trim().is_empty()
        {
            return Err(RegistrationError::new(
                "check-profile-migration-invalid",
                "use an operation ID of letters, digits, '-', '_', '.', ':' (max 128 bytes), 1 to 64 distinct Checks with positive revisions, and nonblank actor and message",
            ));
        }
        Ok(())
    }

    /// Stable identity of everything that decides the outcome; selection order is irrelevant.
    #[must_use]
    pub fn digest(&self) -> String {
        let mut selections = self.selections.clone();
        selections.sort_by_key(|s| s.check_id);
        let canonical = serde_json::json!([
            self.task_id,
            self.expected_generation,
            selections,
            self.actor,
            self.message
        ]);
        Sha256::digest(canonical.to_string().as_bytes())
            .iter()
            .fold(String::new(), |mut s, b| {
                use std::fmt::Write;
                let _ = write!(s, "{b:02x}");
                s
            })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Change {
    pub check_id: u64,
    /// Effective revision immediately before this migration.
    pub prior_revision: u32,
    pub new_revision: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Receipt {
    pub task_id: u64,
    pub operation_id: String,
    pub actor: String,
    pub message: String,
    pub generation_before: u64,
    /// Equal to `generation_before` only when the Task had no work to invalidate.
    pub generation_after: u64,
    pub changes: Vec<Change>,
    pub created_at_millis: u64,
    /// True when an exact retry returned the original result.
    pub replayed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(selections: Vec<(u64, u32)>) -> Request {
        Request {
            task_id: 1,
            operation_id: "m-1".into(),
            expected_generation: 1,
            selections: selections
                .into_iter()
                .map(|(check_id, profile_revision)| Selection {
                    check_id,
                    profile_revision,
                })
                .collect(),
            actor: "operator".into(),
            message: "adopt durable".into(),
        }
    }
    #[test]
    fn validation_rejects_unsafe_requests() {
        request(vec![(1, 2)]).validate().unwrap();
        for bad in [
            request(vec![]),
            request(vec![(1, 2), (1, 3)]),
            request(vec![(1, 0)]),
            Request {
                operation_id: "a b".into(),
                ..request(vec![(1, 2)])
            },
            Request {
                actor: " ".into(),
                ..request(vec![(1, 2)])
            },
        ] {
            assert_eq!(
                bad.validate().unwrap_err().code,
                "check-profile-migration-invalid"
            );
        }
    }
    #[test]
    fn digest_ignores_order_but_not_content() {
        assert_eq!(
            request(vec![(1, 2), (2, 3)]).digest(),
            request(vec![(2, 3), (1, 2)]).digest()
        );
        assert_ne!(
            request(vec![(1, 2)]).digest(),
            request(vec![(1, 3)]).digest()
        );
    }
    #[test]
    fn unknown_fields_are_rejected() {
        let mut value = serde_json::to_value(request(vec![(1, 2)])).unwrap();
        value["template_revision"] = 1.into();
        assert!(serde_json::from_value::<Request>(value).is_err());
    }
}
