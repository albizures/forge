//! Read-only operator inspection contracts. Never part of the Assignment capability
//! API and never an execution, lease, scheduler or recovery operation.
use crate::AgentAssignmentInfoResponse;
pub use forge_app::durable_execution::followup::{
    Receipt as FollowUpReceipt, Request as FollowUpRequest,
};
use forge_app::durable_execution::{
    Admission,
    status::{LeaseState, Status, View, view},
};
use serde::{Deserialize, Serialize};

pub const DEFAULT_TRANSCRIPT_LIMIT: u32 = 50;
pub const MAX_TRANSCRIPT_LIMIT: u32 = 100;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentTranscriptRequest {
    pub assignment_id: String,
    /// Exclusive committed entry ID already held by the caller; omit for the start.
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

impl AssignmentTranscriptRequest {
    /// # Errors
    /// Rejects malformed cursors and out-of-range limits before any I/O.
    pub fn validate(&self) -> Result<(), forge_app::RegistrationError> {
        let cursor_ok = self.after.as_deref().is_none_or(|after| {
            (1..=16).contains(&after.len())
                && !after.starts_with('0')
                && after.bytes().all(|b| b.is_ascii_digit())
        });
        let limit_ok = self
            .limit
            .is_none_or(|l| (1..=MAX_TRANSCRIPT_LIMIT).contains(&l));
        if self.assignment_id.trim().is_empty() || !cursor_ok || !limit_ok {
            return Err(forge_app::RegistrationError::new(
                "transcript-request-invalid",
                "use a committed entry ID cursor from a previous page and a limit from 1 to 100",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DurableRuntimeStatus {
    pub backend: String,
    pub runtime_id: String,
    pub operation_id: String,
    pub attempt_id: String,
    pub incarnation_id: String,
    pub profile_revision: u32,
    pub forge_revision: String,
    pub component_hash: String,
    /// pending, unknown, accepted or rejected
    pub admission: String,
    pub rejection_code: Option<String>,
    /// never-granted, granted-unverified or revoked
    pub lease_state: String,
    pub lease_count: u32,
    pub latest_lease_sequence: Option<u32>,
    pub latest_lease_generation: Option<u64>,
    /// Boot-clock values from the issuing host; informational, not expiry/liveness.
    pub latest_lease_issued_boottime_millis: Option<u64>,
    pub latest_lease_expires_boottime_millis: Option<u64>,
}

/// Persisted facts only; no store path, owner process, profile source or secret.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AssignmentDurableStatusResponse {
    pub assignment: AgentAssignmentInfoResponse,
    /// Absent when Forge retains no durable runtime for this Assignment.
    pub runtime: Option<DurableRuntimeStatus>,
    pub missing_outcome: bool,
    pub uncertainty: Vec<String>,
    pub retirement_blockers: Vec<String>,
    /// Recorded operator follow-ups in first-recorded order. A record is input
    /// intent bound to the original attempt, not proof of runtime delivery.
    #[serde(default)]
    pub followups: Vec<forge_app::durable_execution::followup::Receipt>,
    pub notes: Vec<String>,
}

impl AssignmentDurableStatusResponse {
    #[must_use]
    pub fn with_followups(
        mut self,
        followups: Vec<forge_app::durable_execution::followup::Receipt>,
    ) -> Self {
        self.followups = followups;
        self
    }

    #[must_use]
    pub fn compose(info: forge_app::AgentAssignmentInfo, status: Option<Status>) -> Self {
        let View {
            lease_state,
            missing_outcome,
            uncertainty,
            retirement_blockers,
        } = view(&info, status.as_ref());
        let runtime = status.map(|s| {
            let lease = s.latest_lease.as_ref();
            DurableRuntimeStatus {
                backend: "pi-durable".into(),
                runtime_id: s.runtime_id,
                operation_id: s.operation_id,
                attempt_id: s.attempt_id,
                incarnation_id: s.incarnation_id,
                profile_revision: s.profile_revision,
                forge_revision: s.forge_revision,
                component_hash: s.component_hash,
                admission: match &s.admission {
                    Admission::Pending => "pending",
                    Admission::Unknown => "unknown",
                    Admission::Accepted { .. } => "accepted",
                    Admission::Rejected { .. } => "rejected",
                }
                .into(),
                rejection_code: match s.admission {
                    Admission::Rejected { code } => Some(code),
                    _ => None,
                },
                lease_state: match lease_state {
                    LeaseState::NeverGranted => "never-granted",
                    LeaseState::GrantedUnverified => "granted-unverified",
                    LeaseState::Revoked => "revoked",
                }
                .into(),
                lease_count: s.lease_count,
                latest_lease_sequence: lease.map(|l| l.sequence),
                latest_lease_generation: lease.map(|l| l.generation),
                latest_lease_issued_boottime_millis: lease.map(|l| l.issued_boottime_millis),
                latest_lease_expires_boottime_millis: lease.map(|l| l.expires_boottime_millis),
            }
        });
        Self {
            assignment: info.into(),
            runtime,
            missing_outcome,
            uncertainty: uncertainty.into_iter().map(String::from).collect(),
            retirement_blockers,
            followups: Vec::new(),
            notes: vec![
                "Persisted Forge facts only; this does not contact a runner or prove a process is running or absent.".into(),
                "A missing Outcome or retained uncertainty keeps capacity reserved; inspection never recovers, retries or releases it.".into(),
            ],
        }
    }
}

/// `unavailable` and `not-started` are explicit; neither is an empty successful transcript.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranscriptHistory {
    Available,
    NotStarted,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TranscriptEntry {
    /// Immutable committed runtime entry ID, ascending across pages.
    pub id: String,
    pub kind: String,
    pub by_task_id: Option<String>,
    /// Bounded JSON of the entry's model messages and data. Tool payloads can
    /// contain operator-sensitive output; Forge never adds capabilities/credentials.
    pub content: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AssignmentTranscriptResponse {
    pub assignment_id: String,
    pub history: TranscriptHistory,
    /// Fixed non-secret reason code when history is unavailable.
    pub reason: Option<String>,
    pub entries: Vec<TranscriptEntry>,
    /// Pass as `after` to continue. Equals the request cursor when no entry was returned.
    pub next_after: Option<String>,
    /// More committed entries existed beyond this page when it was read. False is not
    /// end-of-history for a live Assignment: poll `next_after` to observe later appends.
    pub more: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(after: Option<&str>, limit: Option<u32>) -> AssignmentTranscriptRequest {
        AssignmentTranscriptRequest {
            assignment_id: "a".into(),
            after: after.map(Into::into),
            limit,
        }
    }
    #[test]
    fn transcript_request_validates_cursor_and_limit_before_any_io() {
        for ok in [
            request(None, None),
            request(Some("1"), Some(1)),
            request(Some("9007199254740"), Some(100)),
        ] {
            ok.validate().unwrap();
        }
        for bad in [
            request(Some("0"), None),
            request(Some("01"), None),
            request(Some(""), None),
            request(Some("-1"), None),
            request(Some("1.5"), None),
            request(Some("12345678901234567"), None),
            request(None, Some(0)),
            request(None, Some(101)),
        ] {
            assert_eq!(
                bad.validate().unwrap_err().code,
                "transcript-request-invalid",
                "{bad:?}"
            );
        }
        let mut blank = request(None, None);
        blank.assignment_id = " ".into();
        assert!(blank.validate().is_err());
        assert!(
            serde_json::from_value::<AssignmentTranscriptRequest>(
                serde_json::json!({"assignment_id": "a", "path": "/x"})
            )
            .is_err()
        );
    }
}
