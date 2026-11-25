//! Structured observations made by the initiating planning client.

/// An observation for one Project, not a commit pin or a working-tree snapshot.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PlanningContext {
    Available { repository: String, branch: String },
    Unavailable { reason: String },
}

/// Associates an observation with the immutable qualified Project reference.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ProjectPlanningContext {
    pub project: String,
    pub context: PlanningContext,
}
