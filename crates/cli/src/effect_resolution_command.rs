use clap::{Args, ValueEnum};
use forge_rpc::effect_resolution::{ExternalOutcome, InterruptedEffect, Resolution};

#[cfg(test)]
#[path = "effect_resolution_command_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Conclusion {
    Applied,
    NotApplied,
}

#[derive(Debug, Args)]
#[command(
    after_long_help = "Trusted operator assertion, not authenticated human identity or automatic proof of process death. First revoke the Assignment, inspect the external outcome and make old execution/external children safe. The daemon requires exclusive ownership and matched committed inspection. Use effect identities from assignment-info. Never put credentials, prompts or tool arguments/results in evidence/message fields. Unknown outcomes must remain unresolved. Exact retries require the same operation ID and all original evidence. Resolution never replays a command, repairs runtime receipts, adopts dirty commits, clears unrelated holds/resources, or authorizes recovery."
)]
pub struct ResolveEffectArgs {
    assignment_id: String,
    /// Stable resolution identity; reuse only with identical evidence and conclusion.
    #[arg(long)]
    operation_id: String,
    /// Interrupted runtime execution-task identity (not a Forge Task ID).
    #[arg(long)]
    execution_task: String,
    #[arg(long)]
    conversation: String,
    #[arg(long)]
    call: String,
    /// Evidence-supported external conclusion; unknown cannot be resolved.
    #[arg(long, value_enum)]
    outcome: Conclusion,
    #[arg(long)]
    actor: String,
    /// Bounded reference to external evidence, not protected tool payloads.
    #[arg(long)]
    evidence: String,
    #[arg(long)]
    message: String,
    #[arg(long)]
    confirm_old_agent_cannot_act: bool,
    #[arg(long)]
    confirm_external_work_safe: bool,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}

pub async fn run(args: ResolveEffectArgs, workspace: Option<String>) -> i32 {
    let resolution = Resolution {
        operation_id: args.operation_id,
        effect: InterruptedEffect {
            task_id: args.execution_task,
            conversation_id: args.conversation,
            call_id: args.call,
        },
        outcome: match args.outcome {
            Conclusion::Applied => ExternalOutcome::Applied,
            Conclusion::NotApplied => ExternalOutcome::NotApplied,
        },
        actor: args.actor,
        evidence: args.evidence,
        message: args.message,
        confirm_old_agent_cannot_act: args.confirm_old_agent_cannot_act,
        confirm_external_work_safe: args.confirm_external_work_safe,
    };
    if let Err(error) = resolution.validate() {
        eprintln!(
            "forge dispatcher assignment-resolve-effect: {}. Investigate first; both --confirm-old-agent-cannot-act and --confirm-external-work-safe are required. Unknown outcomes must remain unresolved.",
            error.message
        );
        return 1;
    }
    let endpoint = super::resolve_endpoint(args.server_url.as_deref()).await;
    match forge_cli::request_resolve_interrupted_effect(
        (endpoint.as_str(), workspace.as_deref()),
        forge_rpc::ResolveInterruptedEffectRequest {
            assignment_id: args.assignment_id,
            resolution,
        },
    )
    .await
    {
        Ok(receipt) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string(&receipt).expect("typed resolution receipt serializes")
                );
            } else {
                println!(
                    "Recorded effect resolution {} at {}. No command replay or recovery is authorized; holds and other resources remain.",
                    receipt.resolution.operation_id, receipt.accepted_at_millis
                );
            }
            0
        }
        Err(error) => {
            eprintln!("forge dispatcher assignment-resolve-effect: {error}");
            1
        }
    }
}
