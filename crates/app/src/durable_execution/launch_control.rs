//! Rollout/rollback policy for NEW durable launches only.
//!
//! Default eligibility is withheld until retained release verification and handoff exist,
//! so this crate exposes no operation that grants it. The only persisted operator action is
//! a sticky rollback: once recorded, new durable admissions are refused across restarts.
//! It never moves an existing Assignment to another backend, rewrites Check pins or
//! disables inspection, recovery or retirement of work already admitted.
use crate::RegistrationError;
use serde::{Deserialize, Serialize};

pub const MAX_OPERATION_ID_BYTES: usize = 128;
pub const MAX_REASON_BYTES: usize = 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DisableRequest {
    /// Operator-chosen stable identity. Reuse the same value to retry safely.
    pub operation_id: String,
    /// Why new durable launches are being stopped; retained as audit.
    pub reason: String,
}

impl DisableRequest {
    /// # Errors
    /// Rejects empty or unsafe identities and blank/oversized reasons before any I/O.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        let id = !self.operation_id.is_empty()
            && self.operation_id.len() <= MAX_OPERATION_ID_BYTES
            && self
                .operation_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'));
        let reason = !self.reason.trim().is_empty() && self.reason.len() <= MAX_REASON_BYTES;
        if !id || !reason {
            return Err(RegistrationError::new(
                "durable-launch-request-invalid",
                "use an operation ID of letters, digits, '-', '_', '.', ':' (max 128 bytes) and a nonblank reason (max 1024 bytes)",
            ));
        }
        Ok(())
    }
}

/// The one retained rollback. Later requests never replace it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rollback {
    pub operation_id: String,
    pub reason: String,
    pub disabled_at_millis: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// Release verification evidence is not accepted; new durable launches are refused.
    Withheld,
    /// Explicit controlled eligibility granted only by a verification composition.
    Controlled,
    /// Rolled back: new durable launches are refused, permanently for this store.
    Disabled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub state: State,
    /// Whether this daemon admits new durable launches. Always false for production
    /// composition until the release handoff; reported, never inferred from a demo.
    pub new_launches_admitted: bool,
    pub rollback: Option<Rollback>,
    /// True when the request matched an earlier rollback (same or different operation).
    #[serde(default)]
    pub replayed: bool,
    pub notes: Vec<String>,
}

impl Status {
    #[must_use]
    pub fn compose(rollback: Option<Rollback>, admitted_in_process: bool) -> Self {
        let state = match (&rollback, admitted_in_process) {
            (Some(_), _) => State::Disabled,
            (None, true) => State::Controlled,
            (None, false) => State::Withheld,
        };
        Self {
            state,
            new_launches_admitted: rollback.is_none() && admitted_in_process,
            rollback,
            replayed: false,
            notes: vec![
                "Applies to new durable launches only. Existing durable Assignments keep their original backend for status, transcripts, recovery and retirement; nothing is moved to Herdr.".into(),
                "Herdr Tasks and Grilling are unaffected. Check pins and Workspace selections are not rewritten.".into(),
                "Rollback is sticky for this store; default eligibility is withheld pending release verification and handoff.".into(),
            ],
        }
    }
}

/// Persistence for the single sticky rollback.
pub trait Repository {
    /// Record the rollback, or return the existing one unchanged (`replayed = true`).
    ///
    /// # Errors
    /// Returns storage failures; never partially records a rollback.
    fn disable_durable_launches(
        &self,
        request: &DisableRequest,
    ) -> Result<(Rollback, bool), RegistrationError>;

    /// # Errors
    /// Returns storage failures. Callers deciding admission must fail closed.
    fn durable_launch_rollback(&self) -> Result<Option<Rollback>, RegistrationError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_validate_identity_and_reason_before_io() {
        let ok = DisableRequest {
            operation_id: "rollback:1".into(),
            reason: "incident".into(),
        };
        assert!(ok.validate().is_ok());
        for (operation_id, reason) in [("", "r"), ("bad id", "r"), ("ok", " "), ("ok", "")] {
            let request = DisableRequest {
                operation_id: operation_id.into(),
                reason: reason.into(),
            };
            assert_eq!(
                request.validate().unwrap_err().code,
                "durable-launch-request-invalid"
            );
        }
        let long = DisableRequest {
            operation_id: "x".repeat(MAX_OPERATION_ID_BYTES + 1),
            reason: "r".into(),
        };
        assert!(long.validate().is_err());
    }

    #[test]
    fn status_never_admits_after_rollback() {
        let rollback = Rollback {
            operation_id: "op".into(),
            reason: "r".into(),
            disabled_at_millis: 1,
        };
        let status = Status::compose(Some(rollback), true);
        assert_eq!(status.state, State::Disabled);
        assert!(!status.new_launches_admitted);
        let withheld = Status::compose(None, false);
        assert_eq!(withheld.state, State::Withheld);
        assert_eq!(Status::compose(None, true).state, State::Controlled);
        assert!(!withheld.new_launches_admitted);
    }
}
