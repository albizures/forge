//! Task-owned review Findings and their immutable, attributed activity history.
use crate::{
    Application, RegistrationError,
    checks::{RevisionComparison, validate_revision},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Minor,
    Major,
    Critical,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingState {
    Open,
    AwaitingReview,
    Resolved,
    Dismissed,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingAction {
    Raise,
    Resolve,
    Respond,
    Dismiss,
    Reopen,
    Amend,
    #[serde(rename = "assign-verification")]
    AssignVerification,
    Note,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingResponse {
    Fixed,
    Rejected,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FindingAmendment {
    pub old_severity: Severity,
    pub new_severity: Severity,
    pub old_blocking: bool,
    pub new_blocking: bool,
}
/// Typed review action; amendment fields never acquire creation defaults.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
pub enum FindingMutation {
    Respond {
        response: FindingResponse,
    },
    Resolve,
    Dismiss,
    Reopen,
    #[serde(rename = "assign-verification")]
    AssignVerification {
        check_id: u64,
    },
    Amend {
        severity: Option<Severity>,
        blocking: Option<bool>,
    },
    Note,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FindingMutationInput {
    pub change: FindingMutation,
    #[serde(flatten)]
    pub explanation: FindingResolveInput,
}
impl FindingMutationInput {
    /// Validate explanation, evidence and the explicit amendment.
    /// # Errors
    /// Rejects blank attribution/explanation, malformed evidence or empty amendments.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        self.explanation.validate()?;
        if matches!(
            self.change,
            FindingMutation::Amend {
                severity: None,
                blocking: None
            }
        ) {
            return Err(RegistrationError::new(
                "invalid-finding",
                "amend requires severity and/or blocking",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FindingActivity {
    pub id: u64,
    pub finding_id: u64,
    pub action: FindingAction,
    pub response: Option<FindingResponse>,
    pub amendment: Option<FindingAmendment>,
    pub actor: String,
    pub assignment_id: Option<String>,
    pub message: String,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
    pub created_at_millis: u64,
    pub comparison: RevisionComparison,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub id: u64,
    pub record_id: u64,
    pub version: u64,
    pub severity: Severity,
    pub blocking: bool,
    pub check_id: Option<u64>,
    pub assessment_id: Option<u64>,
    /// Originating Check, or an explicitly assigned review Check for an unlinked Finding.
    pub verification_check_id: Option<u64>,
    pub state: FindingState,
    pub raised: FindingActivity,
    pub latest_activity: FindingActivity,
    pub project: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FindingPage {
    pub finding: Finding,
    pub activities: Vec<FindingActivity>,
    pub next_cursor: Option<u64>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FindingList {
    pub findings: Vec<Finding>,
    pub next_cursor: Option<u64>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FindingRaiseInput {
    pub severity: Severity,
    pub blocking: Option<bool>,
    pub check_id: Option<u64>,
    pub assessment_id: Option<u64>,
    pub actor: String,
    pub message: String,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FindingResolveInput {
    pub expected_version: u64,
    pub actor: String,
    pub message: String,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
}
fn validate(actor: &str, message: &str, revision: Option<&str>) -> Result<(), RegistrationError> {
    if actor.trim().is_empty() || message.trim().is_empty() {
        return Err(RegistrationError::new(
            "invalid-finding",
            "actor and explanation must be nonblank",
        ));
    }
    validate_revision(revision)
}
impl FindingRaiseInput {
    /// Validate attribution, explanation and reported evidence.
    /// # Errors
    /// Rejects blank attribution/explanation or malformed revisions.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        validate(&self.actor, &self.message, self.revision.as_deref())
    }
}
impl FindingResolveInput {
    /// Validate attribution, verification explanation and reported evidence.
    /// # Errors
    /// Rejects blank attribution/explanation or malformed revisions.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        validate(&self.actor, &self.message, self.revision.as_deref())
    }
}
fn compare(activity: &mut FindingActivity, revision: Option<&str>, context: bool) {
    activity.comparison = match (activity.revision.as_deref(), revision, context) {
        (Some(a), Some(b), true) if a.eq_ignore_ascii_case(b) => RevisionComparison::Matching,
        (Some(_), Some(_), true) => RevisionComparison::Mismatching,
        _ => RevisionComparison::Unknown,
    };
}
fn compare_finding(finding: &mut Finding, revision: Option<&str>) {
    let context = finding.project.is_some();
    compare(&mut finding.raised, revision, context);
    compare(&mut finding.latest_activity, revision, context);
}
impl Application {
    /// Raise a Task-owned Finding atomically with its first activity and Record Update.
    /// # Errors
    /// Returns validation, scope, origin, frozen-state or storage errors.
    pub fn raise_record_finding(
        &self,
        task: u64,
        input: &FindingRaiseInput,
    ) -> Result<Finding, RegistrationError> {
        input.validate()?;
        self.records.raise_record_finding(task, input)
    }
    /// Resolve an unlinked, unowned Finding using an observed version and explanation.
    /// Linked/assigned Findings require verification by their Check's bound outcome.
    /// # Errors
    /// Returns validation, scope, stale-version, verification-required, transition or storage errors.
    pub fn resolve_record_finding(
        &self,
        task: u64,
        finding: u64,
        input: &FindingResolveInput,
    ) -> Result<Finding, RegistrationError> {
        input.validate()?;
        self.records.resolve_record_finding(task, finding, input)
    }
    /// Append a version-checked review action and its Record Update atomically.
    /// # Errors
    /// Returns validation, scope, stale-version, transition, frozen-state or storage errors.
    pub fn mutate_record_finding(
        &self,
        task: u64,
        finding: u64,
        input: &FindingMutationInput,
    ) -> Result<Finding, RegistrationError> {
        input.validate()?;
        self.records.mutate_record_finding(task, finding, input)
    }
    /// List a bounded ID-ordered page with evidence comparisons in the Task Project context.
    /// # Errors
    /// Returns validation, scope, pagination or storage errors.
    pub fn list_record_findings(
        &self,
        task: u64,
        after: Option<u64>,
        limit: u32,
        revision: Option<&str>,
    ) -> Result<FindingList, RegistrationError> {
        validate_revision(revision)?;
        let mut page = self.records.list_record_findings(task, after, limit)?;
        for finding in &mut page.findings {
            compare_finding(finding, revision);
        }
        Ok(page)
    }
    /// Inspect a Finding and a bounded activity page, retaining its immutable raised explanation.
    /// # Errors
    /// Returns validation, scope, ownership, pagination or storage errors.
    pub fn show_record_finding(
        &self,
        task: u64,
        finding: u64,
        after: Option<u64>,
        limit: u32,
        revision: Option<&str>,
    ) -> Result<FindingPage, RegistrationError> {
        validate_revision(revision)?;
        let mut page = self
            .records
            .show_record_finding(task, finding, after, limit)?;
        let context = page.finding.project.is_some();
        compare_finding(&mut page.finding, revision);
        for activity in &mut page.activities {
            compare(activity, revision, context);
        }
        Ok(page)
    }
}
