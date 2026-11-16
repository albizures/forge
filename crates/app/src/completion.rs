//! Operator-only administrative closure evidence, independent of assessment verdicts.
use serde::{Deserialize, Serialize};

/// Result of an operator override, including immutable already-Done retries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionResult {
    pub record: crate::Record,
    pub already_done: bool,
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
