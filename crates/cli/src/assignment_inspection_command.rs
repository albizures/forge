//! Read-only `dispatcher` commands: persisted durable status and committed transcript pages.
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
