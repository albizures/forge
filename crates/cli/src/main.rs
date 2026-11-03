use clap::{Args, Parser, Subcommand};
use forge_cli::{
    render_agent_dispatch_status, render_human_status, render_json_status, render_managed_worktree,
    render_managed_worktrees, render_project_here, render_project_registered, render_record,
    render_record_comments, render_record_list, render_record_opened, render_record_updates,
    request_add_record_comment, request_agent_dispatch_endpoint, request_agent_dispatch_profile,
    request_agent_dispatch_project, request_agent_dispatch_reconcile,
    request_agent_dispatch_status, request_delete_record_comment, request_dispatch_grilling,
    request_done_record, request_edit_record, request_list_records,
    request_managed_worktree_create, request_managed_worktree_doctor,
    request_managed_worktree_info, request_managed_worktree_list, request_managed_worktree_remove,
    request_open_grilling_under_wayfinder, request_open_record, request_open_spec,
    request_open_task, request_open_task_under_spec, request_open_task_under_wayfinder,
    request_open_wayfinder, request_project_here, request_project_setup_hook,
    request_record_comments, request_record_updates, request_register_project,
    request_relate_record, request_reopen_dispatch_task, request_resume_dispatch_task,
    request_retire_agent_assignment, request_show_record, request_start_assignment_task,
    request_start_record, request_status, request_unrelate_record, request_worktree_root,
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
    /// Manage Wayfinders.
    Wayfinder {
        #[command(subcommand)]
        command: WayfinderCommand,
    },
    /// Manage Grillings.
    Grilling {
        #[command(subcommand)]
        command: GrillingCommand,
    },
    /// Manage Tasks.
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Configure managed Worktrees.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Configure a Project.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Manage Forge-owned Worktrees.
    Worktree {
        #[command(subcommand)]
        command: WorktreeCommand,
    },
    /// Configure and inspect daemon-owned Agent Dispatch.
    AgentDispatch {
        #[command(subcommand)]
        command: AgentDispatchCommand,
    },
}
#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Get(ConfigGetArgs),
    Set(ConfigSetArgs),
}
#[derive(Args, Debug)]
struct ConfigGetArgs {
    key: String,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct ConfigSetArgs {
    key: String,
    value: String,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Subcommand)]
enum ProjectCommand {
    SetupHook {
        #[command(subcommand)]
        command: ProjectSetupHookCommand,
    },
}
#[derive(Debug, Subcommand)]
enum ProjectSetupHookCommand {
    Set(ProjectSetupHookSetArgs),
    Clear(ProjectSetupHookClearArgs),
}
#[derive(Args, Debug)]
struct ProjectSetupHookSetArgs {
    #[arg(long)]
    project: String,
    hook: String,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct ProjectSetupHookClearArgs {
    #[arg(long)]
    project: String,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Subcommand)]
