//! Immutable Task criteria and attributed, versioned Assessments.
use crate::{Application, RegistrationError};
use serde::{Deserialize, Serialize};

pub const IMPLEMENTATION_CRITERION: &str =
    "The work and deliverables described by this Task are complete.";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Pass,
    Fail,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Assessment {
    /// Present only for capability-authenticated mutations; None denotes operator attribution.
    pub assignment_id: Option<String>,
    pub id: u64,
    pub check_id: u64,
    pub verdict: Verdict,
    pub actor: String,
    pub message: String,
    pub created_at_millis: u64,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
    pub invalidation: Option<Invalidation>,
    pub comparison: RevisionComparison,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RevisionComparison {
    Matching,
    Mismatching,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Invalidation {
    pub assignment_id: Option<String>,
    pub id: u64,
    pub assessment_id: u64,
    pub actor: String,
    pub message: String,
    pub created_at_millis: u64,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
    pub comparison: RevisionComparison,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InvalidationInput {
    pub expected_version: u64,
    pub assessment_id: u64,
    pub actor: String,
    pub message: String,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
}
impl InvalidationInput {
    /// Validate attribution, explanation and caller-reported evidence.
    ///
    /// # Errors
    /// Rejects blank attribution/explanation and malformed revisions.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        validate_input(&self.actor, &self.message, self.revision.as_deref())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckActivity {
    pub id: u64,
    pub assessment_id: u64,
    pub invalidation: Option<Invalidation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskCheck {
    pub id: u64,
    pub task_id: u64,
    pub name: String,
    pub description: String,
    pub position: u64,
    pub version: u64,
    pub latest_assessment: Option<Assessment>,
    /// Null means pending, including an invalidated latest Assessment.
    pub current_verdict: Option<Verdict>,
    pub project: Option<String>,
}
impl TaskCheck {
    #[must_use]
    pub fn verdict(&self) -> Option<Verdict> {
        self.latest_assessment
            .as_ref()
            .filter(|a| a.invalidation.is_none())
            .map(|a| a.verdict)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AssessmentInput {
    pub expected_version: u64,
    pub verdict: Verdict,
    pub actor: String,
    pub message: String,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
}
impl AssessmentInput {
    /// Validate caller-supplied attribution and evidence without executing Git.
    ///
    /// # Errors
    /// Rejects blank attribution/explanation or a malformed full commit SHA.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        validate_input(&self.actor, &self.message, self.revision.as_deref())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckPage {
    pub check: TaskCheck,
    pub assessments: Vec<Assessment>,
    pub next_cursor: Option<u64>,
    pub activities: Vec<CheckActivity>,
}

fn validate_input(
    actor: &str,
    message: &str,
    revision: Option<&str>,
) -> Result<(), RegistrationError> {
    if actor.trim().is_empty() || message.trim().is_empty() {
        return Err(RegistrationError::new(
            "invalid-assessment",
            "actor and explanation must be nonblank",
        ));
    }
    validate_revision(revision)
}

/// Validate a full caller-reported commit SHA without consulting Git.
///
/// # Errors
/// Rejects malformed revisions.
pub fn validate_revision(revision: Option<&str>) -> Result<(), RegistrationError> {
    if revision.is_some_and(|sha| sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err(RegistrationError::new(
            "invalid-revision",
            "revision must be a full commit SHA",
        ));
    }
    Ok(())
}

fn compare(evidence: Option<&str>, revision: Option<&str>, context: bool) -> RevisionComparison {
    match (evidence, revision, context) {
        (Some(a), Some(b), true) if a.eq_ignore_ascii_case(b) => RevisionComparison::Matching,
        (Some(_), Some(_), true) => RevisionComparison::Mismatching,
        _ => RevisionComparison::Unknown,
    }
}
fn compare_assessment(a: &mut Assessment, revision: Option<&str>, context: bool) {
    a.comparison = compare(a.revision.as_deref(), revision, context);
    if let Some(i) = &mut a.invalidation {
        i.comparison = compare(i.revision.as_deref(), revision, context);
    }
}

impl Application {
    /// Invalidate only the latest active Assessment at the observed Check version.
    ///
    /// # Errors
    /// Rejects invalid input, stale versions, invalid transitions and frozen Tasks.
    pub fn invalidate_task_check(
        &self,
        task: u64,
        check: u64,
        input: &InvalidationInput,
    ) -> Result<TaskCheck, RegistrationError> {
        input.validate()?;
        self.records.invalidate_task_check(task, check, input)
    }
    /// List criteria and compare reported evidence within the Task Project.
    ///
    /// # Errors
    /// Returns validation, scope, type or storage errors.
    pub fn compare_task_checks(
        &self,
        task: u64,
        revision: Option<&str>,
    ) -> Result<Vec<TaskCheck>, RegistrationError> {
        validate_revision(revision)?;
        let mut checks = self.list_task_checks(task)?;
        for check in &mut checks {
            if let Some(a) = &mut check.latest_assessment {
                compare_assessment(a, revision, check.project.is_some());
            }
        }
        Ok(checks)
    }
    /// Inspect a bounded activity page and compare its caller-reported evidence.
    ///
    /// # Errors
    /// Returns validation, scope, pagination or storage errors.
    pub fn compare_task_check(
        &self,
        task: u64,
        check: u64,
        after: Option<u64>,
        limit: u32,
        revision: Option<&str>,
    ) -> Result<CheckPage, RegistrationError> {
        validate_revision(revision)?;
        let mut page = self.show_task_check(task, check, after, limit)?;
        let context = page.check.project.is_some();
        if let Some(a) = &mut page.check.latest_assessment {
            compare_assessment(a, revision, context);
        }
        for a in &mut page.assessments {
            compare_assessment(a, revision, context);
        }
        for activity in &mut page.activities {
            if let Some(i) = &mut activity.invalidation {
                i.comparison = compare(i.revision.as_deref(), revision, context);
            }
        }
        Ok(page)
    }
    /// List a Task's immutable required criteria.
    ///
    /// # Errors
    /// Returns scope, Record type, or storage errors.
    pub fn list_task_checks(&self, task: u64) -> Result<Vec<TaskCheck>, RegistrationError> {
        self.records.list_task_checks(task)
    }
    /// Inspect one Check and a bounded Assessment history page.
    ///
    /// # Errors
    /// Returns scope, Check ownership, pagination, or storage errors.
    pub fn show_task_check(
        &self,
        task: u64,
        check: u64,
        after: Option<u64>,
        limit: u32,
    ) -> Result<CheckPage, RegistrationError> {
        self.records.show_task_check(task, check, after, limit)
    }
    /// Append an attributed pass/fail Assessment at the observed version.
    ///
    /// # Errors
    /// Rejects invalid input, stale versions, Done Tasks, or inaccessible Checks.
    pub fn assess_task_check(
        &self,
        task: u64,
        check: u64,
        input: &AssessmentInput,
    ) -> Result<TaskCheck, RegistrationError> {
        input.validate()?;
        self.records.assess_task_check(task, check, input)
    }
}
