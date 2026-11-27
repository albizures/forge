//! One-command Task recovery over the documented Assignment recovery runbook.
use clap::Args;
use forge_rpc::{
    UnstickConsent, UnstickFlag, UnstickReport, UnstickRequest, UnstickStepKind, UnstickStepStatus,
    UnstickVerdict,
};

#[derive(Debug, Args)]
// Each bool is an independent command-line flag.
#[allow(clippy::struct_excessive_bools)]
pub struct UnstickArgs {
    /// Task to recover.
    task_id: u64,
    /// Perform the plan. Without it, only show which steps apply and why.
    #[arg(long)]
    apply: bool,
    /// Assert the old agent cannot act AND its known/unknown resources are safe.
    /// Only pass this after checking outside Forge; it is not proof of process exit.
    #[arg(long)]
    agent_gone: bool,
    /// Salvage the old agent's unreported commits or uncommitted work (never discarded).
    #[arg(long)]
    keep_work: bool,
    /// Reset exhausted remediation budgets (requires --actor and --message).
    #[arg(long)]
    reset_budgets: bool,
    /// Retire an agent that Forge still reports as active.
    #[arg(long)]
    retire_working: bool,
    /// Operator attribution for salvage and budget resets.
    #[arg(long)]
    actor: Option<String>,
    /// Explanation recorded with salvage and budget resets.
    #[arg(long)]
    message: Option<String>,
    #[arg(long)]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}

pub async fn run(args: UnstickArgs, workspace: Option<String>) -> i32 {
    let endpoint =
        super::selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await;
    let request = UnstickRequest {
        record_id: args.task_id,
        consent: UnstickConsent {
            agent_gone: args.agent_gone,
            keep_work: args.keep_work,
            reset_budgets: args.reset_budgets,
            retire_working: args.retire_working,
            actor: args.actor,
            message: args.message,
        },
    };
    match forge_cli::request_unstick(endpoint, request, args.apply).await {
        Ok(report) => {
            if args.json {
                match serde_json::to_string(&report) {
                    Ok(text) => println!("{text}"),
                    Err(error) => {
                        eprintln!("forge task unstick: {error}");
                        return 1;
                    }
                }
            } else {
                print!("{}", render(&report));
            }
            exit_code(report.verdict)
        }
        Err(error) => {
            eprintln!("forge task unstick: {error}");
            1
        }
    }
}

/// 0: nothing blocks progress (or a plan is ready); 2: waiting on the operator or a launch;
/// 1: recovery stopped or failed.
fn exit_code(verdict: UnstickVerdict) -> i32 {
    match verdict {
        UnstickVerdict::Healthy
        | UnstickVerdict::NothingToDo
        | UnstickVerdict::Stuck
        | UnstickVerdict::Unstuck => 0,
        UnstickVerdict::NeedsFlag | UnstickVerdict::Launching => 2,
        UnstickVerdict::Stopped | UnstickVerdict::Failed => 1,
    }
}

fn flag(flag: UnstickFlag) -> &'static str {
    match flag {
        UnstickFlag::AgentGone => "--agent-gone",
        UnstickFlag::KeepWork => "--keep-work",
        UnstickFlag::ResetBudgets => "--reset-budgets",
        UnstickFlag::RetireWorking => "--retire-working",
        UnstickFlag::Actor => "--actor <operator>",
        UnstickFlag::Message => "--message <explanation>",
    }
}

fn kind(kind: UnstickStepKind) -> &'static str {
    match kind {
        UnstickStepKind::Retire => "Retire old agent",
        UnstickStepKind::ResolveUnknown => "Resolve unknown resources",
        UnstickStepKind::Salvage => "Keep leftover work",
        UnstickStepKind::ResetRemediation => "Reset remediation budget",
        UnstickStepKind::Resume => "Clear Dispatch Hold",
        UnstickStepKind::Dispatch => "Dispatch",
    }
}

