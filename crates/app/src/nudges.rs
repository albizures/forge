//! Nudges: Forge-sent prompts that keep a live Implementation Agent working after it ends a
//! turn without a Review Request.
//!
//! Every Nudge is durably recorded as a prompt intent before it is sent, and an uncertain
//! delivery places a Dispatch Hold instead of being retried (ADR-0008, extended to all prompts).
use crate::{
    AgentAdapterError, AgentAssignment, AgentAssignmentClosureReason, AgentAssignmentState, AgentObservation, AgentPrompt,
    AgentPromptOutcome, Application, RegistrationError, log_adapter_failure, outcomes::Evidence,
};
use std::path::Path;

/// Consecutive no-commit Nudges allowed per Assignment; the next idle turn holds instead.
pub const NO_COMMIT_NUDGE_LIMIT: u32 = 3;
/// Nudges allowed per Assignment overall; the next idle turn holds instead.
pub const TOTAL_NUDGE_LIMIT: u32 = 20;

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
    /// Operator explanation from `resume`, carried by the next Nudge.
    pub pending_explanation: Option<String>,
    /// Prompt intents ever recorded for this Assignment. Unlike `total` it never resets, so the
    /// next intent's sequence is this plus one.
    pub prompt_sequence: u32,
}

/// A prompt intent recorded before a Nudge is sent. `sequence` is the Assignment's prompt
/// sequence, starting at one; it keeps counting across an operator resume.
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
pub fn nudge_text(
    worktree: &Path,
    consecutive_no_commit: u32,
    explanation: Option<&str>,
) -> String {
    let explanation = explanation.map_or_else(String::new, |text| {
        format!("\nOperator explanation: {text}\n\n")
    });
    format!(
        "Your turn ended without a Review Request. Keep working in {}.\n{explanation}\
- Continue the Task and every open Finding, and commit your work as you go.\n\
- Run `forge assignment request-review` only when the Task and every open Finding are fully met.\n\
- If you are blocked, explain why with `forge assignment comment --message '<explanation>'`, then end your turn.\n\n\
No new commit since the last nudge: {consecutive_no_commit} of {NO_COMMIT_NUDGE_LIMIT}.",
        worktree.display()
    )
}

/// Which Nudge limit an idle Agent has reached.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NudgeLimit {
    ConsecutiveNoCommit,
    Total,
}

impl NudgeLimit {
    fn describe(self) -> String {
        match self {
            Self::ConsecutiveNoCommit => format!(
                "{NO_COMMIT_NUDGE_LIMIT} consecutive Nudges produced no new commit (consecutive-no-commit limit)"
            ),
            Self::Total => format!("{TOTAL_NUDGE_LIMIT} Nudges were sent (total limit)"),
        }
    }
}

/// How many Nudges in a row (including the next) found HEAD unmoved from its baseline: the last
/// Nudge's HEAD, or for the first Nudge the attempt's input commit. A new commit restarts at 0.
fn next_no_commit_count(state: &NudgeState, evidence: &Evidence, head: &str) -> u32 {
    let previous = match (&state.last_head, evidence) {
        (Some(last), _) => Some(last.as_str()),
        (None, Evidence::Repository { commit, .. }) => Some(commit.as_str()),
        (None, Evidence::Artifact { .. }) => None,
    };
    if previous == Some(head) {
        state.consecutive_no_commit + 1
    } else {
        0
    }
}

/// The limit that forbids another Nudge, given how many no-commit Nudges the next one would be.
fn exhausted_limit(state: &NudgeState, next_no_commit: u32) -> Option<NudgeLimit> {
    if state.total >= TOTAL_NUDGE_LIMIT {
        Some(NudgeLimit::Total)
    } else if next_no_commit > NO_COMMIT_NUDGE_LIMIT {
        Some(NudgeLimit::ConsecutiveNoCommit)
    } else {
        None
    }
}

