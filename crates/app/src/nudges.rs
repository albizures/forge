//! Nudges: Forge-sent prompts that keep a live Implementation Agent working after it ends a
//! turn without a Review Request.
//!
//! Every Nudge is durably recorded as a prompt intent before it is sent, and an uncertain
//! delivery places a Dispatch Hold instead of being retried (ADR-0008, extended to all prompts).
use crate::{
    AgentAdapterError, AgentAssignment, AgentAssignmentState, AgentObservation, AgentPrompt,
    AgentPromptOutcome, Application, RegistrationError, log_adapter_failure, outcomes::Evidence,
};
use std::path::Path;

/// Consecutive no-commit Nudges shown in the state line; enforcement belongs to nudge limits.
pub const NO_COMMIT_NUDGE_LIMIT: u32 = 3;

/// Per-Assignment Nudge accounting. A new Assignment starts at zero.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NudgeState {
    /// Herdr `completion_seq` of the finished turn most recently nudged (or attempted).
    pub last_completion_seq: u64,
    /// Nudges attempted for this Assignment, counting uncertain deliveries.
    pub total: u32,
    /// Nudges in a row after which the observed HEAD had not moved.
    pub consecutive_no_commit: u32,
    /// HEAD observed at the last nudge; absent until the first nudge.
    pub last_head: Option<String>,
}

/// A prompt intent recorded before a Nudge is sent. `sequence` is the Assignment's prompt
/// sequence: the nudge number, starting at one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NudgeIntent {
    pub sequence: u32,
    pub completion_seq: u64,
    pub consecutive_no_commit: u32,
    pub head: String,
}

/// How a recorded prompt intent ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NudgeSettlement {
    /// Herdr confirmed it admitted the prompt.
    Confirmed,
    /// Delivery cannot be established; the prompt is never resent.
    Uncertain,
}

/// The Forge-owned Nudge text. It does not repeat the Task body.
#[must_use]
pub fn nudge_text(worktree: &Path, consecutive_no_commit: u32) -> String {
    format!(
        "Your turn ended without a Review Request. Keep working in {}.\n\
- Continue the Task and every open Finding, and commit your work as you go.\n\
- Run `forge assignment request-review` only when the Task and every open Finding are fully met.\n\
- If you are blocked, explain why with `forge assignment comment --message '<explanation>'`, then end your turn.\n\n\
No new commit since the last nudge: {consecutive_no_commit} of {NO_COMMIT_NUDGE_LIMIT}.",
        worktree.display()
    )
}

impl Application {
    /// Nudge each open Implementation Assignment whose Agent finished a turn without a Review
    /// Request. Failures are logged and never stop the scan.
    pub(crate) fn nudge_idle_implementation_agents(&self) -> Result<(), RegistrationError> {
        let repository = self.dispatch()?;
        for assignment in repository.list_open_agent_assignments()? {
            if let Err(error) = self.nudge_assignment(&assignment) {
                tracing::warn!(assignment_id = %assignment.id, code = error.code,
                    "Nudge evaluation failed");
            }
        }
        Ok(())
    }

    /// The bound attempt when the Assignment is bound to the Task's first (Implementation) Check.
    fn implementation_attempt(
        &self,
        assignment: &AgentAssignment,
    ) -> Result<Option<crate::outcomes::Attempt>, RegistrationError> {
        let Some(attempt) = self.dispatch()?.assignment_check_attempt(&assignment.id)? else {
            return Ok(None);
        };
        let first = self
            .list_record_checks(attempt.record_id)?
            .first()
            .is_some_and(|check| check.id == attempt.check_id);
        Ok(first.then_some(attempt))
    }

