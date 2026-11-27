//! Dispatch Holds for a Check-bound Task Assignment whose Agent launch failed or was never
//! acknowledged. Every failure after the launch intent is Prompt Delivery Uncertainty for the
//! initial prompt (ADR-0008): Forge never retries it and keeps the Assignment, its resources
//! and its capacity until the operator follows the recovery runbook.
use crate::{AgentAssignment, AgentDispatchRepository, RegistrationError};

/// Detail shown when the adapter error is not a reviewed Herdr `<method>: <code>` pair.
pub(crate) const WITHHELD: &str = "details withheld";

/// The operator runbook shared by both hold reasons.
fn runbook(assignment: &AgentAssignment) -> String {
    let id = &assignment.id;
    format!(
        " Check `herdr agent get forge-dispatch-{id}` and that its tab and pane are gone, then run `forge dispatcher assignment-retire {id}`, then `forge dispatcher assignment-resolve-unknown {id} --confirm-old-agent-cannot-act --confirm-unknown-resources-safe`, then `forge dispatcher resume {}`.",
        assignment.record_id
    )
}

fn failed_reason(assignment: &AgentAssignment, detail: &str) -> String {
    format!(
        "Agent launch failed for Assignment {}: {detail}. The launch may have been partly delivered, so Forge will not retry it.{}",
        assignment.id,
        runbook(assignment)
    )
}

fn expired_reason(assignment: &AgentAssignment) -> String {
    format!(
        "Agent acknowledgement deadline expired for Assignment {}: the Agent never acknowledged. Forge will not retry the launch.{}",
        assignment.id,
        runbook(assignment)
    )
}

/// Hold the Task and comment, unless it already has a Dispatch Hold, whose reason is kept.
fn hold_once(
    repository: &dyn AgentDispatchRepository,
    assignment: &AgentAssignment,
    reason: &str,
) -> Result<(), RegistrationError> {
    if repository.dispatch_hold(assignment.record_id)?.is_some() {
        return Ok(());
    }
    repository.hold_dispatch(assignment.record_id, reason)?;
    repository
        .add_record_comment(assignment.record_id, reason, Some("agent-dispatch"))
        .map(|_| ())
}

/// `detail` is already redacted: `Herdr <method>: <code>` or [`WITHHELD`].
pub(crate) fn hold_failed_launch(
    repository: &dyn AgentDispatchRepository,
    assignment: &AgentAssignment,
    detail: &str,
) -> Result<(), RegistrationError> {
    hold_once(repository, assignment, &failed_reason(assignment, detail))
}

pub(crate) fn hold_expired_acknowledgement(
    repository: &dyn AgentDispatchRepository,
    assignment: &AgentAssignment,
) -> Result<(), RegistrationError> {
    hold_once(repository, assignment, &expired_reason(assignment))
}
