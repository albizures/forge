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
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FindingActivity {
    pub id: u64,
    pub finding_id: u64,
    pub action: FindingAction,
    pub actor: String,
    pub assignment_id: Option<String>,
    pub message: String,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
    pub created_at_millis: u64,
    pub comparison: RevisionComparison,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Finding {
    pub id: u64,
    pub task_id: u64,
    pub version: u64,
    pub severity: Severity,
    pub blocking: bool,
    pub check_id: Option<u64>,
    pub assessment_id: Option<u64>,
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
    pub fn raise_task_finding(
        &self,
        task: u64,
        input: &FindingRaiseInput,
    ) -> Result<Finding, RegistrationError> {
        input.validate()?;
        self.records.raise_task_finding(task, input)
    }
    /// Resolve an unresolved Finding using an observed version and verification explanation.
    /// # Errors
    /// Returns validation, scope, stale-version, transition or storage errors.
    pub fn resolve_task_finding(
        &self,
        task: u64,
        finding: u64,
        input: &FindingResolveInput,
    ) -> Result<Finding, RegistrationError> {
        input.validate()?;
        self.records.resolve_task_finding(task, finding, input)
    }
    /// List a bounded ID-ordered page with evidence comparisons in the Task Project context.
    /// # Errors
    /// Returns validation, scope, pagination or storage errors.
    pub fn list_task_findings(
        &self,
        task: u64,
        after: Option<u64>,
        limit: u32,
        revision: Option<&str>,
    ) -> Result<FindingList, RegistrationError> {
        validate_revision(revision)?;
        let mut page = self.records.list_task_findings(task, after, limit)?;
        for finding in &mut page.findings {
            compare_finding(finding, revision);
        }
        Ok(page)
    }
    /// Inspect a Finding and a bounded activity page, retaining its immutable raised explanation.
    /// # Errors
    /// Returns validation, scope, ownership, pagination or storage errors.
    pub fn show_task_finding(
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
            .show_task_finding(task, finding, after, limit)?;
        let context = page.finding.project.is_some();
        compare_finding(&mut page.finding, revision);
        for activity in &mut page.activities {
            compare(activity, revision, context);
        }
        Ok(page)
    }
}
