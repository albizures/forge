//! Submits an Assignment's result the only way production allows: an Implementation Assignment
//! requests review (it has no verdict of its own), a review Assignment submits its outcome.
use forge_app::{
    RegistrationError,
    assignment::AuthenticatedAssignment,
    checks::Verdict,
    outcomes::{AcceptedOutcome, OutcomeDisposition, OutcomeInput},
};

/// # Panics
/// Panics if an Implementation Assignment is asked to submit anything but a complete pass,
/// because such a result is no longer reachable.
pub fn submit(
    session: &AuthenticatedAssignment<'_>,
    input: &OutcomeInput,
) -> Result<AcceptedOutcome, RegistrationError> {
    match session.ensure_not_implementation() {
        Err(e) if e.code == "implementation-outcome-forbidden" => {
            assert!(
                input.verdict == Verdict::Pass && input.disposition == OutcomeDisposition::Complete,
                "an Implementation Assignment can only request review"
            );
            session.request_review()
        }
        Err(e) => Err(e),
        Ok(()) => session.submit_outcome(input),
    }
}
