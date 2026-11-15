//! Allowlisted Task activity contract; excludes profiles, resource IDs and capabilities.
use forge_app::task_inspection::TaskActivity;
pub use forge_app::task_inspection::{StackInspection, TaskActivityState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskActivityResponse {
    pub state: TaskActivityState,
    pub stack: Option<StackInspection>,
    pub assignment_id: Option<String>,
    pub historical_assignment_state: Option<String>,
    pub closed_at_millis: Option<u64>,
    pub acknowledged_at_millis: Option<u64>,
    pub retained_resources: u64,
    pub dispatch_hold: Option<String>,
}

impl From<TaskActivity> for TaskActivityResponse {
    fn from(value: TaskActivity) -> Self {
        Self {
            state: value.state(),
            stack: value.stack,
            assignment_id: value.assignment.as_ref().map(|a| a.id.clone()),
            historical_assignment_state: value
                .assignment
                .as_ref()
                .map(|a| format!("{:?}", a.state).to_lowercase()),
            closed_at_millis: value.assignment.as_ref().and_then(|a| a.closed_at_millis),
            acknowledged_at_millis: value
                .assignment
                .as_ref()
                .and_then(|a| a.acknowledged_at_millis),
            retained_resources: value.retained_resources,
            dispatch_hold: value.dispatch_hold,
        }
    }
}
