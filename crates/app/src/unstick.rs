//! One-command Task recovery: plans and applies the documented Assignment recovery runbook
//! by composing the existing explicit operations. It adds no new authority: every operator
//! assertion still comes from an explicit consent flag, and every gate of the underlying
//! operations (retirement verification, salvage revalidation, dispatch eligibility) applies.
use crate::{
    AgentAssignment, AgentAssignmentInfo, AgentAssignmentState, Application, RecordStatus,
    RecordType, RegistrationError, outcomes::Evidence, retained_work::RecoveryInput,
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Explicit operator assertions. Absent assertions never default to true.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
// Independent operator assertions that mirror CLI flags one to one; not a state machine.
#[allow(clippy::struct_excessive_bools)]
pub struct UnstickConsent {
    /// Asserts both that the old agent cannot act and that its resources are safe.
    pub agent_gone: bool,
    /// Authorizes salvaging retained Implementation work instead of stopping.
    pub keep_work: bool,
    /// Authorizes resetting exhausted remediation or continuation budgets.
    pub reset_budgets: bool,
    /// Authorizes retiring an agent that persisted observations still report as active.
    pub retire_working: bool,
    pub actor: Option<String>,
    pub message: Option<String>,
}

/// An operator input a step requires, named as its CLI flag.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnstickFlag {
    AgentGone,
    KeepWork,
    ResetBudgets,
    RetireWorking,
    Actor,
    Message,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnstickStepKind {
    Retire,
    ResolveUnknown,
    Salvage,
    ResetRemediation,
    ResetContinuations,
    Resume,
    Dispatch,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnstickStepStatus {
    /// Would run (plan) or was not reached (apply stopped earlier).
    Pending,
    Done,
    NeedsFlag,
    Blocked,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UnstickStep {
    pub kind: UnstickStepKind,
    pub status: UnstickStepStatus,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_flags: Vec<UnstickFlag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnstickVerdict {
    /// The agent is alive and nothing blocks it.
    Healthy,
    /// A launch is still within its acknowledgement deadline.
    Launching,
    /// Nothing to recover (for example, the Task is Done).
    NothingToDo,
    /// Recovery steps apply and all required inputs were given.
    Stuck,
    /// At least one step needs an explicit operator flag.
    NeedsFlag,
    /// Recovery cannot proceed automatically; investigate the reason.
    Stopped,
    /// Apply completed every step.
    Unstuck,
    /// Apply stopped at a failed step.
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UnstickReport {
    pub record_id: u64,
    pub verdict: UnstickVerdict,
    pub summary: String,
    pub applied: bool,
    pub steps: Vec<UnstickStep>,
    /// The Assignment open after apply, if dispatch produced or found one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment_id: Option<String>,
}

/// Planned salvage input, observed outside any transaction and revalidated by the operation.
#[derive(Clone, Debug)]
struct SalvagePlan {
    generation: u64,
    evidence: Evidence,
}

struct Plan {
    report: UnstickReport,
    salvage: Option<SalvagePlan>,
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn step(kind: UnstickStepKind, reason: impl Into<String>) -> UnstickStep {
    UnstickStep {
        kind,
        status: UnstickStepStatus::Pending,
        reason: reason.into(),
        assignment_id: None,
        check_id: None,
        required_flags: Vec::new(),
        error: None,
    }
}

fn nonblank(value: Option<&String>) -> bool {
    value.is_some_and(|v| !v.trim().is_empty())
}

impl UnstickConsent {
    fn missing(&self, flags: &[UnstickFlag]) -> Vec<UnstickFlag> {
        flags
            .iter()
            .copied()
            .filter(|flag| match flag {
                UnstickFlag::AgentGone => !self.agent_gone,
                UnstickFlag::KeepWork => !self.keep_work,
                UnstickFlag::ResetBudgets => !self.reset_budgets,
                UnstickFlag::RetireWorking => !self.retire_working,
                UnstickFlag::Actor => !nonblank(self.actor.as_ref()),
                UnstickFlag::Message => !nonblank(self.message.as_ref()),
            })
            .collect()
    }
}

impl UnstickStep {
    fn requiring(mut self, consent: &UnstickConsent, flags: &[UnstickFlag]) -> Self {
        self.required_flags = consent.missing(flags);
        if !self.required_flags.is_empty() {
            self.status = UnstickStepStatus::NeedsFlag;
        }
        self
    }
}

/// Unretired resources other than the provisioning-intent marker, which retirement cannot
/// clean up because its identity is unknown.
fn has_known_unretired(info: &AgentAssignmentInfo) -> bool {
    info.resources
        .iter()
        .any(|r| r.retired_at_millis.is_none() && r.resource_kind != "provision-intent")
}

fn has_uncertain_provisioning(info: &AgentAssignmentInfo) -> bool {
    info.recovery_blockers()
        .iter()
        .any(|b| b == "uncertain-provisioning")
}

fn agent_active(state: AgentAssignmentState) -> bool {
    matches!(
        state,
        AgentAssignmentState::Working | AgentAssignmentState::Idle | AgentAssignmentState::Blocked
    )
}

impl Application {
    /// Read-only recovery plan for one Task: which runbook steps apply, why, and which
    /// explicit operator flags each still needs. Contacts Git only to observe retained work.
    ///
    /// # Errors
    /// Returns lookup, configuration or storage failures.
    pub fn unstick_plan(
        &self,
        record: u64,
        consent: &UnstickConsent,
    ) -> Result<UnstickReport, RegistrationError> {
        Ok(self.plan_unstick(record, consent)?.report)
    }

    /// Applies the recovery plan in runbook order through the existing operations, stopping at
    /// the first step that needs a flag, is blocked, or fails. Each step keeps its own safety
    /// gates; a stop leaves all retained holds, evidence and budgets in place.
    ///
    /// # Errors
    /// Returns lookup, configuration or storage failures raised while planning.
    pub fn unstick_apply(
        &self,
        record: u64,
        consent: &UnstickConsent,
    ) -> Result<UnstickReport, RegistrationError> {
        let Plan {
            mut report,
            salvage,
        } = self.plan_unstick(record, consent)?;
        report.applied = true;
        if report
            .steps
            .iter()
            .any(|s| s.status != UnstickStepStatus::Pending)
        {
            // Never apply a prefix of a plan that is already known not to finish.
            return Ok(report);
        }
        let mut failed = false;
        for index in 0..report.steps.len() {
            let current = &report.steps[index];
            let result = self.apply_unstick_step(record, current, salvage.as_ref(), consent);
            let current = &mut report.steps[index];
            match result {
                Ok(assignment) => {
                    current.status = UnstickStepStatus::Done;
                    if let Some(assignment) = assignment {
                        report.assignment_id = Some(assignment.id);
                    }
                }
                Err(error) => {
                    current.status = UnstickStepStatus::Failed;
                    current.error = Some(format!("{}: {}", error.code, error.message));
                    failed = true;
                    break;
                }
            }
        }
        if failed {
            report.verdict = UnstickVerdict::Failed;
            report.summary =
                "Recovery stopped at a failed step; retained holds and evidence are unchanged by later steps. Investigate the error, then re-run.".into();
        } else {
            report.verdict = UnstickVerdict::Unstuck;
            report.summary = match &report.assignment_id {
                Some(id) => format!("Task {record} recovered; current Assignment {id}."),
                None => format!("Task {record} recovered."),
            };
        }
        Ok(report)
    }

    fn apply_unstick_step(
        &self,
        record: u64,
        step: &UnstickStep,
        salvage: Option<&SalvagePlan>,
        consent: &UnstickConsent,
    ) -> Result<Option<AgentAssignment>, RegistrationError> {
        let assignment = || {
            step.assignment_id
                .clone()
                .ok_or_else(|| RegistrationError::new("unstick-invalid", "step has no Assignment"))
        };
        let actor = consent.actor.clone().unwrap_or_default();
        let message = consent.message.clone().unwrap_or_default();
        match step.kind {
            UnstickStepKind::Retire => {
                let id = assignment()?;
                self.retire_agent_assignment(&id)?;
                // Retirement verifies provider absence; surviving known resources stop recovery.
                if has_known_unretired(&self.agent_assignment_info(&id)?) {
                    return Err(RegistrationError::new(
                        "resources-survived-cleanup",
                        "known resources remain after retirement; investigate the provider and retry",
                    ));
                }
                Ok(None)
            }
            UnstickStepKind::ResolveUnknown => {
                self.resolve_unknown_agent_resources(
                    &assignment()?,
                    consent.agent_gone,
                    consent.agent_gone,
                )?;
                Ok(None)
            }
            UnstickStepKind::Salvage => {
                let plan = salvage.ok_or_else(|| {
                    RegistrationError::new("unstick-invalid", "salvage step has no observation")
                })?;
                self.recover_retained_work(
                    record,
                    &RecoveryInput {
                        source_assignment: assignment()?,
                        generation: plan.generation,
                        evidence: plan.evidence.clone(),
                        actor,
                        message,
                        confirm_old_agent_cannot_act: consent.agent_gone,
                        confirm_resources_safe: consent.agent_gone,
                    },
                )?;
                Ok(None)
            }
            UnstickStepKind::ResetRemediation => {
                self.reset_task_remediation(record, &actor, &message)?;
                Ok(None)
            }
            UnstickStepKind::ResetContinuations => {
                let check = step.check_id.ok_or_else(|| {
                    RegistrationError::new("unstick-invalid", "step has no Check")
                })?;
                self.reset_check_continuations(record, check, &actor, &message)?;
                Ok(None)
            }
            UnstickStepKind::Resume => {
                self.resume_task_dispatch(record)?;
                Ok(None)
            }
            UnstickStepKind::Dispatch => Ok(Some(self.dispatch_record(record)?.0)),
        }
    }

    #[allow(clippy::too_many_lines)] // One linear runbook reads best in order.
    fn plan_unstick(
        &self,
        record: u64,
        consent: &UnstickConsent,
    ) -> Result<Plan, RegistrationError> {
        let dispatch = self.dispatch()?;
        let task = dispatch.show_record(record)?;
        let done = |verdict, summary: String, steps| Plan {
            report: UnstickReport {
                record_id: record,
                verdict,
                summary,
                applied: false,
                steps,
                assignment_id: None,
            },
            salvage: None,
        };
        if task.record_type != RecordType::Task {
            return Err(RegistrationError::new(
                "unsupported-record-type",
                "unstick supports only Tasks",
            ));
        }
        if task.status == RecordStatus::Done {
            return Ok(done(
                UnstickVerdict::NothingToDo,
                format!("Task {record} is Done."),
                Vec::new(),
            ));
        }
        if let Err(error) = crate::validate_task_dispatch_mode(&task)
            .and_then(|()| dispatch.validate_task_execution_plan(record))
        {
            return Ok(done(
                UnstickVerdict::Stopped,
                format!(
                    "{}: {} Retain the evidence and follow the Spec recovery runbook; unstick never repins or resets.",
                    error.code, error.message
                ),
                Vec::new(),
            ));
        }

        let assignments = dispatch.list_agent_assignments(record)?;
        let hold = dispatch.dispatch_hold(record)?;
        let mut steps = Vec::new();
        let mut resolve = Vec::new();
        let mut alive = None;
        for (index, assignment) in assignments.iter().enumerate() {
            let info = dispatch.agent_assignment_info(&assignment.id)?;
            let open = assignment.closed_at_millis.is_none();
            if open && index == 0 {
                match assignment.state {
                    AgentAssignmentState::Provisioning
                        if info.launch_intent.as_ref().is_none_or(|intent| {
                            now_millis() < intent.acknowledgement_deadline_millis
                        }) =>
                    {
                        return Ok(done(
                            UnstickVerdict::Launching,
                            format!(
                                "Assignment {} is still launching within its acknowledgement deadline; wait, then re-run.",
                                assignment.id
                            ),
                            Vec::new(),
                        ));
                    }
                    state if agent_active(state) && !consent.retire_working => {
                        alive = Some(assignment.id.clone());
                        continue;
                    }
                    _ => {}
                }
            }
            if open || has_known_unretired(&info) {
                let mut retire = step(
                    UnstickStepKind::Retire,
                    if open && agent_active(assignment.state) {
                        "operator asked to retire an agent still reported active"
                    } else if open {
                        "agent missed its acknowledgement deadline or its state is unknown"
                    } else {
                        "closed attempt still holds provider resources"
                    },
                );
                retire.assignment_id = Some(assignment.id.clone());
                if open && agent_active(assignment.state) {
                    retire = retire.requiring(consent, &[UnstickFlag::RetireWorking]);
                }
                steps.push(retire);
            }
            if has_uncertain_provisioning(&info) && info.unknown_resource_resolution.is_none() {
                let mut unknown = step(
                    UnstickStepKind::ResolveUnknown,
                    "a crash left a provisioning marker without a resource identity; confirm the old agent cannot act and its resources are safe",
                );
                unknown.assignment_id = Some(assignment.id.clone());
                resolve.push(unknown.requiring(consent, &[UnstickFlag::AgentGone]));
            }
        }
        steps.extend(resolve);

        let work = self.record_work_state(record)?;
        let mut salvage = None;
        if alive.is_none()
            && task.status == RecordStatus::InProgress
            && let Some(latest) = assignments.first()
            && !work
                .recoveries
                .iter()
                .any(|r| r.request.source_assignment == latest.id)
            && self.managed_worktrees.is_some()
            && self.managed_worktrees()?.worktree_claim(record)?.is_some()
        {
            let project = dispatch.project(&task.project_slugs[0])?;
            match self.observe_retained_evidence(record, &project) {
                Ok((evidence, dirty))
                    if dirty
                        || (Some(&evidence) != work.input.as_ref()
                            && Some(&evidence) != work.output.as_ref()) =>
                {
                    let mut keep = step(
                        UnstickStepKind::Salvage,
                        if dirty {
                            "checkout has uncommitted work the old agent never reported"
                        } else {
                            "checkout HEAD has commits the old agent never reported"
                        },
                    );
                    keep.assignment_id = Some(latest.id.clone());
                    steps.push(keep.requiring(
                        consent,
                        &[
                            UnstickFlag::KeepWork,
                            UnstickFlag::AgentGone,
                            UnstickFlag::Actor,
                            UnstickFlag::Message,
                        ],
                    ));
                    salvage = Some(SalvagePlan {
                        generation: work.generation,
                        evidence,
                    });
                }
                Ok(_) => {}
                Err(error) => {
                    let mut blocked = step(
                        UnstickStepKind::Salvage,
                        "retained checkout could not be observed; preserve it and investigate",
                    );
                    blocked.status = UnstickStepStatus::Blocked;
                    blocked.error = Some(format!("{}: {}", error.code, error.message));
                    steps.push(blocked);
                }
            }
        }

        let budget_flags = [
            UnstickFlag::ResetBudgets,
            UnstickFlag::Actor,
            UnstickFlag::Message,
        ];
        if let Some(reason) = &work.progress.stop_reason {
            steps.push(
                step(UnstickStepKind::ResetRemediation, reason.clone())
                    .requiring(consent, &budget_flags),
            );
        }
        for continuations in work.progress.continuations.iter().filter(|c| c.exhausted()) {
            let mut reset = step(
                UnstickStepKind::ResetContinuations,
                format!(
                    "Check {} exhausted its continuations ({} total, {} unchanged in a row)",
                    continuations.check_id,
                    continuations.total,
                    continuations.consecutive_unchanged
                ),
            );
            reset.check_id = Some(continuations.check_id);
            steps.push(reset.requiring(consent, &budget_flags));
        }

        let retiring = steps.iter().any(|s| s.kind == UnstickStepKind::Retire);
        if hold.is_some() || retiring {
            steps.push(step(
                UnstickStepKind::Resume,
                match &hold {
                    Some(hold) => format!("clear Dispatch Hold: {}", hold.reason),
                    None => "clear the hold retirement installs".into(),
                },
            ));
        }
        if alive.is_none() {
            steps.push(step(
                UnstickStepKind::Dispatch,
                "launch the next attempt through normal dispatch gates",
            ));
        }

        let (verdict, summary) = if steps.iter().any(|s| s.status == UnstickStepStatus::Blocked) {
            (
                UnstickVerdict::Stopped,
                "Recovery cannot proceed automatically; investigate the blocked step.".to_string(),
            )
        } else if steps
            .iter()
            .any(|s| s.status == UnstickStepStatus::NeedsFlag)
        {
            (
                UnstickVerdict::NeedsFlag,
                "Some steps need an explicit operator assertion; re-run with the listed flags only when they are true.".to_string(),
            )
        } else if let (Some(id), true) = (&alive, steps.is_empty()) {
            (
                UnstickVerdict::Healthy,
                format!("Assignment {id} is active and nothing blocks it."),
            )
        } else {
            (
                UnstickVerdict::Stuck,
                format!("{} recovery step(s) apply.", steps.len()),
            )
        };
        Ok(Plan {
            report: UnstickReport {
                record_id: record,
                verdict,
                summary,
                applied: false,
                steps,
                assignment_id: alive,
            },
            salvage,
        })
    }
}
