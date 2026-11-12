mod agent_profiles;
mod assignment_check_commands;
mod assignment_finding_commands;
mod check_templates;
mod checks;
mod findings;
mod project_selection;
mod workspaces;

use clap::{Args, Parser, Subcommand};
use forge_cli::{
    render_agent_dispatch_status, render_human_status, render_json_status, render_managed_worktree,
    render_managed_worktrees, render_project_here, render_project_registered, render_record,
    render_record_comments, render_record_list, render_record_opened, render_record_updates,
    request_add_record_comment, request_agent_dispatch_profile, request_agent_dispatch_reconcile,
    request_agent_dispatch_status, request_delete_record_comment, request_dispatch_record,
    request_done_record, request_edit_record, request_list_records,
    request_managed_worktree_create, request_managed_worktree_doctor,
    request_managed_worktree_info, request_managed_worktree_list, request_managed_worktree_recover,
    request_managed_worktree_remove, request_open_grilling_under_wayfinder, request_open_record,
    request_open_spec, request_open_task, request_open_task_under_spec,
    request_open_task_under_wayfinder, request_open_wayfinder, request_project_setup_hook,
    request_record_comments, request_record_updates, request_register_project_selected,
    request_relate_record, request_reopen_dispatch_task, request_resume_dispatch_task,
    request_retire_agent_assignment, request_select_project, request_show_record,
    request_start_record, request_status, request_unrelate_record,
    resolve_endpoint as resolve_managed_endpoint,
};
use std::io::Read;

#[derive(Debug, Parser)]
#[command(
    name = "forge",
    about = "Client for the Forge daemon",
    after_long_help = "Workspace selection is routing, not authentication or OS isolation. Use --workspace or canonical client cwd; there is no saved default. Qualified Project references use <workspace>/<slug>; conflicting explicit selectors fail. No Herdr plugin or upstream change is required."
)]
struct Cli {
    /// Explicit Forge Workspace ID; overrides cwd inference. No saved default.
    #[arg(long, global = true)]
    workspace: Option<String>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Debug, Subcommand)]
enum Command {
    /// Manage Forge filesystem Workspace registrations and selection.
    Workspace(workspaces::WorkspaceArgs),
    /// Preview or explicitly register an immutable Agent Profile document.
    AgentProfile(agent_profiles::AgentProfileArgs),
    /// Print the daemon's shared application status.
    Status {
        #[arg(long, value_name = "URL")]
        server_url: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Register a main Git checkout within its Workspace (origin remote required).
    Add {
        #[command(subcommand)]
        command: AddCommand,
    },
    /// Select an explicit Project, otherwise the deepest containing Project root.
    Here {
        #[arg(long, value_name = "URL")]
        server_url: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Manage generic Records.
    Record {
        #[command(subcommand)]
        command: RecordCommand,
    },
    /// List workflow-ready work in the selected Workspace (no dispatch setup required).
    Ready(ReadyRecordsArgs),
    /// Manage Workspace-owned Initiatives.
    Initiative {
        #[command(subcommand)]
        command: InitiativeCommand,
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
    /// Configure a Project.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Manage Task checkouts at <workspace-root>/.forge/worktrees/<project-slug>/<record-id>.
    /// Select a Workspace explicitly or by cwd; placement cannot be overridden.
    Worktree {
        #[command(subcommand)]
        command: WorktreeCommand,
    },
    /// Configure Workspace dispatch selections and inspect daemon-owned Agent Dispatch.
    #[command(
        after_long_help = "Dispatch requires both Task and Grilling profile selections and an exclusive existing Herdr binding. Status distinguishes configured completeness from current dispatchability. Task cwd is its bound Managed Worktree; Grilling cwd is its Forge Workspace root. Binding changes are guarded by live Assignments and retained resources in that Workspace; socket changes apply the guard daemon-wide and invalidate every Workspace's provider verification. Profile changes affect future Assignments only. Planning remains available without dispatch configuration."
    )]
    Dispatcher {
        #[command(subcommand)]
        command: AgentDispatchCommand,
    },
    /// Agent-side Assignment bootstrap and authenticated Task start.
    Assignment {
        #[command(subcommand)]
        command: AssignmentCommand,
    },
}
#[derive(Debug, Subcommand)]
enum ProjectCommand {
    CheckTemplate {
        #[command(subcommand)]
        command: check_templates::CheckTemplateCommand,
    },
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
    /// Create a checkout at its fixed Workspace path for an Open single-Project Task.
    Create(WorktreeCreateArgs),
    /// Adopt an unclaimed checkout at the same fixed path after operator verification.
    Recover(WorktreeRecoverArgs),
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
struct WorktreeRecoverArgs {
    task: u64,
    #[arg(long, required = true)]
    confirm_no_active_agent: bool,
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
enum InitiativeCommand {
    /// Open an Initiative; Project scope may be empty or contain several Projects.
    Open(OpenWayfinderArgs),
}
#[derive(Args, Debug)]
#[allow(clippy::struct_excessive_bools)] // Independent workflow and output switches, not state.
struct ReadyRecordsArgs {
    #[arg(long)]
    planning: bool,
    #[arg(long)]
    include_hitl: bool,
    #[arg(long)]
    blocked: bool,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Subcommand)]
enum SpecCommand {
    /// Open a Spec with exactly one Project.
    Open(OpenSpecArgs),
}
#[derive(Debug, Subcommand)]
enum WayfinderCommand {
    /// Open a Wayfinder with zero or more Projects in the selected Workspace.
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
enum AssignmentCommand {
    /// Inspect and review only the assigned Task's Findings.
    Finding {
        #[command(subcommand)]
        command: assignment_finding_commands::Command,
    },
    /// Inspect, assess or invalidate only the assigned Task's Checks.
    Check {
        #[command(subcommand)]
        command: assignment_check_commands::Command,
    },
    /// Authenticate, acknowledge, start and fetch context before returning behavior.
    Bootstrap {
        #[arg(long)]
        json: bool,
    },
    /// Read the authenticated Assignment context.
    Context {
        #[arg(long)]
        json: bool,
    },
    /// Read an authorized Record.
    Record {
        id: u64,
        #[arg(long)]
        json: bool,
    },
    /// Complete the assigned Record; Grilling requires conversational confirmation.
    Done(AssignmentDoneArgs),
    /// Comment on the Record bound to this Assignment.
    Comment(AssignmentCommentArgs),
    /// Start the Record bound to this Assignment.
    Start {
        #[arg(long)]
        json: bool,
    },
    /// Read a bounded page of comments on an authorized Record.
    Comments(AssignmentPageArgs),
    /// Read a bounded page of history on an authorized Record.
    History(AssignmentPageArgs),
}
#[derive(Debug, Args)]
struct AssignmentDoneArgs {
    #[arg(long)]
    resolution: Option<String>,
    /// Confirm a human explicitly agreed in conversation (not independent authentication).
    #[arg(long, requires_all = ["confirmation", "agreed_outcome"])]
    confirmed: bool,
    #[arg(long, requires = "confirmed")]
    confirmation: Option<String>,
    #[arg(long, requires = "confirmed")]
    agreed_outcome: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("message_source").required(true).args(["message", "message_file"])))]
struct AssignmentCommentArgs {
    #[arg(long)]
    message: Option<String>,
    #[arg(long)]
    message_file: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
struct AssignmentPageArgs {
    id: u64,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Subcommand)]
enum AgentDispatchCommand {
    /// Inspect/change Workspace dispatch selections, daemon socket and scoped Project capacity.
    Config {
        #[command(subcommand)]
        command: DispatcherConfigCommand,
    },
    /// Inspect the Workspace-selected immutable Task or Grilling profile (read-only).
    Profile(AgentDispatchProfileArgs),
    /// Inspect configured completeness and probe current provider dispatchability.
    Status(AgentDispatchStatusArgs),
    /// Reconcile Assignments and provision eligible Tasks; not a read-only check.
    Reconcile(AgentDispatchRequestArgs),
    /// Dispatch an eligible Task or interactive Grilling; return an existing open Assignment.
    /// Readiness, capacity, Dispatch Holds, and recovery safety checks still apply.
    Dispatch(DispatchRecordArgs),
    /// Return an unassigned In Progress Task to Open.
    TaskReopen(DispatchTaskArgs),
    /// Clear an operator-controlled Dispatch Hold.
    Resume(DispatchTaskArgs),
    /// Inspect persisted Assignment evidence without contacting the provider.
    AssignmentInfo(AssignmentRetireArgs),
    /// Revoke an Assignment and verify provider cleanup; retains unresolved capacity and hold.
    AssignmentRetire(AssignmentRetireArgs),
    /// Assert unknown resources are safe after retirement (risk of duplicate agents).
    AssignmentResolveUnknown(AssignmentResolveUnknownArgs),
}
#[derive(Args, Debug)]
struct DispatcherConfigReadArgs {
    /// Configuration key. Omit to inspect Workspace selections, daemon socket and scoped capacity.
    key: Option<String>,
    #[arg(long)]
    project: Option<String>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Subcommand)]