impl Application {
    /// Nudge each open Implementation Assignment whose Agent finished a turn without a Review
    /// Request. Failures are logged and never stop the scan.
    pub(crate) fn nudge_idle_implementation_agents(&self) -> Result<(), RegistrationError> {
        let repository = self.dispatch()?;
        for assignment in repository.list_open_agent_assignments()? {
            match self.reap_absent_agent(&assignment) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(assignment_id = %assignment.id, code = error.code,
                        "Absent Agent evaluation failed");
                    continue;
                }
            }
            if let Err(error) = self.nudge_assignment(&assignment) {
                tracing::warn!(assignment_id = %assignment.id, code = error.code,
                    "Nudge evaluation failed");
            }
        }
        Ok(())
    }

    /// Liveness for an open Check-bound Assignment whose Agent Herdr no longer has (`Absent`):
    /// hold the Task, retire the provider resources and close the Assignment as Lost. The Task
    /// is never relaunched automatically because its worktree may hold unreported work.
    /// Returns whether the Assignment was Absent. Any other observation, including `Unknown`,
    /// is left to the usual grace and recovery handling.
    fn reap_absent_agent(&self, assignment: &AgentAssignment) -> Result<bool, RegistrationError> {
        let repository = self.dispatch()?;
        if assignment.state == AgentAssignmentState::Provisioning
            || repository.assignment_check_attempt(&assignment.id)?.is_none()
            || repository.assignment_attempt_outcome_accepted(&assignment.id)?
        {
            return Ok(false);
        }
        // While agent.start is in flight its identity is not yet retained, so an absent
        // observation is not evidence that the Agent died.
        if repository.agent_launch_intent(&assignment.id)?.is_some()
            && !repository
                .list_agent_assignment_resources(&assignment.id)?
                .iter()
                .any(|resource| {
                    resource.resource_kind == "agent-name" && resource.retired_at_millis.is_none()
                })
        {
            return Ok(false);
        }
        let context = crate::agent_dispatch_context(repository, assignment)?;
        let adapter = self
            .adapter_factory()?
            .adapter_for(&assignment.profile)
            .map_err(|error| {
                log_adapter_failure(assignment, "select-adapter", &error);
                RegistrationError::new("agent-adapter-unavailable", "adapter unavailable")
            })?;
        match adapter.observe(&context) {
            Ok(AgentObservation::Absent) => {}
            Ok(_) => return Ok(false),
            Err(error) => {
                log_adapter_failure(assignment, "observe", &error);
                return Ok(false);
            }
        }
        if repository.dispatch_hold(assignment.record_id)?.is_none() {
            let reason = format!(
                "Agent lost: Herdr no longer has the Agent of Assignment {}. The Assignment is closed as Lost and Forge will not relaunch it, because its worktree may hold unreported work. Inspect the worktree, then follow retained-work recovery in docs/assignment-recovery.md (`forge dispatcher recover-retained-work`) and `forge dispatcher resume` once it is safe.",
                assignment.id
            );
            repository.hold_dispatch(assignment.record_id, &reason)?;
            repository.add_record_comment(assignment.record_id, &reason, Some("agent-dispatch"))?;
        }
        if let Err(error) = adapter.retire(&context, repository) {
            log_adapter_failure(assignment, "retire", &error);
            return Ok(true);
        }
        repository.compare_and_close_agent_assignment(
            &assignment.id,
            assignment.state,
            AgentAssignmentClosureReason::Lost,
        )?;
        Ok(true)
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
        // An operator explanation from `resume` is delivered at the next finished turn, even
        // one that was already seen before the hold.
        if completion_seq <= state.last_completion_seq && state.pending_explanation.is_none() {
            return Ok(());
        }
        let Some(worktree) = context.agent_workspace.clone() else {
            return Ok(());
        };
        let head = self.inspector.head_commit(&worktree)?;
        let consecutive_no_commit = next_no_commit_count(&state, &attempt.evidence, &head);
        if let Some(limit) = exhausted_limit(&state, consecutive_no_commit) {
            return self.hold_exhausted_nudges(assignment, limit);
        }
        let intent = NudgeIntent {
            sequence: state.prompt_sequence + 1,
            completion_seq,
            consecutive_no_commit,
            head,
        };
        // Durable before any provider call: only the writer that records this intent sends it.
        if !repository.begin_assignment_nudge(&assignment.id, &intent)? {
            return Ok(());
        }
        let prompt = AgentPrompt {
            text: nudge_text(
                &worktree,
                consecutive_no_commit,
                state.pending_explanation.as_deref(),
            ),
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

    /// Place a Dispatch Hold instead of a further Nudge. The Assignment stays open and the
    /// Agent is not retired; the operator resumes with an explanation or retires via unstick.
    fn hold_exhausted_nudges(
        &self,
        assignment: &AgentAssignment,
        limit: NudgeLimit,
    ) -> Result<(), RegistrationError> {
        let repository = self.dispatch()?;
        let latest = repository
            .list_record_comments(assignment.record_id)?
            .into_iter()
            .find(|comment| comment.author.as_deref() != Some("agent-dispatch"));
        let comment = latest.map_or_else(
            || " The Agent left no comment explaining itself.".to_owned(),
            |comment| {
                let mut excerpt: String = comment.body.chars().take(200).collect();
                if comment.body.chars().count() > 200 {
                    excerpt.push_str("...");
                }
                format!(" Latest comment (#{}): {excerpt}", comment.id.0)
            },
        );
        let reason = format!(
            "Nudges exhausted: {}. The Assignment stays open and the Agent is not retired.{comment} Resume with an explanation to nudge it again, or `forge task unstick --retire-working --agent-gone` to retire it and start over.",
            limit.describe()
        );
        repository.hold_dispatch(assignment.record_id, &reason)?;
        repository
            .add_record_comment(assignment.record_id, &reason, Some("agent-dispatch"))
            .map(|_| ())
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
