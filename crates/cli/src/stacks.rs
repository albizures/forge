//! Root execution configuration, derived Stack inspection and verified publication.
use clap::{Args, Subcommand};
use forge_rpc::{StackDestination, StackSource, TaskStackAction, TaskStackRequest};

#[derive(Debug, Args)]
pub struct StackArgs {
    #[command(subcommand)]
    command: StackCommand,
    #[arg(long, global = true)]
    server_url: Option<String>,
    #[arg(long, global = true)]
    json: bool,
}
#[derive(Debug, Subcommand)]
enum StackCommand {
    /// Show inherited destination, predecessor, pinned base and publication evidence.
    Show { id: u64 },
    /// Configure root execution inputs against any branch in the selected GitHub repository.
    Root {
        id: u64,
        #[arg(long)]
        repository: String,
        #[arg(long)]
        remote: String,
        #[arg(long)]
        base: String,
    },
    /// Rejected legacy declaration; use record relate --kind builds-on.
    #[command(hide = true)]
    After {
        #[arg(value_parser = reject_legacy_after)]
        id: u64,
        #[arg(long)]
        predecessor: u64,
    },
    /// Fetch authoritative branch state and pin the starting commit without creating a worktree.
    Prepare { id: u64 },
    /// Verify clean output at the exact prepared commit; submit the Publication outcome separately.
    PublishUnchanged { id: u64 },
    /// Independently verify an existing open non-draft PR for this Task.
    Publish {
        id: u64,
        #[arg(long)]
        url: String,
    },
}

fn reject_legacy_after(_: &str) -> Result<u64, String> {
    Err("task stack after is no longer supported; use forge record relate <task> --kind builds-on --to <predecessor>".into())
}

pub async fn run(args: StackArgs, workspace: Option<String>) -> i32 {
    let result = async {
        let (record_id, action) = match args.command {
            StackCommand::Show { id } => (id, TaskStackAction::Show),
            StackCommand::Root {
                id,
                repository,
                remote,
                base,
            } => (
                id,
                TaskStackAction::Configure {
                    source: StackSource::Root {
                        destination: StackDestination {
                            repository,
                            remote,
                            branch: base,
                        },
                    },
                },
            ),
            StackCommand::After { id, predecessor } => return Err(format!(
                "task stack after is no longer supported; use forge record relate {id} --kind builds-on --to {predecessor}"
            )),
            StackCommand::Prepare { id } => (id, TaskStackAction::Prepare),
            StackCommand::PublishUnchanged { id } => (id, TaskStackAction::PublishUnchanged),
            StackCommand::Publish { id, url } => (id, TaskStackAction::Publish { url }),
        };
        let endpoint = super::resolve_managed_endpoint(args.server_url.as_deref()).await?;
        forge_cli::request_task_stack(
            (endpoint.as_str(), workspace.as_deref()),
            TaskStackRequest { record_id, action },
        )
        .await
    }
    .await;
    render(result, args.json)
}

pub async fn assignment(url: Option<String>, unchanged: bool, json: bool) -> i32 {
    render(forge_cli::assignment_stack(url, unchanged).await, json)
}

fn render(result: Result<Option<forge_rpc::TaskStack>, String>, json: bool) -> i32 {
    match result {
        Ok(stack) => {
            if json {
                match serde_json::to_string_pretty(&stack) {
                    Ok(text) => println!("{text}"),
                    Err(error) => {
                        eprintln!("forge task stack: {error}");
                        return 1;
                    }
                }
            } else if let Some(stack) = stack {
                println!(
                    "Task {}: feat/forge-{} -> {} ({})",
                    stack.record_id,
                    stack.record_id,
                    stack.base_branch,
                    stack.destination.repository
                );
                if let Some(predecessor) = stack.predecessor {
                    println!("Stack Predecessor: {predecessor}");
                }
                if let Some(commit) = stack.base_commit {
                    println!("Base Commit: {commit}");
                }
                if let Some(result) = stack.unchanged_result {
                    println!(
                        "Verified unchanged result: {:?} (generation {})",
                        result.evidence, result.generation
                    );
                }
                if let Some(pr) = stack.publication {
                    println!("PR: {} at {}", pr.url, pr.head_commit);
                }
            } else {
                println!("Task is not stacked");
            }
            0
        }
        Err(error) => {
            eprintln!("forge task stack: {error}");
            1
        }
    }
}
