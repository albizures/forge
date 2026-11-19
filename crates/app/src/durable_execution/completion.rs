//! Runtime answers are observations, not authenticated Forge Outcomes.
use super::Launch;
use crate::RegistrationError;

/// The accepted-Outcome distinction is read from Forge, never supplied by a runner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnswerObservation {
    /// This exact attempt already has an accepted Outcome. This says nothing
    /// about its verdict, current freshness, retirement or successor eligibility.
    OutcomeAccepted,
    /// No accepted Outcome exists. Retained diagnostic and safety state have
    /// committed together; the Task and Check remain unresolved.
    MissingOutcome,
}

/// Trusted host observation port, not an Assignment capability operation.
/// Call only after observing an answer for the exact retained conversation.
/// A successful observation never authorizes cleanup or further execution.
pub trait Repository: Send + Sync {
    /// Recheck scope/runtime/attempt binding and accepted Outcome atomically.
    /// If missing, retain a visible diagnostic and hold without overwriting
    /// existing safety evidence. Replays preserve the first diagnostic timestamp.
    /// Delayed observations after closure or freshness invalidation are allowed;
    /// no execution authority is restored. A later accepted Outcome must not
    /// incidentally erase an earlier missing-Outcome diagnostic.
    /// # Errors
    /// Rejects foreign/mismatched identities, unaccepted runtime admission and
    /// persistence failures. Failure is not permission to release capacity.
    fn observe_answer(
        &self,
        workspace: &str,
        launch: &Launch,
    ) -> Result<AnswerObservation, RegistrationError>;
}
