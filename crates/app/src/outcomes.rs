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
    /// Most recent accepted handoff for this Check, captured when the attempt starts.
    #[serde(default)]
    pub continuation: Option<ContinuationCheckpoint>,
    /// Explicitly authorized salvage context, never a passing result or accepted handoff.
    #[serde(default)]
    pub recovery: Option<crate::retained_work::RetainedWorkRecovery>,
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
impl OutcomeInput {
    /// Validate deliberate handoff authority and content without observing external state.
    /// # Errors
    /// Rejects Manual/review handoffs, substantive effects and incomplete content.
    pub fn validate_continuation(
        &self,
        implementation: bool,
        authenticated: bool,
    ) -> Result<(), RegistrationError> {
        if let OutcomeDisposition::Continuation(handoff) = &self.disposition {
            handoff.validate()?;
            if !authenticated
                || !implementation
                || self.verdict != Verdict::Fail
                || !self.findings.is_empty()
                || !self.verifications.is_empty()
            {
                return Err(RegistrationError::new(
                    "invalid-continuation",
                    "Continuation is only for the authenticated Task Implementation agent, with Fail and no Finding or verification effects",
                ));
            }
        }
        Ok(())
    }

    /// Interruption cannot adopt a new checkpoint; accepted historical replay is exempt.
    /// # Errors
    /// Rejects changed interruption evidence without authorizing recovery or handoff.
    pub fn validate_interruption_evidence(
        &self,
        bound: &Evidence,
    ) -> Result<(), RegistrationError> {
        if self.disposition == OutcomeDisposition::Interrupted && &self.evidence != bound {
            return Err(RegistrationError::new(
                "interrupted-evidence-changed",
                "Interrupted outcomes must repeat their bound input evidence. Deliberate checkpoint handoffs require explicit Continuation support; genuinely interrupted retained work requires operator recovery. Use the explicit continuation disposition for a deliberate clean Implementation checkpoint; clean retained work requires the operator's explicit recover-retained-work authorization after safe retirement. Preserve the checkout and ask the operator; do not reset, discard changes, or report completion merely to hand off work.",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutcomeDisposition {
    #[default]
    Complete,
    Handback,
    Interrupted,
    /// No substantive verdict; stop for an explicit operator decision.
    HumanInput,
    /// Explicit unfinished Implementation checkpoint, never a substantive verdict.
    Continuation(ContinuationHandoff),
}

pub const CONTINUATION_LIMIT: u32 = 20;
pub const UNCHANGED_CONTINUATION_LIMIT: u32 = 2;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuationHandoff {
    pub stopping_reason: String,
    pub completed_work: String,
    /// Actual results, explicitly including gates not run and why.
    pub verification: String,
    pub remaining_work: String,
}
impl ContinuationHandoff {
    /// # Errors
    /// Rejects empty handoff sections; prose is not evaluated as semantic progress.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        if [
            &self.stopping_reason,
            &self.completed_work,
            &self.verification,
            &self.remaining_work,
        ]
        .iter()
        .any(|s| s.trim().is_empty())
        {
            return Err(RegistrationError::new(
                "invalid-continuation",
                "Continuation requires a concrete stopping reason, completed work, actual verification including unrun gates, and remaining work",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContinuationCheckpoint {
    pub attempt_id: String,
    pub evidence: Evidence,
    pub handoff: ContinuationHandoff,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckContinuations {
    pub check_id: u64,
    pub total: u32,
    pub consecutive_unchanged: u32,
    pub checkpoint: ContinuationCheckpoint,
    #[serde(default)]
    pub resets: Vec<ProgressReset>,
}
impl CheckContinuations {
    #[must_use]
    pub fn exhausted(&self) -> bool {
        self.total >= CONTINUATION_LIMIT
            || self.consecutive_unchanged >= UNCHANGED_CONTINUATION_LIMIT
    }
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
    #[serde(default)]
    pub recoveries: Vec<crate::retained_work::RetainedWorkRecovery>,
    pub progress: TaskProgress,
    pub generation: u64,
    pub input: Option<Evidence>,
    pub output: Option<Evidence>,
    pub latest_invalidation: Option<WorkInvalidation>,
}
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskProgress {
    #[serde(default)]
    pub continuations: Vec<CheckContinuations>,
    pub remediation_cycles: u32,
    pub baseline: Option<Evidence>,
    pub verified_resolutions: u64,
    pub stop_reason: Option<String>,
    pub interrupted_checks: Vec<CheckRetries>,
    pub resets: Vec<ProgressReset>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckRetries {
    pub check_id: u64,
    pub interruptions: u32,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProgressReset {
    pub actor: String,
    pub message: String,
    pub created_at_millis: u64,
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
    /// Explicitly reset only remediation policy; retain holds, retries, evidence and ownership.
    /// # Errors
    /// Rejects blank attribution, non-Tasks and unsafe ownership.
    pub fn reset_task_remediation(
        &self,
        record: u64,
        actor: &str,
        message: &str,
    ) -> Result<WorkState, RegistrationError> {
        self.dispatch()?
            .reset_task_remediation(record, actor, message)
    }
    /// Reset only one Check's continuation counters, retaining checkpoints and safety state.
    /// # Errors
    /// Rejects blank attribution and Check identities outside the selected Task.
    pub fn reset_check_continuations(
        &self,
        record: u64,
        check: u64,
        actor: &str,
        message: &str,
    ) -> Result<WorkState, RegistrationError> {
        self.records
            .reset_record_continuations(record, check, actor, message)
    }
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
        // Manual start is not continuing authorization for later Check attempts.
        // Observe hosting outside the writer transaction; persistence rechecks
        // the retained plan, pins and result generations when admitting the attempt.
        self.verify_stack_execution(record)?;
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
