//! Operator-owned, evidence-bound Manual attempts. No provider or worktree is required.
use crate::checks::{RecordCheck, Verdict};
use crate::{Application, RegistrationError};
use serde::{Deserialize, Serialize};

/// Caller-attested immutable work identity, not a mutable branch, path or URL alone.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Evidence {
    Repository { repository: String, commit: String },
    Artifact { uri: String, sha256: String },
}
impl Evidence {
    /// Validate the immutable identity without accessing Git or fetching artifacts.
    /// # Errors
    /// Rejects blank locations and malformed full content identities.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        let (location, digest, length) = match self {
            Self::Repository { repository, commit } => (repository, commit, 40),
            Self::Artifact { uri, sha256 } => (uri, sha256, 64),
        };
        if location.trim().is_empty()
            || digest.len() != length
            || !digest.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(RegistrationError::new(
                "invalid-evidence",
                "evidence requires a nonblank location and a full immutable hexadecimal digest",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptInput {
    pub id: String,
    pub check_id: u64,
    pub generation: u64,
    pub evidence: Evidence,
    pub actor: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub id: String,
    pub record_id: u64,
    pub check_id: u64,
    pub generation: u64,
    pub evidence: Evidence,
    pub actor: String,
    /// Task input text at the start of this immutable attempt.
    pub body: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeInput {
    pub attempt_id: String,
    pub generation: u64,
    /// Implementation's resulting deliverable; reviews must repeat their bound input evidence.
    pub evidence: Evidence,
    pub verdict: Verdict,
    pub actor: String,
    pub message: String,
    #[serde(default)]
    pub disposition: OutcomeDisposition,
    #[serde(default)]
    pub findings: Vec<OutcomeFinding>,
    #[serde(default)]
    pub verifications: Vec<FindingVerification>,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutcomeDisposition {
    #[default]
    Complete,
    Handback,
    Interrupted,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeFinding {
    pub severity: crate::findings::Severity,
    pub blocking: Option<bool>,
    pub message: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FindingVerification {
    pub finding_id: u64,
    pub expected_version: u64,
    pub message: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AcceptedOutcome {
    pub attempt: Attempt,
    pub evidence: Evidence,
    pub check: RecordCheck,
    pub findings: Vec<crate::findings::Finding>,
    pub work: WorkState,
    /// Residual blocking Findings; acceptance never clears an existing Dispatch Hold.
    pub blocking_findings: Vec<u64>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkState {
    pub generation: u64,
    pub input: Option<Evidence>,
    pub output: Option<Evidence>,
    pub latest_invalidation: Option<WorkInvalidation>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkInvalidation {
    pub generation: u64,
    pub actor: String,
    pub message: String,
    pub created_at_millis: u64,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkInvalidationInput {
    pub generation: u64,
    pub evidence: Evidence,
    pub actor: String,
    pub message: String,
}
impl Application {
    /// Inspect freshness and the immutable input/output evidence of a Task.
    /// # Errors
    /// Rejects inaccessible or non-Task owners.
    pub fn record_work_state(&self, record: u64) -> Result<WorkState, RegistrationError> {
        self.records.record_work_state(record)
    }
    /// Begin one explicitly identified Manual attempt without launching an agent.
    /// # Errors
    /// Rejects stale evidence, invalid order, active ownership or conflicting identities.
    pub fn begin_record_attempt(
        &self,
        record: u64,
        input: &AttemptInput,
    ) -> Result<Attempt, RegistrationError> {
        self.records.begin_record_attempt(record, input)
    }
    /// Atomically accept an outcome or return its original durable result on exact replay.
    /// # Errors
    /// Rejects stale, unauthorized, unbound or conflicting outcomes without effects.
    pub fn submit_record_outcome(
        &self,
        record: u64,
        input: &OutcomeInput,
    ) -> Result<AcceptedOutcome, RegistrationError> {
        self.records.submit_record_outcome(record, input)
    }
    /// Advance freshness, even at an unchanged commit, without deleting Assessment history.
    /// # Errors
    /// Rejects stale generations and inaccessible/frozen Tasks.
    pub fn invalidate_record_work(
        &self,
        record: u64,
        input: &WorkInvalidationInput,
    ) -> Result<WorkState, RegistrationError> {
        self.records.invalidate_record_work(record, input)
    }
}
