//! Persisted Task activity, independent of Record lifecycle and provider observations.
use crate::{AgentAssignmentState, Application, RegistrationError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskActivity {
    pub assignment: Option<AssignmentActivity>,
    /// Counts all unretired resources, including older closed attempts.
    pub retained_resources: u64,
    pub dispatch_hold: Option<String>,
    pub stack: Option<StackInspection>,
}

/// Exact persisted lineage, not a fresh Git/hosting observation.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct StackInspection {
    pub predecessor: Option<u64>,
    pub expected_commit: Option<String>,
    pub verified_predecessor_commit: Option<String>,
    pub problem: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignmentActivity {
    pub id: String,
    pub state: AgentAssignmentState,
    pub closed_at_millis: Option<u64>,
    pub acknowledged_at_millis: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskActivityState {
    Absent,
    Closed,
    Provisioning,
    Working,
    Idle,
    Blocked,
    Unknown,
    RetainedResources,
}

impl TaskActivity {
    /// Closure takes precedence over historical state; retained resources never imply absence.
    #[must_use]
    pub fn state(&self) -> TaskActivityState {
        if self.retained_resources > 0
            && self
                .assignment
                .as_ref()
                .is_none_or(|a| a.closed_at_millis.is_some())
        {
            return TaskActivityState::RetainedResources;
        }
        match &self.assignment {
            None => TaskActivityState::Absent,
            Some(a) if a.closed_at_millis.is_some() => TaskActivityState::Closed,
            Some(a) => match a.state {
                AgentAssignmentState::Provisioning => TaskActivityState::Provisioning,
                AgentAssignmentState::Working => TaskActivityState::Working,
                AgentAssignmentState::Idle => TaskActivityState::Idle,
                AgentAssignmentState::Blocked => TaskActivityState::Blocked,
                AgentAssignmentState::Unknown => TaskActivityState::Unknown,
            },
        }
    }
}

impl Application {
    /// Read persisted facts only. No reconciliation, Git or provider operations.
    /// # Errors
    /// Returns scope, configuration and storage errors without inventing absence.
    pub fn task_activity(&self, record_id: u64) -> Result<TaskActivity, RegistrationError> {
        self.show_record(record_id)?;
        self.records.task_activity(record_id)
    }
}
