use clap::{Args, Parser, Subcommand};
use forge_cli::{
    render_human_status, render_json_status, render_project_here, render_project_registered,
    render_record, render_record_list, render_record_opened, request_done_record,
    request_edit_record, request_list_records, request_open_record, request_project_here,
    request_register_project, request_relate_record, request_show_record, request_start_record,
    request_status, request_unrelate_record, resolve_endpoint as resolve_managed_endpoint,
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
    /// List Records by optional Project Scope membership and status.
    List(ListRecordsArgs),
    /// Edit a non-Done Record Body.
    Edit(EditRecordArgs),
    /// Advance an open Record to in-progress.
    Start(RecordIdArgs),
    /// Complete an in-progress Record.
    Done(DoneRecordArgs),
    /// Link a child Record to a parent with `--kind child-of`.
    Relate(RelationshipArgs),
    /// Remove a child Record's link to a parent with `--kind child-of`.
    Unrelate(RelationshipArgs),
}
#[derive(Args, Debug)]
struct ListRecordsArgs {
    /// Restrict results to Records whose Project Scope includes this Project.
    #[arg(long, value_name = "SLUG")]
    project: Option<String>,
    /// Restrict results to open, in-progress, or done Records.
    #[arg(long, value_parser = ["open", "in-progress", "done"])]
    status: Option<String>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
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
struct RelationshipArgs {
    child_id: u64,
    #[arg(long, value_parser = ["child-of"])]
    kind: String,
    #[arg(long = "to")]
    parent_id: u64,
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
    if print_build_info_if_requested() {
        return;
    }
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
                request_show_record(&resolve_endpoint(args.server_url.as_deref()).await, args.id)
                    .await,
            ),
            RecordCommand::List(args) => list_records(args).await,
            RecordCommand::Edit(args) => record_result(
                "edit",
                args.json,
                request_edit_record(
                    &resolve_endpoint(args.server_url.as_deref()).await,
                    args.id,
                    &args.body,
                )
                .await,
            ),
            RecordCommand::Start(args) => record_result(
                "start",
                args.json,
                request_start_record(&resolve_endpoint(args.server_url.as_deref()).await, args.id)
                    .await,
            ),
            RecordCommand::Done(args) => record_result(
                "done",
                args.json,
                request_done_record(
                    &resolve_endpoint(args.server_url.as_deref()).await,
                    args.id,
                    args.resolution,
                )
                .await,
            ),
            RecordCommand::Relate(args) => record_result(
                "relate",
                args.json,
                request_relate_record(
                    &resolve_endpoint(args.server_url.as_deref()).await,
                    args.child_id,
                    args.parent_id,
                    args.kind,
                )
                .await,
            ),
            RecordCommand::Unrelate(args) => record_result(
                "unrelate",
                args.json,
                request_unrelate_record(
                    &resolve_endpoint(args.server_url.as_deref()).await,
                    args.child_id,
                    args.parent_id,
                    args.kind,
                )
                .await,
            ),
        },
    };
    std::process::exit(exit_code);
}
async fn resolve_endpoint(flag: Option<&str>) -> String {
    match resolve_managed_endpoint(flag).await {
        Ok(endpoint) => endpoint,
        Err(error) => {
            eprintln!("forge: {error}");
            std::process::exit(1);
        }
    }
}

fn print_build_info_if_requested() -> bool {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() == 2
        && arguments.iter().any(|argument| argument == "--build-info")
        && arguments.iter().any(|argument| argument == "--json")
    {
        println!(
            "{}",
            serde_json::to_string(&forge_build_info::current()).expect("BuildInfo must serialize")
        );
        true
    } else {
        false
    }
}

async fn status(flag: Option<&str>, json: bool) -> i32 {
    match request_status(&resolve_endpoint(flag).await).await {
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
    match request_project_here(&resolve_endpoint(flag).await, &folder.to_string_lossy()).await {
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
    match request_register_project(&resolve_endpoint(flag).await, folder, slug).await {
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
async fn list_records(args: ListRecordsArgs) -> i32 {
    let status = args.status.map(|status| status.replace('-', "_"));
    match request_list_records(
        &resolve_endpoint(args.server_url.as_deref()).await,
        args.project,
        status,
    )
    .await
    {
        Ok(records) => {
            println!(
                "{}",
                if args.json {
                    serde_json::to_string(&records).expect("ListRecordsResponse must serialize")
                } else {
                    render_record_list(&records)
                }
            );
            0
        }
        Err(error) => {
            eprintln!("forge record list: {error}");
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
        &resolve_endpoint(args.server_url.as_deref()).await,
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

#[cfg(test)]
mod tests {
    use super::{Cli, Command, RecordCommand};
    use clap::Parser;

    #[test]
    fn build_info_is_shared_json_without_parsing_a_daemon_command() {
        let json = serde_json::to_value(forge_build_info::current()).unwrap();
        assert!(json["commit"].is_string());
        assert!(json["management_protocol_revision"].is_u64());
    }

    #[test]
    fn record_list_accepts_one_project_and_a_public_status_name() {
        let cli = Cli::try_parse_from([
            "forge",
            "record",
            "list",
            "--project",
            "alpha",
            "--status",
            "in-progress",
            "--json",
        ])
        .unwrap();
        let Command::Record {
            command: RecordCommand::List(args),
        } = cli.command
        else {
            panic!("expected record list command");
        };
        assert_eq!(args.project.as_deref(), Some("alpha"));
        assert_eq!(args.status.as_deref(), Some("in-progress"));
        assert!(args.json);
    }
}