    fn nudge_assignment(&self, assignment: &AgentAssignment) -> Result<(), RegistrationError> {
        let repository = self.dispatch()?;
        if assignment.state == AgentAssignmentState::Provisioning {
            return Ok(());
        }
        let Some(attempt) = self.implementation_attempt(assignment)? else {
            return Ok(());
        };
        // A restart (or crash) between recording an intent and settling it leaves an
        // unconfirmed prompt: it may have been delivered, so it is never resent.
        if let Some(intent) = repository.unsettled_assignment_nudge(&assignment.id)? {
            return self.settle_uncertain_nudge(
                assignment,
                &intent,
                "Forge restarted after recording the Nudge but before Herdr confirmed it",
            );
        }
        if repository.dispatch_hold(assignment.record_id)?.is_some()
            || repository.assignment_attempt_outcome_accepted(&assignment.id)?
            || !repository.assignment_context_read(&assignment.id)?
        {
            return Ok(());
        }
        let context = crate::agent_dispatch_context(repository, assignment)?;
        let adapter = self
            .adapter_factory()?
            .adapter_for(&assignment.profile)
            .map_err(|error| {
                log_adapter_failure(assignment, "select-adapter", &error);
                RegistrationError::new("agent-adapter-unavailable", "adapter unavailable")
            })?;
        let turn = match adapter.observe_turn(&context) {
            Ok(turn) => turn,
            Err(error) => {
                log_adapter_failure(assignment, "observe-turn", &error);
                return Ok(());
            }
        };
        // Only a finished turn, with a Herdr sequence we can compare, is ever nudged.
        let (AgentObservation::Idle | AgentObservation::Done, Some(completion_seq)) =
            (turn.observation, turn.completion_seq)
        else {
            return Ok(());
        };
        let state = repository.assignment_nudge_state(&assignment.id)?;
        if completion_seq <= state.last_completion_seq {
            return Ok(());
        }
        let Some(worktree) = context.agent_workspace.clone() else {
            return Ok(());
        };
        let head = self.inspector.head_commit(&worktree)?;
        let previous = match (&state.last_head, &attempt.evidence) {
            (Some(head), _) => Some(head.as_str()),
            (None, Evidence::Repository { commit, .. }) => Some(commit.as_str()),
            (None, Evidence::Artifact { .. }) => None,
        };
        let consecutive_no_commit = if previous == Some(head.as_str()) {
            state.consecutive_no_commit + 1
        } else {
            0
        };
        let intent = NudgeIntent {
            sequence: state.total + 1,
            completion_seq,
            consecutive_no_commit,
            head,
        };
        // Durable before any provider call: only the writer that records this intent sends it.
        if !repository.begin_assignment_nudge(&assignment.id, &intent)? {
            return Ok(());
        }
        let prompt = AgentPrompt {
            text: nudge_text(&worktree, consecutive_no_commit),
            completion_seq,
        };
        match adapter.prompt(&context, &prompt) {
            Ok(crate::AgentPromptOutcome::Delivered) => repository.settle_assignment_nudge(
                &assignment.id,
                intent.sequence,
                NudgeSettlement::Confirmed,
            ),
            // Provably unsent (the Agent moved on before the send): forget the intent.
            Ok(AgentPromptOutcome::NotReady) => {
                repository.cancel_assignment_nudge(&assignment.id, intent.sequence)
            }
            Err(error) => {
                log_adapter_failure(assignment, "prompt", &error);
                self.settle_uncertain_nudge(
                    assignment,
                    &intent,
                    match error {
                        AgentAdapterError::Conflict(_) => {
                            "the Agent's pinned terminal, pane or tab identity no longer matched"
                        }
                        _ => "Herdr did not confirm the prompt",
                    },
                )
            }
        }
    }

    /// Record Prompt Delivery Uncertainty and hold the Task. An existing hold keeps its reason.
    fn settle_uncertain_nudge(
        &self,
        assignment: &AgentAssignment,
        intent: &NudgeIntent,
        cause: &str,
    ) -> Result<(), RegistrationError> {
        let repository = self.dispatch()?;
        repository.settle_assignment_nudge(
            &assignment.id,
            intent.sequence,
            NudgeSettlement::Uncertain,
        )?;
        if repository.dispatch_hold(assignment.record_id)?.is_some() {
            return Ok(());
        }
        let reason = format!(
            "Prompt Delivery Uncertainty: Nudge {} to the Agent could not be confirmed ({cause}). It will not be resent; the Assignment remains open because the Agent may still be live. Investigate before resuming.",
            intent.sequence
        );
        repository.hold_dispatch(assignment.record_id, &reason)?;
        repository
            .add_record_comment(assignment.record_id, &reason, Some("agent-dispatch"))
            .map(|_| ())
    }
}
