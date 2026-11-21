//! `dispatcher` commands: read-only durable status/transcripts and idempotent follow-up recording.
use clap::Args;

#[cfg(test)]
#[path = "assignment_inspection_command_tests.rs"]
mod tests;

#[derive(Debug, Args)]
#[command(
    after_long_help = "Examples:\n  forge dispatcher assignment-status <assignment-id>\n  forge dispatcher assignment-status <assignment-id> --workspace owner --json\n\nReports persisted Forge facts only. It starts no runner, scheduler, model call, tool call or lease, and does not prove a process is running or absent. Missing Outcome and uncertainty keep capacity reserved."
)]
pub struct StatusArgs {
    assignment_id: String,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    /// Print the typed response as one JSON object on stdout.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(
    after_long_help = "Examples:\n  forge dispatcher assignment-transcript <assignment-id>\n  forge dispatcher assignment-transcript <assignment-id> --after 42 --limit 100 --json\n\nPages are committed entries in ascending immutable ID order. Pass `next_after` from the previous page as --after. A live Assignment may append later, so an empty page is not end of history. Unavailable or corrupt storage is reported, never repaired or shown as an empty transcript. Entry content is bounded runtime data; operators should treat it as sensitive."
)]
pub struct TranscriptArgs {
    assignment_id: String,
    /// Exclusive committed entry ID already seen; omit to start from the first entry.
    #[arg(long)]
    after: Option<String>,
    /// Entries per page, 1 to 100 (default 50).
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=100))]
    limit: Option<u32>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    /// Print the typed page as one JSON object on stdout.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(
    after_long_help = "Examples:\n  forge dispatcher assignment-followup <assignment-id> --operation-id fix-1 --message 'state the result with the Outcome tool'\n  forge dispatcher assignment-followup <assignment-id> --operation-id fix-1 --message-file note.txt --json\n\nRecords operator input bound to the Assignment's original attempt, Check, profile and evidence; nothing can be rebound. Reuse the same --operation-id to retry after an uncertain reply: the original record is returned, never a second one. Reusing it with different text is rejected. Revoked Assignments and retained interrupted-effect/authorization/recovery/retirement uncertainty reject new input. A record is not an Outcome, lease or proof of delivery. Scope or evidence changes need Forge's invalidation and replacement workflow."
)]
pub struct FollowUpArgs {
    assignment_id: String,
    /// Stable identity for safe retries (letters, digits, - _ . :; up to 128 bytes).
    #[arg(long, value_name = "ID")]
    operation_id: String,
    /// Message text (up to 16 KiB).
    #[arg(
        long,
        conflicts_with = "message_file",
        required_unless_present = "message_file"
    )]
    message: Option<String>,
    /// Read the message from a file instead.
    #[arg(long, value_name = "PATH")]
    message_file: Option<std::path::PathBuf>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    /// Print the typed receipt as one JSON object on stdout.
    #[arg(long)]
    json: bool,
}

pub async fn follow_up(args: FollowUpArgs, workspace: Option<String>) -> i32 {
    let message = match (&args.message, &args.message_file) {
        (Some(m), _) => m.clone(),
        (None, Some(path)) => {
            let Ok(text) = std::fs::read_to_string(path) else {
                eprintln!(
                    "forge dispatcher assignment-followup: could not read --message-file as UTF-8 text"
                );
                return 1;
            };
            text
        }
        (None, None) => unreachable!("clap requires a message source"),
    };
    let request = forge_rpc::FollowUpRequest {
        assignment_id: args.assignment_id,
        operation_id: args.operation_id,
        message,
    };
    if let Err(error) = request.validate() {
        eprintln!("forge dispatcher assignment-followup: {}", error.message);
        return 1;
    }
    let endpoint = super::resolve_endpoint(args.server_url.as_deref()).await;
    match forge_cli::request_assignment_follow_up(
        (endpoint.as_str(), workspace.as_deref()),
        request,
    )
    .await
    {
        Ok(receipt) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string(&receipt).expect("typed receipt serializes")
                );
            } else {
                println!("{}", forge_cli::render_follow_up(&receipt));
            }
            0
        }
        Err(error) => {
            eprintln!("forge dispatcher assignment-followup: {error}");
            1
        }
    }
}

pub async fn status(args: StatusArgs, workspace: Option<String>) -> i32 {
    let endpoint = super::resolve_endpoint(args.server_url.as_deref()).await;
    match forge_cli::request_assignment_durable_status(
        (endpoint.as_str(), workspace.as_deref()),
        args.assignment_id,
    )
    .await
    {
        Ok(status) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string(&status).expect("typed status serializes")
                );
            } else {
                println!("{}", forge_cli::render_durable_status(&status));
            }
            0
        }
        Err(error) => {
            eprintln!("forge dispatcher assignment-status: {error}");
            1
        }
    }
}

pub async fn transcript(args: TranscriptArgs, workspace: Option<String>) -> i32 {
    let request = forge_rpc::AssignmentTranscriptRequest {
        assignment_id: args.assignment_id,
        after: args.after,
        limit: args.limit,
    };
    if let Err(error) = request.validate() {
        eprintln!("forge dispatcher assignment-transcript: {}", error.message);
        return 1;
    }
    let endpoint = super::resolve_endpoint(args.server_url.as_deref()).await;
    match forge_cli::request_assignment_transcript(
        (endpoint.as_str(), workspace.as_deref()),
        request,
    )
    .await
    {
        Ok(page) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string(&page).expect("typed page serializes")
                );
            } else {
                println!("{}", forge_cli::render_transcript(&page));
            }
            0
        }
        Err(error) => {
            eprintln!("forge dispatcher assignment-transcript: {error}");
            1
        }
    }
}