enum WorktreeCommand {
    Create(WorktreeCreateArgs),
    Info(WorktreeIdArgs),
    List(WorktreeListArgs),
    Remove(WorktreeRemoveArgs),
    Doctor(WorktreeDoctorArgs),
}
#[derive(Args, Debug)]
struct WorktreeCreateArgs {
    task: u64,
    #[arg(long)]
    branch: String,
    #[arg(long)]
    base: String,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct WorktreeIdArgs {
    task: u64,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct WorktreeListArgs {
    #[arg(long = "all")]
    include_all: bool,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct WorktreeRemoveArgs {
    task: u64,
    #[arg(long)]
    force: bool,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct WorktreeDoctorArgs {
    #[arg(long)]
    repair: bool,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
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
enum WayfinderCommand {
    /// Open a Wayfinder with one or more Projects.
    Open(OpenWayfinderArgs),
}
#[derive(Debug, Subcommand)]
enum GrillingCommand {
    /// Open a Grilling beneath a Wayfinder, inheriting or selecting its Project Scope.
    Open(OpenGrillingUnderWayfinderArgs),
}
#[derive(Debug, Subcommand)]
enum TaskCommand {
    /// Open a standalone Task with zero or one Project.
    Open(OpenTaskArgs),
    /// Open a Task beneath a Spec, inheriting the Spec's Project Scope.
    UnderSpec(OpenTaskUnderSpecArgs),
    /// Open a Task beneath a Wayfinder, selecting one scoped Project.
    UnderWayfinder(OpenTaskUnderWayfinderArgs),
}
#[derive(Debug, Subcommand)]
enum AgentDispatchCommand {
    Endpoint(AgentDispatchEndpointArgs),
    Profile(AgentDispatchRequestArgs),
    Project(AgentDispatchProjectArgs),
    Status(AgentDispatchStatusArgs),
    Reconcile(AgentDispatchRequestArgs),
    /// Dispatch one ready Grilling to an interactive Agent without a Managed Worktree.
    GrillingDispatch(DispatchGrillingArgs),
    /// Start the Task bound to an Assignment Capability.
    TaskStart(AssignmentTaskStartArgs),
    /// Return an unassigned In Progress Task to Open.
    TaskReopen(DispatchTaskArgs),
    /// Clear an operator-controlled Dispatch Hold.
    Resume(DispatchTaskArgs),
    /// Retire an Assignment after provider cleanup.
    AssignmentRetire(AssignmentRetireArgs),
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
#[derive(Args, Debug)]
struct DispatchGrillingArgs {
    grilling_id: u64,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct DispatchTaskArgs {
    task_id: u64,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct AssignmentTaskStartArgs {
    task_id: u64,
    #[arg(long)]
    assignment_id: String,
    #[arg(long)]
    capability_id: String,
    #[arg(long)]
    correlation_id: String,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct AssignmentRetireArgs {
    assignment_id: String,
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
    /// Link Records with `--kind child-of`, `--kind depends-on`, or `--kind generated-by`.
    Relate(RelationshipArgs),
    /// Remove a Record Relationship with `--kind child-of`, `--kind depends-on`, or `--kind generated-by`.
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
    /// Restrict results to a Record Type.
    #[arg(long = "type", value_parser = ["generic", "spec", "task", "wayfinder", "grilling"])]
    record_type: Option<String>,
    /// Restrict results to a Task Kind.
    #[arg(long = "kind", value_parser = ["research", "prototype", "review"])]
    task_kind: Option<String>,
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
    from_id: u64,
    #[arg(long, value_parser = ["child-of", "depends-on", "generated-by"])]
    kind: String,
    #[arg(long = "to")]
    to_id: u64,
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
    /// Record that generated this Record. Repeat for multiple generators.
    #[arg(long = "generated-by", value_name = "ID")]
    generated_by_ids: Vec<u64>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct OpenWayfinderArgs {
    /// Markdown body, or - to read it from standard input.
    #[arg(long)]
    body: String,
    /// A Project in the Wayfinder's immutable Project Scope.
    #[arg(long = "project", value_name = "SLUG", required = true)]
    project: Vec<String>,
    /// Record that generated this Record. Repeat for multiple generators.
    #[arg(long = "generated-by", value_name = "ID")]
    generated_by_ids: Vec<u64>,
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
    /// Optional immutable Task Kind metadata.
    #[arg(long = "kind", value_parser = ["research", "prototype", "review"])]
    task_kind: Option<String>,
    /// Record that generated this Record. Repeat for multiple generators.
    #[arg(long = "generated-by", value_name = "ID")]
    generated_by_ids: Vec<u64>,
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
    /// Optional immutable Task Kind metadata.
    #[arg(long = "kind", value_parser = ["research", "prototype", "review"])]
    task_kind: Option<String>,
    /// Record that generated this Record. Repeat for multiple generators.
    #[arg(long = "generated-by", value_name = "ID")]
    generated_by_ids: Vec<u64>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct OpenGrillingUnderWayfinderArgs {
    /// Markdown body, or - to read it from standard input.
    #[arg(long)]
    body: String,
    /// The parent Wayfinder's ID.
    #[arg(long)]
    wayfinder: u64,
    /// A Project in the Grilling's scope. Omit to inherit the whole Wayfinder scope.
    #[arg(long = "project", value_name = "SLUG")]
    project: Option<Vec<String>>,
    /// Record that generated this Record. Repeat for multiple generators.
    #[arg(long = "generated-by", value_name = "ID")]
    generated_by_ids: Vec<u64>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct OpenTaskUnderWayfinderArgs {
    /// Markdown body, or - to read it from standard input.
    #[arg(long)]
    body: String,
    /// The parent Wayfinder's ID.
    #[arg(long)]
    wayfinder: u64,
    /// The selected Project; inferred only for a single-Project Wayfinder.
    #[arg(long, value_name = "SLUG")]
    project: Option<String>,
    /// Optional immutable Task Kind metadata.
    #[arg(long = "kind", value_parser = ["research", "prototype", "review"])]
    task_kind: Option<String>,
    /// Record that generated this Record. Repeat for multiple generators.
    #[arg(long = "generated-by", value_name = "ID")]
    generated_by_ids: Vec<u64>,
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
    /// Record that generated this Record. Repeat for multiple generators.
    #[arg(long = "generated-by", value_name = "ID")]
    generated_by_ids: Vec<u64>,
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
                    args.from_id,
                    args.to_id,
                    args.kind,
                )
                .await,
            ),
            RecordCommand::Unrelate(args) => record_result(
                "unrelate",
                args.json,
                request_unrelate_record(
                    &resolve_endpoint(args.server_url.as_deref()).await,
                    args.from_id,
                    args.to_id,
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
        Command::Wayfinder { command } => match command {
            WayfinderCommand::Open(args) => open_wayfinder(args).await,
        },
        Command::Grilling { command } => match command {
            GrillingCommand::Open(args) => open_grilling_under_wayfinder(args).await,
        },
        Command::Task { command } => match command {
            TaskCommand::Open(args) => open_task(args).await,
            TaskCommand::UnderSpec(args) => open_task_under_spec(args).await,
            TaskCommand::UnderWayfinder(args) => open_task_under_wayfinder(args).await,
        },
        Command::Config { command } => config(command).await,
        Command::Project { command } => project(command).await,
        Command::Worktree { command } => worktree(command).await,
        Command::AgentDispatch { command } => agent_dispatch(command).await,
    };
    std::process::exit(exit_code);
}
async fn config(command: ConfigCommand) -> i32 {
    let (key, root, set, url, json) = match command {
        ConfigCommand::Get(args) => (args.key, None, false, args.server_url, args.json),
        ConfigCommand::Set(args) => (args.key, Some(args.value), true, args.server_url, args.json),
    };
    if key != "worktreeRoot" {
        eprintln!("forge config: unsupported key `{key}`; supported key: worktreeRoot");
        return 1;
    }
    match request_worktree_root(&resolve_endpoint(url.as_deref()).await, root, set).await {
        Ok(response) => {
            if json {
                println!("{}", serde_json::to_string(&response).unwrap());
            } else {
                println!("{}", response.root.as_deref().unwrap_or("unset"));
            }
            0
        }
        Err(error) => {
            eprintln!("forge config: {error}");
            1
        }
    }
}
async fn project(command: ProjectCommand) -> i32 {
    let ProjectCommand::SetupHook { command } = command;
    let (project, hook, server_url, json) = match command {
        ProjectSetupHookCommand::Set(args) => {
            (args.project, Some(args.hook), args.server_url, args.json)
        }
        ProjectSetupHookCommand::Clear(args) => (args.project, None, args.server_url, args.json),
    };
    match request_project_setup_hook(
        &resolve_endpoint(server_url.as_deref()).await,
        project,
        hook,
    )
    .await
    {
        Ok(()) => {
            if json {
                println!("{{}}");
            } else {
                println!("Updated Project Setup Hook");
            }
            0
        }
        Err(error) => {
            eprintln!("forge project setup-hook: {error}");
            1
        }
    }
}
async fn worktree(command: WorktreeCommand) -> i32 {
    let (result, json) = match command {
        WorktreeCommand::Create(args) => (
            request_managed_worktree_create(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.task,
                args.branch,
                args.base,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        WorktreeCommand::Info(args) => (
            request_managed_worktree_info(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.task,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        WorktreeCommand::List(args) => (
            request_managed_worktree_list(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.include_all,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        WorktreeCommand::Remove(args) => (
            request_managed_worktree_remove(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.task,
                args.force,
            )
            .await
            .map(|()| serde_json::Value::Null),
            args.json,
        ),
        WorktreeCommand::Doctor(args) => (
            request_managed_worktree_doctor(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.repair,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
    };
    match result {
        Ok(value) => {
            if json {
                println!("{value}");
            } else if let Ok(value) =
                serde_json::from_value::<forge_rpc::ManagedWorktreeResponse>(value.clone())
            {
                println!("{}", render_managed_worktree(&value));
            } else if let Ok(value) =
                serde_json::from_value::<forge_rpc::ListManagedWorktreesResponse>(value.clone())
            {
                println!("{}", render_managed_worktrees(&value));
            } else {
                println!("Removed Managed Worktree");
            }
            0
        }
        Err(error) => {
            eprintln!("forge worktree: {error}");
            1
        }
    }
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
        AgentDispatchCommand::GrillingDispatch(args) => (
            request_dispatch_grilling(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.grilling_id,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        AgentDispatchCommand::TaskStart(args) => (
            request_start_assignment_task(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.assignment_id,
                args.capability_id,
                args.correlation_id,
                args.task_id,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        AgentDispatchCommand::TaskReopen(args) => (
            request_reopen_dispatch_task(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.task_id,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        AgentDispatchCommand::Resume(args) => (
            request_resume_dispatch_task(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.task_id,
            )
            .await
            .map(|()| serde_json::Value::Null),
            args.json,
        ),
        AgentDispatchCommand::AssignmentRetire(args) => (
            request_retire_agent_assignment(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.assignment_id,
            )
            .await
            .map(|()| serde_json::Value::Null),
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
        args.task_kind,
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
        args.generated_by_ids,
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
            args.generated_by_ids,
        )
        .await,
    )
}

async fn open_wayfinder(args: OpenWayfinderArgs) -> i32 {
    let body = match read_body(args.body, "wayfinder open") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    open_typed_result(
        "wayfinder open",
        args.json,
        request_open_wayfinder(
            &resolve_endpoint(args.server_url.as_deref()).await,
            &body,
            args.project,
            args.generated_by_ids,
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
            args.task_kind,
            args.generated_by_ids,
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
            args.task_kind,
            args.generated_by_ids,
        )
        .await,
    )
}

async fn open_grilling_under_wayfinder(args: OpenGrillingUnderWayfinderArgs) -> i32 {
    let body = match read_body(args.body, "grilling open") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    open_typed_result(
        "grilling open",
        args.json,
        request_open_grilling_under_wayfinder(
            &resolve_endpoint(args.server_url.as_deref()).await,
            &body,
            args.wayfinder,
            args.project,
            args.generated_by_ids,
        )
        .await,
    )
}

async fn open_task_under_wayfinder(args: OpenTaskUnderWayfinderArgs) -> i32 {
    let body = match read_body(args.body, "task under-wayfinder") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    open_typed_result(
        "task under-wayfinder",
        args.json,
        request_open_task_under_wayfinder(
            &resolve_endpoint(args.server_url.as_deref()).await,
            &body,
            args.wayfinder,
            args.project,
            args.task_kind,
            args.generated_by_ids,
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
    use super::{Cli, Command, RecordCommand, SpecCommand, TaskCommand, WayfinderCommand};
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

        let cli = Cli::try_parse_from([
            "forge",
            "wayfinder",
            "open",
            "--body",
            "Map",
            "--project",
            "alpha",
            "--project",
            "beta",
        ])
        .unwrap();
        let Command::Wayfinder {
            command: WayfinderCommand::Open(args),
        } = cli.command
        else {
            panic!("expected wayfinder open command");
        };
        assert_eq!(args.project, ["alpha", "beta"]);

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

        let cli = Cli::try_parse_from([
            "forge",
            "task",
            "under-wayfinder",
            "--body",
            "Child",
            "--wayfinder",
            "43",
            "--project",
            "alpha",
        ])
        .unwrap();
        let Command::Task {
            command: TaskCommand::UnderWayfinder(args),
        } = cli.command
        else {
            panic!("expected task under-wayfinder command");
        };
        assert_eq!(args.wayfinder, 43);
        assert_eq!(args.project.as_deref(), Some("alpha"));
    }

    #[test]
    fn managed_worktree_commands_parse_the_public_contract() {
        assert!(
            Cli::try_parse_from(["forge", "config", "set", "worktreeRoot", "/tmp/worktrees"])
                .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "forge",
                "project",
                "setup-hook",
                "set",
                "--project",
                "alpha",
                "cargo test"
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "forge", "worktree", "create", "42", "--branch", "task-42", "--base", "main",
                "--json"
            ])
            .is_ok()
        );
        assert!(Cli::try_parse_from(["forge", "worktree", "info", "42"]).is_ok());
        assert!(Cli::try_parse_from(["forge", "worktree", "list", "--all"]).is_ok());
        assert!(Cli::try_parse_from(["forge", "worktree", "remove", "42", "--force"]).is_ok());
        assert!(Cli::try_parse_from(["forge", "worktree", "doctor", "--repair"]).is_ok());
        assert!(
            Cli::try_parse_from(["forge", "worktree", "create", "42", "--branch", "task-42"])
                .is_err()
        );
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
