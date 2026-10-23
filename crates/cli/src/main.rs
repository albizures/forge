use clap::{Args, Parser, Subcommand};
use forge_cli::{
    render_agent_dispatch_status, render_human_status, render_json_status, render_project_here,
    render_project_registered, render_record, render_record_comments, render_record_list,
    render_record_opened, render_record_updates, request_add_record_comment,
    request_agent_dispatch_endpoint, request_agent_dispatch_profile,
    request_agent_dispatch_project, request_agent_dispatch_reconcile,
    request_agent_dispatch_status, request_delete_record_comment, request_done_record,
    request_edit_record, request_list_records, request_open_record, request_open_spec,
    request_open_task, request_open_task_under_spec, request_project_here, request_record_comments,
    request_record_updates, request_register_project, request_relate_record, request_show_record,
    request_start_record, request_status, request_unrelate_record,
    resolve_endpoint as resolve_managed_endpoint,
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
    /// Manage Specs.
    Spec {
        #[command(subcommand)]
        command: SpecCommand,
    },
    /// Manage Tasks.
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Configure and inspect daemon-owned Agent Dispatch.
    AgentDispatch {
        #[command(subcommand)]
        command: AgentDispatchCommand,
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
enum SpecCommand {
    /// Open a Spec with exactly one Project.
    Open(OpenSpecArgs),
}
#[derive(Debug, Subcommand)]
enum TaskCommand {
    /// Open a standalone Task with zero or one Project.
    Open(OpenTaskArgs),
    /// Open a Task beneath a Spec, inheriting the Spec's Project Scope.
    UnderSpec(OpenTaskUnderSpecArgs),
}
#[derive(Debug, Subcommand)]
enum AgentDispatchCommand {
    Endpoint(AgentDispatchEndpointArgs),
    Profile(AgentDispatchRequestArgs),
    Project(AgentDispatchProjectArgs),
    Status(AgentDispatchStatusArgs),
    Reconcile(AgentDispatchRequestArgs),
}
#[derive(Args, Debug)]
struct AgentDispatchEndpointArgs {
    /// Set this absolute Unix socket path. Omit to inspect current configuration.
    #[arg(long)]
    socket_path: Option<String>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct AgentDispatchProjectArgs {
    #[arg(long)]
    project: String,
    /// Set a limit from 1 through 1024; omit to clear the override.
    #[arg(long)]
    limit: Option<u16>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct AgentDispatchStatusArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct AgentDispatchRequestArgs {
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
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
    /// Add an immutable Comment to a Record.
    Comment(AddRecordCommentArgs),
    #[command(hide = true)]
    CommentDelete(DeleteRecordCommentArgs),
    /// List current Record Comments newest first.
    Comments(RecordIdArgs),
    /// List a Record's Forge-authored Update Log newest first.
    Updates(RecordIdArgs),
}
#[derive(Args, Debug)]
struct ListRecordsArgs {
    /// Restrict results to Records whose Project Scope includes this Project.
    #[arg(long, value_name = "SLUG")]
    project: Option<String>,
    /// Restrict results to open, in-progress, or done Records.
    #[arg(long, value_parser = ["open", "in-progress", "done"])]
    status: Option<String>,
    /// Restrict results to generic, spec, or task Records.
    #[arg(long = "type", value_parser = ["generic", "spec", "task"])]
    record_type: Option<String>,
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
struct AddRecordCommentArgs {
    id: u64,
    #[arg(long)]
    message: String,
    #[arg(long)]
    author: Option<String>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct DeleteRecordCommentArgs {
    record_id: u64,
    comment_id: u64,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
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
struct OpenSpecArgs {
    /// Markdown body, or - to read it from standard input.
    #[arg(long)]
    body: String,
    /// The Spec's required Project.
    #[arg(long, value_name = "SLUG")]
    project: String,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct OpenTaskArgs {
    /// Markdown body, or - to read it from standard input.
    #[arg(long)]
    body: String,
    /// The standalone Task's optional Project.
    #[arg(long, value_name = "SLUG")]
    project: Option<String>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct OpenTaskUnderSpecArgs {
    /// Markdown body, or - to read it from standard input.
    #[arg(long)]
    body: String,
    /// The parent Spec's ID.
    #[arg(long)]
    spec: u64,
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
    let cli = Cli::parse_from(normalize_comment_delete(std::env::args()));
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
            RecordCommand::Comment(args) => record_comment(args).await,
            RecordCommand::CommentDelete(args) => delete_record_comment(args).await,
            RecordCommand::Comments(args) => record_comments(args).await,
            RecordCommand::Updates(args) => record_updates(args).await,
        },
        Command::Spec { command } => match command {
            SpecCommand::Open(args) => open_spec(args).await,
        },
        Command::Task { command } => match command {
            TaskCommand::Open(args) => open_task(args).await,
            TaskCommand::UnderSpec(args) => open_task_under_spec(args).await,
        },
        Command::AgentDispatch { command } => agent_dispatch(command).await,
    };
    std::process::exit(exit_code);
}
async fn agent_dispatch(command: AgentDispatchCommand) -> i32 {
    let (result, json) = match command {
        AgentDispatchCommand::Endpoint(args) => (
            request_agent_dispatch_endpoint(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.socket_path,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        AgentDispatchCommand::Profile(args) => (
            request_agent_dispatch_profile(&resolve_endpoint(args.server_url.as_deref()).await)
                .await
                .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        AgentDispatchCommand::Project(args) => (
            request_agent_dispatch_project(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.project,
                args.limit,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        AgentDispatchCommand::Status(args) => (
            request_agent_dispatch_status(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.project,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        AgentDispatchCommand::Reconcile(args) => (
            request_agent_dispatch_reconcile(&resolve_endpoint(args.server_url.as_deref()).await)
                .await
                .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
    };
    match result {
        Ok(value) => {
            if json {
                println!("{value}");
            } else if let Ok(status) =
                serde_json::from_value::<forge_rpc::AgentDispatchStatusResponse>(value.clone())
            {
                println!("{}", render_agent_dispatch_status(&status));
            } else {
                println!("{value}");
            }
            0
        }
        Err(error) => {
            eprintln!("forge agent-dispatch: {error}");
            1
        }
    }
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
        args.record_type,
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

async fn record_comment(args: AddRecordCommentArgs) -> i32 {
    match request_add_record_comment(
        &resolve_endpoint(args.server_url.as_deref()).await,
        args.id,
        &args.message,
        args.author,
    )
    .await
    {
        Ok(comment) => {
            println!(
                "{}",
                if args.json {
                    serde_json::to_string(&comment).expect("RecordCommentResponse must serialize")
                } else {
                    format!("Added Comment {}", comment.id)
                }
            );
            0
        }
        Err(error) => {
            eprintln!("forge record comment: {error}");
            1
        }
    }
}
async fn delete_record_comment(args: DeleteRecordCommentArgs) -> i32 {
    match request_delete_record_comment(
        &resolve_endpoint(args.server_url.as_deref()).await,
        args.record_id,
        args.comment_id,
    )
    .await
    {
        Ok(()) => {
            println!("Deleted Comment {}", args.comment_id);
            0
        }
        Err(error) => {
            eprintln!("forge record comment delete: {error}");
            1
        }
    }
}
fn normalize_comment_delete(arguments: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut arguments = arguments.into_iter().collect::<Vec<_>>();
    if arguments
        .windows(3)
        .any(|window| window == ["record", "comment", "delete"])
    {
        for index in 0..arguments.len() - 2 {
            if arguments[index..index + 3] == ["record", "comment", "delete"] {
                arguments[index + 1] = "comment-delete".into();
                arguments.remove(index + 2);
                break;
            }
        }
    }
    arguments
}
async fn record_comments(args: RecordIdArgs) -> i32 {
    match request_record_comments(&resolve_endpoint(args.server_url.as_deref()).await, args.id)
        .await
    {
        Ok(comments) => {
            println!(
                "{}",
                if args.json {
                    serde_json::to_string(&comments)
                        .expect("ListRecordCommentsResponse must serialize")
                } else {
                    render_record_comments(&comments)
                }
            );
            0
        }
        Err(error) => {
            eprintln!("forge record comments: {error}");
            1
        }
    }
}
async fn record_updates(args: RecordIdArgs) -> i32 {
    match request_record_updates(&resolve_endpoint(args.server_url.as_deref()).await, args.id).await
    {
        Ok(updates) => {
            println!(
                "{}",
                if args.json {
                    serde_json::to_string(&updates)
                        .expect("ListRecordUpdatesResponse must serialize")
                } else {
                    render_record_updates(&updates)
                }
            );
            0
        }
        Err(error) => {
            eprintln!("forge record updates: {error}");
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

fn read_body(body: String, operation: &str) -> Result<String, i32> {
    if body != "-" {
        return Ok(body);
    }
    let mut contents = String::new();
    std::io::stdin()
        .read_to_string(&mut contents)
        .map_err(|error| {
            eprintln!("forge {operation}: could not read body: {error}");
            1
        })?;
    Ok(contents)
}

async fn open_record(args: OpenRecordArgs) -> i32 {
    let body = match read_body(args.body, "record open") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
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

async fn open_spec(args: OpenSpecArgs) -> i32 {
    let body = match read_body(args.body, "spec open") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    open_typed_result(
        "spec open",
        args.json,
        request_open_spec(
            &resolve_endpoint(args.server_url.as_deref()).await,
            &body,
            args.project,
        )
        .await,
    )
}

async fn open_task(args: OpenTaskArgs) -> i32 {
    let body = match read_body(args.body, "task open") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    open_typed_result(
        "task open",
        args.json,
        request_open_task(
            &resolve_endpoint(args.server_url.as_deref()).await,
            &body,
            args.project,
        )
        .await,
    )
}

async fn open_task_under_spec(args: OpenTaskUnderSpecArgs) -> i32 {
    let body = match read_body(args.body, "task under-spec") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    open_typed_result(
        "task under-spec",
        args.json,
        request_open_task_under_spec(
            &resolve_endpoint(args.server_url.as_deref()).await,
            &body,
            args.spec,
        )
        .await,
    )
}

fn open_typed_result(
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
                    render_record_opened(&record)
                }
            );
            0
        }
        Err(error) => {
            eprintln!("forge {operation}: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command, RecordCommand, SpecCommand, TaskCommand};
    use clap::Parser;

    #[test]
    fn build_info_is_shared_json_without_parsing_a_daemon_command() {
        let json = serde_json::to_value(forge_build_info::current()).unwrap();
        assert!(json["commit"].is_string());
        assert!(json["management_protocol_revision"].is_u64());
    }

    #[test]
    fn record_updates_accepts_json_output() {
        let cli = Cli::try_parse_from(["forge", "record", "updates", "42", "--json"]).unwrap();
        let Command::Record {
            command: RecordCommand::Updates(args),
        } = cli.command
        else {
            panic!("expected record updates command");
        };
        assert_eq!(args.id, 42);
        assert!(args.json);
    }

    #[test]
    fn typed_open_commands_require_their_valid_scope_or_parent_arguments() {
        let cli = Cli::try_parse_from([
            "forge",
            "spec",
            "open",
            "--body",
            "Spec",
            "--project",
            "alpha",
        ])
        .unwrap();
        let Command::Spec {
            command: SpecCommand::Open(args),
        } = cli.command
        else {
            panic!("expected spec open command");
        };
        assert_eq!(args.project, "alpha");

        let cli = Cli::try_parse_from(["forge", "task", "open", "--body", "Task"]).unwrap();
        let Command::Task {
            command: TaskCommand::Open(args),
        } = cli.command
        else {
            panic!("expected task open command");
        };
        assert!(args.project.is_none());
        assert!(
            Cli::try_parse_from([
                "forge",
                "task",
                "open",
                "--body",
                "Task",
                "--project",
                "alpha",
                "--project",
                "beta",
            ])
            .is_err()
        );

        let cli = Cli::try_parse_from([
            "forge",
            "task",
            "under-spec",
            "--body",
            "Child",
            "--spec",
            "42",
        ])
        .unwrap();
        let Command::Task {
            command: TaskCommand::UnderSpec(args),
        } = cli.command
        else {
            panic!("expected task under-spec command");
        };
        assert_eq!(args.spec, 42);
        assert!(Cli::try_parse_from(["forge", "task", "under-spec", "--body", "Child"]).is_err());
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
            "--type",
            "spec",
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
        assert_eq!(args.record_type.as_deref(), Some("spec"));
        assert!(args.json);
        assert!(Cli::try_parse_from(["forge", "record", "list", "--type", "invalid"]).is_err());
    }
}
