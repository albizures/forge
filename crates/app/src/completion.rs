//! Operator-only administrative closure evidence, independent of assessment verdicts.
use serde::{Deserialize, Serialize};

/// Result of an operator override, including immutable already-Done retries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionResult {
    pub record: crate::Record,
    pub already_done: bool,
    pub dependency_consequences: Option<DependencyConsequences>,
}

/// Completion-time workflow eligibility, not a dispatch or repository verification promise.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DependencyConsequences {
    pub newly_eligible_record_ids: Vec<u64>,
    pub warning: String,
}

impl DependencyConsequences {
    #[must_use]
    pub fn new(newly_eligible_record_ids: Vec<u64>) -> Self {
        Self {
            newly_eligible_record_ids,
            warning: "Normal reconciliation may dispatch newly eligible dependents. Eligibility is not dispatchability or a launch promise; Stack and publication evidence remain independent. Completion neither pauses dispatch nor launches work.".into(),
        }
    }
}

/// A workflow condition observed and bypassed in the completion transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BypassedGate {
    pub code: String,
    pub message: String,
}

/// Immutable evidence linked to the exact lifecycle update that closed the Record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ForcedCompletion {
    pub update_id: u64,
    pub actor: String,
    pub resolution: String,
    pub bypassed_gates: Vec<BypassedGate>,
}
