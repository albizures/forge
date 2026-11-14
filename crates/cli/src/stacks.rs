//! Explicit stack membership and verified publication commands.
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
    /// Start a stack against any branch in the selected GitHub repository.
    Root {
        id: u64,
        #[arg(long)]
        repository: String,
        #[arg(long)]
        remote: String,
        #[arg(long)]
        base: String,
    },
    /// Stack an unprepared Task on another explicitly stacked Task.
    After {
        id: u64,
        #[arg(long)]
        predecessor: u64,
    },
    /// Fetch authoritative branch state and pin the starting commit without creating a worktree.
    Prepare { id: u64 },
    /// Independently verify an existing open non-draft PR for this Task.
    Publish {
        id: u64,
        #[arg(long)]
        url: String,
    },
}

pub async fn run(args: StackArgs, workspace: Option<String>) -> i32 {
    let result = async {
        let endpoint = super::resolve_managed_endpoint(args.server_url.as_deref()).await?;
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
            StackCommand::After { id, predecessor } => (
                id,
                TaskStackAction::Configure {
                    source: StackSource::Predecessor {
                        record_id: predecessor,
                    },
                },
            ),
            StackCommand::Prepare { id } => (id, TaskStackAction::Prepare),
            StackCommand::Publish { id, url } => (id, TaskStackAction::Publish { url }),
        };
        forge_cli::request_task_stack(
            (endpoint.as_str(), workspace.as_deref()),
            TaskStackRequest { record_id, action },
        )
        .await
    }
    .await;
    render(result, args.json)
}

pub async fn assignment(url: Option<String>, json: bool) -> i32 {
    render(forge_cli::assignment_stack(url).await, json)
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
                if let Some(commit) = stack.base_commit {
                    println!("Base Commit: {commit}");
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