enum DispatcherConfigCommand {
    /// Read effective values and default/override provenance.
    Get(DispatcherConfigReadArgs),
    /// Set an explicit override; workspace selection verifies an existing Herdr Workspace.
    Set(DispatcherConfigSetArgs),
    /// Remove an override when no unresolved Assignments/resources depend on it.
    Unset(DispatcherConfigUnsetArgs),
}
#[derive(Args, Debug)]
struct DispatcherConfigSetArgs {
    /// herdr-socket (daemon), workspace/task-profile/grilling-profile (Workspace), project-limit, or acknowledgement-timeout-ms (profile revision).
    key: String,
    /// Required configuration value.
    value: String,
    #[arg(long)]
    project: Option<String>,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Args, Debug)]
struct DispatcherConfigUnsetArgs {
    /// herdr-socket, workspace, task-profile, grilling-profile, project-limit, or acknowledgement-timeout-ms.
    key: String,
    #[arg(long)]
    project: Option<String>,
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
struct AgentDispatchProfileArgs {
    /// Workspace-selected immutable profile to inspect.
    #[arg(long, default_value = "task", value_parser = ["task", "grilling"])]
    kind: String,
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
struct DispatchRecordArgs {
    record_id: u64,
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
struct AssignmentRetireArgs {
    assignment_id: String,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct AssignmentResolveUnknownArgs {
    assignment_id: String,
    /// Confirm the old agent can no longer act, including outside Forge.
    #[arg(long)]
    confirm_old_agent_cannot_act: bool,
    /// Confirm unknown resources have been removed or otherwise made safe.
    #[arg(long)]
    confirm_unknown_resources_safe: bool,
    #[arg(long, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum RecordCommand {
    /// Retain review Findings and verification history (Tasks only).
    Finding {
        #[command(subcommand)]
        command: findings::FindingCommand,
    },
    /// Inspect and assess immutable completion criteria (Tasks only).
    Check {
        #[command(subcommand)]
        command: checks::CheckCommand,
    },
    /// Open a Record with an explicit Project Scope. Use --no-projects for an empty scope.
    Open(OpenRecordArgs),
    /// Show a Record.
    Show(RecordIdArgs),
    /// List Records by optional Project Scope membership and status.
    List(ListRecordsArgs),
    /// Edit a non-Done Record Body.
    Edit(EditRecordArgs),
    /// Explicitly change Execution Mode after safe ownership/resource handoff.
    ExecutionMode(ExecutionModeArgs),
    /// Advance an open Record to in-progress (Tasks require Manual mode).
    Start(RecordIdArgs),
    /// Complete an in-progress Record.
    Done(DoneRecordArgs),
    /// Link Records with child-of, depends-on, generated-by, or initiative membership.
    Relate(RelationshipArgs),
    /// Remove a child-of, depends-on, generated-by, or initiative Relationship.
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
struct ExecutionModeArgs {
    id: u64,
    #[arg(value_parser = ["manual", "dispatched"])]
    mode: String,
    #[arg(long, value_parser = ["manual", "dispatched"])]
    expected: String,
    #[arg(long)]
    actor: String,
    #[arg(long)]
    message: String,
    #[arg(long)]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
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
    #[arg(long = "type", value_parser = ["generic", "spec", "task", "wayfinder", "grilling", "initiative"])]
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
    from_id: u64,
    #[arg(long, value_parser = ["child-of", "depends-on", "generated-by", "initiative"])]
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
    #[arg(long = "project", value_name = "SLUG")]
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
    let mut cli = Cli::parse_from(normalize_comment_delete(std::env::args()));
    if let Err(error) = project_selection::qualify(&mut cli.command, cli.workspace.as_deref()).await
    {
        eprintln!("forge: {error}");
        std::process::exit(1);
    }
    std::process::exit(run(cli).await);
}

async fn run(cli: Cli) -> i32 {
    match cli.command {
        Command::Workspace(args) => workspaces::run(args, cli.workspace).await,
        Command::AgentProfile(args) => agent_profiles::run(args, cli.workspace).await,
        Command::Status { server_url, json } => status(server_url.as_deref(), json).await,
        Command::Add {
            command:
                AddCommand::Project {
                    folder,
                    slug,
                    server_url,
                },
        } => add_project(&folder, &slug, server_url.as_deref(), cli.workspace).await,
        Command::Here {
            server_url,
            project,
            json,
        } => here(server_url.as_deref(), cli.workspace, project, json).await,
        Command::Record { command } => run_record(command, cli.workspace).await,
        Command::Ready(args) => ready_records(args, cli.workspace).await,
        Command::Initiative {
            command: InitiativeCommand::Open(args),
        } => open_initiative(args, cli.workspace).await,
        Command::Spec { command } => match command {
            SpecCommand::Open(args) => open_spec(args, cli.workspace).await,
        },
        Command::Wayfinder { command } => match command {
            WayfinderCommand::Open(args) => open_wayfinder(args, cli.workspace).await,
        },
        Command::Grilling { command } => match command {
            GrillingCommand::Open(args) => open_grilling_under_wayfinder(args, cli.workspace).await,
        },
        Command::Task { command } => match command {
            TaskCommand::Open(args) => open_task(args, cli.workspace).await,
            TaskCommand::UnderSpec(args) => open_task_under_spec(args, cli.workspace).await,
            TaskCommand::UnderWayfinder(args) => {
                open_task_under_wayfinder(args, cli.workspace).await
            }
        },
        Command::Project { command } => project(command, cli.workspace).await,
        Command::Worktree { command } => worktree(command, cli.workspace).await,
        Command::Dispatcher { command } => agent_dispatch(command, cli.workspace).await,
        Command::Assignment { command } => assignment(command).await,
    }
}
async fn run_record(command: RecordCommand, workspace: Option<String>) -> i32 {
    match command {
        RecordCommand::Finding { command } => findings::run(command, workspace).await,
        RecordCommand::Check { command } => checks::run(command, workspace).await,
        RecordCommand::Open(args) => open_record(args, workspace).await,
        RecordCommand::List(args) => list_records(args, workspace).await,
        RecordCommand::Show(args) => record_result(
            "show",
            args.json,
            request_show_record(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.id,
            )
            .await,
        ),
        RecordCommand::Edit(args) => record_result(
            "edit",
            args.json,
            request_edit_record(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.id,
                &args.body,
            )
            .await,
        ),
        RecordCommand::ExecutionMode(args) => {
            let mode = |s: &str| {
                if s == "manual" {
                    forge_rpc::ExecutionMode::Manual
                } else {
                    forge_rpc::ExecutionMode::Dispatched
                }
            };
            record_result(
                "execution-mode",
                args.json,
                forge_cli::request_set_execution_mode(
                    selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref())
                        .await,
                    forge_rpc::SetExecutionModeRequest {
                        id: args.id,
                        expected: mode(&args.expected),
                        mode: mode(&args.mode),
                        actor: args.actor,
                        message: args.message,
                    },
                )
                .await,
            )
        }
        RecordCommand::Start(args) => record_result(
            "start",
            args.json,
            request_start_record(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.id,
            )
            .await,
        ),
        RecordCommand::Done(args) => record_result(
            "done",
            args.json,
            request_done_record(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.id,
                args.resolution,
            )
            .await,
        ),
        RecordCommand::Relate(args) => record_result(
            "relate",
            args.json,
            request_relate_record(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
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
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.from_id,
                args.to_id,
                args.kind,
            )
            .await,
        ),
        RecordCommand::Comment(args) => record_comment(args, workspace).await,
        RecordCommand::CommentDelete(args) => delete_record_comment(args, workspace).await,
        RecordCommand::Comments(args) => record_comments(args, workspace).await,
        RecordCommand::Updates(args) => record_updates(args, workspace).await,
    }
}

async fn project(command: ProjectCommand, workspace: Option<String>) -> i32 {
    let command = match command {
        ProjectCommand::CheckTemplate { command } => {
            return check_templates::run(command, workspace).await;
        }
        ProjectCommand::SetupHook { command } => command,
    };
    let (project, hook, server_url, json) = match command {
        ProjectSetupHookCommand::Set(args) => {
            (args.project, Some(args.hook), args.server_url, args.json)
        }
        ProjectSetupHookCommand::Clear(args) => (args.project, None, args.server_url, args.json),
    };
    match request_project_setup_hook(
        selected_record_endpoint(server_url.as_deref(), workspace.as_deref()).await,
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
async fn worktree(command: WorktreeCommand, workspace: Option<String>) -> i32 {
    let (result, json) = match command {
        WorktreeCommand::Create(args) => (
            request_managed_worktree_create(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.task,
                args.branch,
                args.base,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        WorktreeCommand::Recover(args) => (
            request_managed_worktree_recover(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.task,
                args.confirm_no_active_agent,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        WorktreeCommand::Info(args) => (
            request_managed_worktree_info(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.task,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        WorktreeCommand::List(args) => (
            request_managed_worktree_list(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.include_all,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        WorktreeCommand::Remove(args) => (
            request_managed_worktree_remove(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.task,
                args.force,
            )
            .await
            .map(|()| serde_json::Value::Null),
            args.json,
        ),
        WorktreeCommand::Doctor(args) => (
            request_managed_worktree_doctor(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
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

async fn assignment_comment_command(args: AssignmentCommentArgs) -> i32 {
    let body = match (args.message, args.message_file) {
        (Some(message), None) => Ok(message),
        (None, Some(file)) if file == "-" => {
            let mut body = String::new();
            std::io::stdin()
                .read_to_string(&mut body)
                .map(|_| body)
                .map_err(|_| "could not read comment")
        }
        (None, Some(file)) => {
            std::fs::read_to_string(file).map_err(|_| "could not read comment file")
        }
        _ => Err("exactly one comment source is required"),
    };
    let result = match body {
        Ok(body) => forge_cli::assignment_comment(&body).await,
        Err(error) => Err(error.into()),
    };
    render_assignment_result(result, args.json)
}

fn render_assignment_result(result: Result<serde_json::Value, String>, json: bool) -> i32 {
    match result {
        Ok(value) => {
            let rendered = if json {
                serde_json::to_string(&value)
            } else {
                serde_json::to_string_pretty(&value)
            };
            println!("{}", rendered.expect("response is serializable"));
            0
        }
        Err(error) => {
            eprintln!("forge assignment: {error}");
            1
        }
    }
}

async fn assignment(command: AssignmentCommand) -> i32 {
    let history = matches!(&command, AssignmentCommand::History(_));
    match command {
        AssignmentCommand::Check { command } => assignment_check_commands::run(command).await,
        AssignmentCommand::Finding { command } => assignment_finding_commands::run(command).await,
        AssignmentCommand::Done(args) => {
            let human = args.confirmation.zip(args.agreed_outcome);
            render_assignment_result(
                forge_cli::assignment_done(args.resolution, human).await,
                args.json,
            )
        }
        AssignmentCommand::Comment(args) => assignment_comment_command(args).await,
        AssignmentCommand::Start { json } => {
            render_assignment_result(forge_cli::assignment_start().await, json)
        }
        AssignmentCommand::Comments(args) | AssignmentCommand::History(args) => {
            match forge_cli::assignment_page(args.id, args.limit, args.cursor, history).await {
                Ok(page) => {
                    let rendered = if args.json {
                        serde_json::to_string(&page)
                    } else {
                        serde_json::to_string_pretty(&page)
                    };
                    println!("{}", rendered.expect("page is serializable"));
                    0
                }
                Err(error) => {
                    eprintln!("forge assignment: {error}");
                    1
                }
            }
        }
        AssignmentCommand::Bootstrap { json } => match forge_cli::assignment_bootstrap().await {
            Ok(response) => {
                let rendered = if json {
                    serde_json::to_string(&response)
                } else {
                    serde_json::to_string_pretty(&response)
                };
                println!("{}", rendered.expect("bootstrap is serializable"));
                0
            }
            Err(error) => {
                eprintln!("forge assignment bootstrap: {error}");
                1
            }
        },
        AssignmentCommand::Record { id, json } => {
            render_assignment_result(forge_cli::assignment_record(id).await, json)
        }
        AssignmentCommand::Context { json } => match forge_cli::assignment_context().await {
            Ok(context) => {
                let rendered = if json {
                    serde_json::to_string(&context)
                } else {
                    serde_json::to_string_pretty(&context)
                };
                println!("{}", rendered.expect("context is serializable"));
                0
            }
            Err(error) => {
                eprintln!("forge assignment context: {error}");
                1
            }
        },
    }
}

async fn agent_dispatch(command: AgentDispatchCommand, workspace: Option<String>) -> i32 {
    let (result, json) = match command {
        AgentDispatchCommand::AssignmentInfo(args) => {
            // Keep typed diagnostics separate from generic dispatch response rendering.
            return match forge_cli::request_agent_assignment_info(
                &resolve_endpoint(args.server_url.as_deref()).await,
                args.assignment_id,
            )
            .await
            {
                Ok(info) => {
                    if args.json {
                        println!("{}", serde_json::to_string(&info).unwrap());
                    } else {
                        println!("{}", forge_cli::render_agent_assignment_info(&info));
                    }
                    0
                }
                Err(error) => {
                    eprintln!("forge dispatcher assignment-info: {error}");
                    1
                }
            };
        }
        AgentDispatchCommand::Config { command } => {
            return dispatcher_config(command, workspace).await;
        }
        AgentDispatchCommand::Profile(args) => (
            request_agent_dispatch_profile(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.kind,
            )
            .await
            .map(|value| serde_json::to_value(value).unwrap()),
            args.json,
        ),
        AgentDispatchCommand::Status(args) => (
            request_agent_dispatch_status(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
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
        AgentDispatchCommand::Dispatch(args) => (
            request_dispatch_record(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.record_id,
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
        AgentDispatchCommand::Resume(args) => return dispatcher_resume(args).await,
        AgentDispatchCommand::AssignmentResolveUnknown(args) => {
            return dispatcher_resolve_unknown(args).await;
        }
        AgentDispatchCommand::AssignmentRetire(args) => return dispatcher_retire(args).await,
    };
    match result {
        Ok(value) => {
            if json {
                println!("{value}");
            } else if let Ok(status) =
                serde_json::from_value::<forge_rpc::AgentDispatchStatusResponse>(value.clone())
            {
                println!("{}", render_agent_dispatch_status(&status));
            } else if let Ok(dispatch) =
                serde_json::from_value::<forge_rpc::DispatchRecordResponse>(value.clone())
            {
                println!("{}", dispatch.message);
            } else {
                println!("{value}");
            }
            0
        }
        Err(error) => {
            eprintln!("forge dispatcher: {error}");
            1
        }
    }
}

async fn dispatcher_config(command: DispatcherConfigCommand, workspace: Option<String>) -> i32 {
    let (result, json) = match command {
        DispatcherConfigCommand::Get(args) => (
            forge_cli::request_dispatcher_config_get(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.key,
                args.project,
            )
            .await,
            args.json,
        ),
        DispatcherConfigCommand::Set(args) => (
            forge_cli::request_dispatcher_config_set(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.key,
                args.value,
                args.project,
            )
            .await,
            args.json,
        ),
        DispatcherConfigCommand::Unset(args) => (
            forge_cli::request_dispatcher_config_unset(
                selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
                args.key,
                args.project,
            )
            .await,
            args.json,
        ),
    };
    match result {
        Ok(config) => {
            if json {
                println!("{}", serde_json::to_string(&config).unwrap());
            } else {
                if let Some(applicability) = config.applicability {
                    println!("{applicability}");
                }
                for entry in config.entries {
                    println!(
                        "{}\t{}\t{}\t{}",
                        entry.key,
                        entry.value.as_deref().unwrap_or("unset"),
                        entry.provenance,
                        entry.ownership
                    );
                }
            }
            0
        }
        Err(error) => {
            eprintln!("forge dispatcher config: {error}");
            1
        }
    }
}

async fn dispatcher_resume(args: DispatchTaskArgs) -> i32 {
    match request_resume_dispatch_task(
        &resolve_endpoint(args.server_url.as_deref()).await,
        args.task_id,
    )
    .await
    {
        Ok(()) => {
            let warning = "Clearing a Dispatch Hold does not resolve recovery blockers or prove an agent is absent. It does not resend a prompt and does not authorize a replacement. Replacement requires retirement and safe resolution of all known and uncertain resources; hold clearing is separate. Use assignment-info <assignment-id> to investigate.";
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "task_id": args.task_id,
                        "operation": "clear-dispatch-hold",
                        "resolves_recovery_blockers": false,
                        "authorizes_replacement": false,
                        "warning": warning,
                    })
                );
            } else {
                println!("Task {}: Dispatch Hold cleared.\n{warning}", args.task_id);
            }
            0
        }
        Err(error) => {
            eprintln!("forge dispatcher resume: {error}");
            1
        }
    }
}

async fn dispatcher_resolve_unknown(args: AssignmentResolveUnknownArgs) -> i32 {
    if !args.confirm_old_agent_cannot_act || !args.confirm_unknown_resources_safe {
        eprintln!(
            "Risk of duplicate agents and external actions: confirm the old agent can no longer act AND unknown resources have been removed or otherwise made safe. Revocation cannot undo external actions. Investigate first, then pass both --confirm-old-agent-cannot-act and --confirm-unknown-resources-safe. Known resources still require verified retirement; Dispatch Hold clearing remains separate."
        );
        return 1;
    }
    let endpoint = resolve_endpoint(args.server_url.as_deref()).await;
    match forge_cli::request_resolve_unknown_agent_resources(
        &endpoint,
        args.assignment_id,
        args.confirm_old_agent_cannot_act,
        args.confirm_unknown_resources_safe,
    )
    .await
    {
        Ok(info) => {
            if args.json {
                println!("{}", serde_json::to_string(&info).unwrap());
            } else {
                println!("{}", forge_cli::render_agent_assignment_info(&info));
            }
            0
        }
        Err(error) => {
            eprintln!("forge dispatcher assignment-resolve-unknown: {error}");
            1
        }
    }
}

async fn dispatcher_retire(args: AssignmentRetireArgs) -> i32 {
    let endpoint = resolve_endpoint(args.server_url.as_deref()).await;
    match async {
        request_retire_agent_assignment(&endpoint, args.assignment_id.clone()).await?;
        forge_cli::request_agent_assignment_info(&endpoint, args.assignment_id).await
    }
    .await
    {
        Ok(info) => {
            if args.json {
                println!("{}", serde_json::to_string(&info).unwrap());
            } else {
                println!("{}", forge_cli::render_agent_assignment_info(&info));
                println!(
                    "Assignment access revoked. External actions already performed cannot be undone. Dispatch Hold clearing remains separate."
                );
            }
            0
        }
        Err(error) => {
            eprintln!(
                "forge dispatcher assignment-retire: {error}. Cleanup may be incomplete; inspect assignment-info before any replacement."
            );
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
async fn here(
    flag: Option<&str>,
    workspace: Option<String>,
    project: Option<String>,
    json: bool,
) -> i32 {
    let folder = match std::env::current_dir() {
        Ok(folder) => folder,
        Err(error) => {
            eprintln!("forge here: could not determine the current folder: {error}");
            return 1;
        }
    };
    match request_select_project(
        &resolve_endpoint(flag).await,
        &folder.to_string_lossy(),
        workspace,
        project,
    )
    .await
    {
        Ok(project) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&project).expect("Project is serializable")
                );
            } else {
                println!("{}", render_project_here(&project));
            }
            0
        }
        Err(error) => {
            eprintln!("forge here: {error}");
            1
        }
    }
}
async fn add_project(
    folder: &str,
    slug: &str,
    flag: Option<&str>,
    workspace: Option<String>,
) -> i32 {
    let folder = match std::env::current_dir() {
        Ok(cwd) => cwd.join(folder),
        Err(error) => {
            eprintln!("forge: {error}");
            return 1;
        }
    };
    match request_register_project_selected(
        &resolve_endpoint(flag).await,
        &folder.to_string_lossy(),
        slug,
        workspace,
    )
    .await
    {
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
async fn ready_records(args: ReadyRecordsArgs, workspace: Option<String>) -> i32 {
    match forge_cli::request_ready_records(
        selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
        args.planning,
        args.include_hitl,
        args.blocked,
    )
    .await
    {
        Ok(response) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string(&response).expect("readiness serializes")
                );
            } else {
                for item in response.records {
                    println!(
                        "{} {} ({}){}",
                        item.record.record_type,
                        item.record.id,
                        item.record.status,
                        if item.blocked_by.is_empty() {
                            String::new()
                        } else {
                            format!(" blocked by {:?}", item.blocked_by)
                        }
                    );
                }
            }
            0
        }
        Err(error) => {
            eprintln!("forge ready: {error}");
            1
        }
    }
}

async fn open_initiative(args: OpenWayfinderArgs, workspace: Option<String>) -> i32 {
    let body = match read_body(args.body, "initiative open") {
        Ok(body) => body,
        Err(code) => return code,
    };
    open_typed_result(
        "initiative open",
        args.json,
        forge_cli::request_open_initiative(
            selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
            &body,
            args.project,
            args.generated_by_ids,
        )
        .await,
    )
}

async fn selected_record_endpoint(
    flag: Option<&str>,
    workspace: Option<&str>,
) -> forge_cli::RecordEndpoint {
    forge_cli::RecordEndpoint {
        url: resolve_endpoint(flag).await,
        workspace: workspace.map(str::to_owned),
    }
}

async fn list_records(args: ListRecordsArgs, workspace: Option<String>) -> i32 {
    let status = args.status.map(|status| status.replace('-', "_"));
    match request_list_records(
        selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
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

async fn record_comment(args: AddRecordCommentArgs, workspace: Option<String>) -> i32 {
    match request_add_record_comment(
        selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
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
async fn delete_record_comment(args: DeleteRecordCommentArgs, workspace: Option<String>) -> i32 {
    match request_delete_record_comment(
        selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
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
async fn record_comments(args: RecordIdArgs, workspace: Option<String>) -> i32 {
    match request_record_comments(
        selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
        args.id,
    )
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
async fn record_updates(args: RecordIdArgs, workspace: Option<String>) -> i32 {
    match request_record_updates(
        selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
        args.id,
    )
    .await
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

async fn open_record(args: OpenRecordArgs, workspace: Option<String>) -> i32 {
    let body = match read_body(args.body, "record open") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    match request_open_record(
        selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
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

async fn open_spec(args: OpenSpecArgs, workspace: Option<String>) -> i32 {
    let body = match read_body(args.body, "spec open") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    open_typed_result(
        "spec open",
        args.json,
        request_open_spec(
            selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
            &body,
            args.project,
            args.generated_by_ids,
        )
        .await,
    )
}

async fn open_wayfinder(args: OpenWayfinderArgs, workspace: Option<String>) -> i32 {
    let body = match read_body(args.body, "wayfinder open") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    open_typed_result(
        "wayfinder open",
        args.json,
        request_open_wayfinder(
            selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
            &body,
            args.project,
            args.generated_by_ids,
        )
        .await,
    )
}

async fn open_task(args: OpenTaskArgs, workspace: Option<String>) -> i32 {
    let body = match read_body(args.body, "task open") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    open_typed_result(
        "task open",
        args.json,
        request_open_task(
            selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
            &body,
            args.project,
            args.generated_by_ids,
        )
        .await,
    )
}

async fn open_task_under_spec(args: OpenTaskUnderSpecArgs, workspace: Option<String>) -> i32 {
    let body = match read_body(args.body, "task under-spec") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    open_typed_result(
        "task under-spec",
        args.json,
        request_open_task_under_spec(
            selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
            &body,
            args.spec,
            args.generated_by_ids,
        )
        .await,
    )
}

async fn open_grilling_under_wayfinder(
    args: OpenGrillingUnderWayfinderArgs,
    workspace: Option<String>,
) -> i32 {
    let body = match read_body(args.body, "grilling open") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    open_typed_result(
        "grilling open",
        args.json,
        request_open_grilling_under_wayfinder(
            selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
            &body,
            args.wayfinder,
            args.project,
            args.generated_by_ids,
        )
        .await,
    )
}

async fn open_task_under_wayfinder(
    args: OpenTaskUnderWayfinderArgs,
    workspace: Option<String>,
) -> i32 {
    let body = match read_body(args.body, "task under-wayfinder") {
        Ok(body) => body,
        Err(exit_code) => return exit_code,
    };
    open_typed_result(
        "task under-wayfinder",
        args.json,
        request_open_task_under_wayfinder(
            selected_record_endpoint(args.server_url.as_deref(), workspace.as_deref()).await,
            &body,
            args.wayfinder,
            args.project,
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

    // These fixtures are parsed only: never invoke main, handlers, or a daemon.
    fn skill_examples(document: &str) -> Result<Vec<(usize, Vec<String>)>, String> {
        let mut in_shell = false;
        let mut examples = Vec::new();
        for (index, line) in document.lines().enumerate() {
            let line = line.trim();
            if line == "```sh" {
                if in_shell {
                    return Err(format!("nested shell fence at line {}", index + 1));
                }
                in_shell = true;
                continue;
            }
            if line == "```" && in_shell {
                in_shell = false;
                continue;
            }
            if !in_shell || line.is_empty() {
                continue;
            }
            let words = shlex::split(line)
                .ok_or_else(|| format!("invalid shell quoting at line {}", index + 1))?;
            let prefix = ["cargo", "run", "-p", "forge-cli", "--bin", "forge", "--"];
            let args = if words
                .iter()
                .map(String::as_str)
                .take(prefix.len())
                .eq(prefix)
            {
                std::iter::once("forge".to_owned())
                    .chain(words.into_iter().skip(prefix.len()))
                    .collect()
            } else if words.first().is_some_and(|word| word == "forge") {
                words
            } else {
                return Err(format!("expected a Forge command at line {}", index + 1));
            };
            examples.push((index + 1, args));
        }
        if in_shell || examples.is_empty() {
            return Err("unclosed shell fence or no examples".into());
        }
        Ok(examples)
    }

    #[test]
    fn dispatcher_and_assignment_command_paths() {
        use clap::CommandFactory;
        for args in [
            vec!["forge", "dispatcher", "status"],
            vec!["forge", "dispatcher", "reconcile"],
            vec!["forge", "dispatcher", "profile"],
            vec!["forge", "dispatcher", "resume", "42"],
            vec!["forge", "dispatcher", "task-reopen", "42"],
            vec!["forge", "dispatcher", "assignment-info", "a"],
            vec!["forge", "dispatcher", "assignment-retire", "a"],
            vec!["forge", "dispatcher", "assignment-resolve-unknown", "a"],
            vec!["forge", "assignment", "bootstrap"],
            vec!["forge", "assignment", "start"],
        ] {
            Cli::try_parse_from(&args).unwrap_or_else(|error| panic!("{args:?}: {error}"));
        }
        for args in [
            vec!["forge", "agent-dispatch", "status"],
            vec!["forge", "agent-dispatch", "bootstrap"],
            vec!["forge", "agent-dispatch", "task-start", "42"],
            vec!["forge", "dispatcher", "bootstrap"],
            vec!["forge", "dispatcher", "task-start", "42"],
            vec!["forge", "assignment", "task-start", "42"],
        ] {
            assert!(Cli::try_parse_from(&args).is_err(), "{args:?}");
        }
        let mut command = Cli::command();
        let help = command.render_long_help().to_string();
        assert!(help.contains("dispatcher"));
        assert!(help.contains("assignment"));
        assert!(!help.contains("agent-dispatch"));
        let help = command
            .find_subcommand_mut("dispatcher")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(!help.contains("bootstrap"));
        assert!(!help.contains("task-start"));
        for canonical in [
            "config",
            "dispatch",
            "profile",
            "status",
            "reconcile",
            "assignment-retire",
        ] {
            assert!(
                help.contains(canonical),
                "missing {canonical} in Dispatcher help"
            );
        }
        for removed in [
            "grilling-dispatch",
            "workspace-select",
            "endpoint",
            "profile-acknowledgement-timeout-set",
        ] {
            assert!(
                !help.contains(removed),
                "obsolete {removed} in Dispatcher help"
            );
        }
        let config = command
            .find_subcommand_mut("dispatcher")
            .unwrap()
            .find_subcommand_mut("config")
            .unwrap();
        let help = config.render_long_help().to_string();
        for canonical in ["get", "set", "unset", "provenance"] {
            assert!(help.contains(canonical));
        }
        let help = config
            .find_subcommand_mut("set")
            .unwrap()
            .render_long_help()
            .to_string();
        for required in [
            "<KEY>",
            "<VALUE>",
            "--project",
            "herdr-socket",
            "workspace",
            "project-limit",
            "acknowledgement-timeout-ms",
        ] {
            assert!(
                help.contains(required),
                "missing {required} in config set help"
            );
        }
    }

    #[test]
    fn documented_skill_commands_parse_without_execution() {
        use clap::CommandFactory;
        let documents = [
            ("SKILL.md", include_str!("../../../skills/forge/SKILL.md")),
            (
                "commands.md",
                include_str!("../../../skills/forge/commands.md"),
            ),
        ];
        let mut covered = std::collections::HashSet::new();
        for (name, document) in documents {
            for (line, args) in skill_examples(document).expect(name) {
                match Cli::try_parse_from(&args) {
                    Ok(_) => {
                        covered.insert(args[1].clone());
                    }
                    Err(error) if error.kind() == clap::error::ErrorKind::DisplayHelp => {}
                    Err(error) => panic!("{name}:{line}: {error}"),
                }
            }
        }
        for command in Cli::command().get_subcommands() {
            assert!(
                covered.contains(command.get_name()),
                "missing skill example for {}",
                command.get_name()
            );
        }
    }

    #[test]
    fn operator_guide_commands_parse_without_execution() {
        let documents = [
            ("README.md", include_str!("../../../README.md")),
            ("workspaces.md", include_str!("../../../docs/workspaces.md")),
            ("projects.md", include_str!("../../../docs/projects.md")),
            ("records.md", include_str!("../../../docs/records.md")),
            ("worktrees.md", include_str!("../../../docs/worktrees.md")),
            ("dispatch.md", include_str!("../../../docs/dispatch.md")),
            (
                "assignment-context.md",
                include_str!("../../../docs/assignment-context.md"),
            ),
            (
                "agent-profile-policy.md",
                include_str!("../../../docs/agent-profile-policy.md"),
            ),
            (
                "assignment-recovery.md",
                include_str!("../../../docs/assignment-recovery.md"),
            ),
        ];
        for (name, document) in documents {
            let mut in_shell = false;
            let mut continued = String::new();
            let mut count = 0;
            for (index, line) in document.lines().enumerate() {
                let line = line.trim();
                if line == "```sh" {
                    in_shell = true;
                } else if line == "```" {
                    assert!(continued.is_empty(), "{name}: unfinished command");
                    in_shell = false;
                } else if in_shell && (line.starts_with("forge ") || !continued.is_empty()) {
                    if let Some(prefix) = line.strip_suffix('\\') {
                        continued.push_str(prefix);
                        continued.push(' ');
                        continue;
                    }
                    continued.push_str(line);
                    let args: Vec<String> = shlex::split(&continued)
                        .expect("valid shell quoting")
                        .into_iter()
                        .map(|arg| match arg.as_str() {
                            "<spec-id>" | "<task-id>" | "<record-id>" => "42".into(),
                            _ => arg,
                        })
                        .collect();
                    Cli::try_parse_from(&args)
                        .unwrap_or_else(|error| panic!("{name}:{}: {error}", index + 1));
                    continued.clear();
                    count += 1;
                }
            }
            assert!(
                !in_shell && count > 0,
                "{name}: missing/unfinished examples"
            );
        }
    }

    #[test]
    fn workspace_contract_is_visible_in_generated_help() {
        use clap::CommandFactory;
        let mut command = Cli::command();
        let help = command.render_long_help().to_string();
        for required in [
            "no saved default",
            "not authentication",
            "<workspace>/<slug>",
        ] {
            assert!(help.contains(required), "missing {required}: {help}");
        }
        let help = command
            .find_subcommand_mut("workspace")
            .unwrap()
            .render_long_help()
            .to_string();
        for required in ["canonical", "overlapping", "immutable", "never deletes"] {
            assert!(help.contains(required), "missing {required}: {help}");
        }
        let help = command
            .find_subcommand_mut("dispatcher")
            .unwrap()
            .render_long_help()
            .to_string();
        for required in [
            "both Task and Grilling",
            "Grilling cwd",
            "retained resources",
            "invalidate",
        ] {
            assert!(help.contains(required), "missing {required}: {help}");
        }
    }

    #[test]
    fn documented_skill_extractor_preserves_arguments_and_rejects_bad_fixtures() {
        let examples =
            skill_examples("```sh\nforge record comment 42 --message 'two words'\n```").unwrap();
        assert_eq!(examples[0].1.last().unwrap(), "two words");
        for document in [
            "no examples",
            "```sh\nforge here",
            "```sh\nforge record comment 42 --message 'unclosed\n```",
            "```sh\nother command\n```",
        ] {
            assert!(skill_examples(document).is_err(), "{document}");
        }
        let examples = skill_examples("```sh\nforge nonexistent\n```").unwrap();
        assert!(Cli::try_parse_from(&examples[0].1).is_err());
    }

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
    fn explicit_record_dispatch_uses_only_the_canonical_path() {
        assert!(Cli::try_parse_from(["forge", "dispatcher", "dispatch", "42", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["forge", "dispatcher", "dispatch"]).is_err());
        assert!(Cli::try_parse_from(["forge", "dispatcher", "grilling-dispatch", "42"]).is_err());
    }

    #[test]
    fn dispatcher_provider_configuration_requires_explicit_values() {
        for args in [
            vec!["config", "get"],
            vec!["config", "get", "workspace"],
            vec!["config", "set", "workspace", "workspace-42"],
            vec!["config", "set", "herdr-socket", "/tmp/herdr.sock"],
            vec!["config", "unset", "workspace"],
            vec!["config", "unset", "herdr-socket"],
            vec!["config", "set", "project-limit", "3", "--project", "alpha"],
            vec!["config", "get", "project-limit", "--project", "alpha"],
            vec!["config", "unset", "project-limit", "--project", "alpha"],
            vec!["config", "set", "acknowledgement-timeout-ms", "42000"],
            vec!["config", "unset", "acknowledgement-timeout-ms"],
            vec!["profile"],
        ] {
            assert!(Cli::try_parse_from([vec!["forge", "dispatcher"], args].concat()).is_ok());
        }
        for args in [
            vec!["config", "set", "workspace"],
            vec!["config", "set", "herdr-socket"],
            vec!["endpoint"],
            vec!["workspace-select"],
            vec!["project", "--project", "alpha", "--limit", "3"],
            vec![
                "profile-acknowledgement-timeout-set",
                "--timeout-millis",
                "42000",
            ],
            vec!["config", "set", "project-limit", "--project", "alpha"],
            vec!["config", "set", "acknowledgement-timeout-ms"],
        ] {
            assert!(Cli::try_parse_from([vec!["forge", "dispatcher"], args].concat()).is_err());
        }
    }

    #[test]
    fn managed_worktree_commands_parse_the_public_contract() {
        assert!(
            Cli::try_parse_from(["forge", "config", "set", "worktreeRoot", "/tmp/worktrees"])
                .is_err()
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
