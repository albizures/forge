use clap::{Args, Parser, Subcommand};
use forge_cli::{
    render_human_status, render_json_status, render_project_here, render_project_registered,
    render_record, render_record_opened, request_done_record, request_edit_record,
    request_open_record, request_project_here, request_register_project, request_show_record,
    request_start_record, request_status, server_url_from_process,
};
use std::io::Read;

#[derive(Debug, Parser)]
#[command(name = "forge", about = "Client for the Forge daemon")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Debug, Subcommand)]
enum Command {
    /// Print the daemon's shared application status.
    Status {
        #[arg(long, value_name = "URL")]
        server_url: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Register a Git main worktree as a Project through the daemon.
    Add {
        #[command(subcommand)]
        command: AddCommand,
    },
    /// Confirm the Project containing the current folder.
    Here {
        #[arg(long, value_name = "URL")]
        server_url: Option<String>,
    },
    /// Manage generic Records.
    Record {
        #[command(subcommand)]
        command: RecordCommand,
    },
}
#[derive(Debug, Subcommand)]
enum AddCommand {
    Project {
        folder: String,
        slug: String,
        #[arg(long, value_name = "URL")]
        server_url: Option<String>,
    },
}
#[derive(Debug, Subcommand)]
enum RecordCommand {
    /// Open a Record with an explicit Project Scope. Use --no-projects for an empty scope.
    Open(OpenRecordArgs),
    /// Show a Record.
    Show(RecordIdArgs),
    /// Edit a non-Done Record Body.
    Edit(EditRecordArgs),
    /// Advance an open Record to in-progress.
    Start(RecordIdArgs),
    /// Complete an in-progress Record.
    Done(DoneRecordArgs),
}
#[derive(Args, Debug)]
struct RecordIdArgs {
    id: u64,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct EditRecordArgs {
    id: u64,
    #[arg(long)]
    body: String,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct DoneRecordArgs {
    id: u64,
    #[arg(long)]
    resolution: Option<String>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("scope").required(true).args(["project", "no_projects"])))]
struct OpenRecordArgs {
    /// Markdown body, or - to read it from standard input.
    #[arg(long)]
    body: String,
    /// A Project in the Record's immutable Project Scope.
    #[arg(long = "project", value_name = "SLUG")]
    project: Vec<String>,
    /// Explicitly select no Projects.
    #[arg(long = "no-projects")]
    no_projects: bool,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let exit_code = match cli.command {
        Command::Status { server_url, json } => status(server_url.as_deref(), json).await,
        Command::Add {
            command:
                AddCommand::Project {
                    folder,
                    slug,
                    server_url,
                },
        } => add_project(&folder, &slug, server_url.as_deref()).await,
        Command::Here { server_url } => here(server_url.as_deref()).await,
        Command::Record { command } => match command {
            RecordCommand::Open(args) => open_record(args).await,
            RecordCommand::Show(args) => record_result(
                "show",
                args.json,
                request_show_record(
                    &server_url_from_process(args.server_url.as_deref()),
                    args.id,
                )
                .await,
            ),
            RecordCommand::Edit(args) => record_result(
                "edit",
                args.json,
                request_edit_record(
                    &server_url_from_process(args.server_url.as_deref()),
                    args.id,
                    &args.body,
                )
                .await,
            ),
            RecordCommand::Start(args) => record_result(
                "start",
                args.json,
                request_start_record(
                    &server_url_from_process(args.server_url.as_deref()),
                    args.id,
                )
                .await,
            ),
            RecordCommand::Done(args) => record_result(
                "done",
                args.json,
                request_done_record(
                    &server_url_from_process(args.server_url.as_deref()),
                    args.id,
                    args.resolution,
                )
                .await,
            ),
        },
    };
    std::process::exit(exit_code);
}
async fn status(flag: Option<&str>, json: bool) -> i32 {
    match request_status(&server_url_from_process(flag)).await {
        Ok(response) => {
            println!(
                "{}",
                if json {
                    render_json_status(&response)
                } else {
                    render_human_status(&response)
                }
            );
            0
        }
        Err(error) => {
            eprintln!("forge status: {error}");
            1
        }
    }
}
async fn here(flag: Option<&str>) -> i32 {
    let folder = match std::env::current_dir() {
        Ok(folder) => folder,
        Err(error) => {
            eprintln!("forge here: could not determine the current folder: {error}");
            return 1;
        }
    };
    match request_project_here(&server_url_from_process(flag), &folder.to_string_lossy()).await {
        Ok(project) => {
            println!("{}", render_project_here(&project));
            0
        }
        Err(error) => {
            eprintln!("forge here: {error}");
            1
        }
    }
}
async fn add_project(folder: &str, slug: &str, flag: Option<&str>) -> i32 {
    match request_register_project(&server_url_from_process(flag), folder, slug).await {
        Ok(project) => {
            println!("{}", render_project_registered(&project));
            0
        }
        Err(error) => {
            eprintln!("forge add project: {error}");
            1
        }
    }
}
fn record_result(
    operation: &str,
    json: bool,
    result: Result<forge_rpc::RecordResponse, String>,
) -> i32 {
    match result {
        Ok(record) => {
            println!(
                "{}",
                if json {
                    serde_json::to_string(&record).expect("RecordResponse must serialize")
                } else {
                    render_record(&record)
                }
            );
            0
        }
        Err(error) => {
            eprintln!("forge record {operation}: {error}");
            1
        }
    }
}

async fn open_record(args: OpenRecordArgs) -> i32 {
    let body = if args.body == "-" {
        let mut body = String::new();
        if let Err(error) = std::io::stdin().read_to_string(&mut body) {
            eprintln!("forge record open: could not read body: {error}");
            return 1;
        }
        body
    } else {
        args.body
    };
    match request_open_record(
        &server_url_from_process(args.server_url.as_deref()),
        &body,
        args.project,
    )
    .await
    {
        Ok(record) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string(&record).expect("RecordResponse must serialize")
                );
            } else {
                println!("{}", render_record_opened(&record));
            }
            0
        }
        Err(error) => {
            eprintln!("forge record open: {error}");
            1
        }
    }
}