fn render(report: &UnstickReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let mode = if report.applied { "apply" } else { "plan" };
    let _ = writeln!(
        out,
        "Task {} ({mode}): {}",
        report.record_id, report.summary
    );
    for (index, step) in report.steps.iter().enumerate() {
        let status = match (step.status, report.applied) {
            (UnstickStepStatus::Pending, false) => "will run",
            (UnstickStepStatus::Pending, true) => "not reached",
            (UnstickStepStatus::Done, _) => "done",
            (UnstickStepStatus::NeedsFlag, _) => "needs flag",
            (UnstickStepStatus::Blocked, _) => "blocked",
            (UnstickStepStatus::Failed, _) => "failed",
        };
        let mut target = String::new();
        if let Some(id) = &step.assignment_id {
            let _ = write!(target, " [Assignment {id}]");
        }
        let _ = writeln!(
            out,
            "  {}. {}{target}: {status} — {}",
            index + 1,
            kind(step.kind),
            step.reason
        );
        if !step.required_flags.is_empty() {
            let flags: Vec<_> = step.required_flags.iter().map(|f| flag(*f)).collect();
            let _ = writeln!(out, "     requires: {}", flags.join(" "));
        }
        if let Some(error) = &step.error {
            let _ = writeln!(out, "     error: {error}");
        }
    }
    let needed: Vec<_> = {
        let mut flags: Vec<UnstickFlag> = Vec::new();
        for f in report.steps.iter().flat_map(|s| &s.required_flags) {
            if !flags.contains(f) {
                flags.push(*f);
            }
        }
        flags.into_iter().map(flag).collect()
    };
    if !needed.is_empty() {
        let _ = writeln!(
            out,
            "Only if true, re-run: forge task unstick {} --apply {}",
            report.record_id,
            needed.join(" ")
        );
    } else if !report.applied && report.verdict == UnstickVerdict::Stuck {
        let _ = writeln!(out, "Run: forge task unstick {} --apply", report.record_id);
    }
    if let Some(id) = report.assignment_id.as_ref().filter(|_| report.applied) {
        let _ = writeln!(out, "Inspect: forge dispatcher assignment-info {id}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_rpc::UnstickStep;

    fn report(applied: bool, steps: Vec<UnstickStep>, verdict: UnstickVerdict) -> UnstickReport {
        UnstickReport {
            record_id: 38,
            verdict,
            summary: "s".into(),
            applied,
            steps,
            assignment_id: None,
        }
    }

    #[test]
    fn plan_lists_missing_flags_once_in_rerun_command() {
        let needs = |kind| UnstickStep {
            kind,
            status: UnstickStepStatus::NeedsFlag,
            reason: "r".into(),
            assignment_id: Some("a1".into()),
            required_flags: vec![UnstickFlag::KeepWork, UnstickFlag::AgentGone],
            error: None,
        };
        let text = render(&report(
            false,
            vec![
                needs(UnstickStepKind::ResolveUnknown),
                needs(UnstickStepKind::Salvage),
            ],
            UnstickVerdict::NeedsFlag,
        ));
        assert!(text.contains("2. Keep leftover work [Assignment a1]: needs flag"));
        assert!(text.contains("re-run: forge task unstick 38 --apply --keep-work --agent-gone\n"));
        assert_eq!(exit_code(UnstickVerdict::NeedsFlag), 2);
    }

    #[test]
    fn stuck_plan_suggests_apply() {
        let text = render(&report(
            false,
            vec![UnstickStep {
                kind: UnstickStepKind::Dispatch,
                status: UnstickStepStatus::Pending,
                reason: "r".into(),
                assignment_id: None,
                required_flags: Vec::new(),
                error: None,
            }],
            UnstickVerdict::Stuck,
        ));
        assert!(text.contains("1. Dispatch: will run"));
        assert!(text.contains("Run: forge task unstick 38 --apply"));
    }
}
