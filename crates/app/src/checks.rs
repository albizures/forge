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
    pub id: u64,
    pub check_id: u64,
    pub verdict: Verdict,
    pub actor: String,
    pub message: String,
    pub created_at_millis: u64,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
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
        if self.actor.trim().is_empty() || self.message.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-assessment",
                "actor and explanation must be nonblank",
            ));
        }
        if self
            .revision
            .as_ref()
            .is_some_and(|sha| sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(RegistrationError::new(
                "invalid-revision",
                "revision must be a full commit SHA",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckPage {
    pub check: TaskCheck,
    pub assessments: Vec<Assessment>,
    pub next_cursor: Option<u64>,
}

impl Application {
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
