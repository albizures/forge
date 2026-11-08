//! Resolve user-facing Project selectors once, before any mutating request.
use super::{
    AgentDispatchCommand, Command, DispatcherConfigCommand, GrillingCommand, ProjectCommand,
    ProjectSetupHookCommand, RecordCommand, SpecCommand, TaskCommand, WayfinderCommand,
};

pub async fn qualify(command: &mut Command, workspace: Option<&str>) -> Result<(), String> {
    let (projects, endpoint): (Vec<&mut String>, Option<&str>) = match command {
        Command::Spec {
            command: SpecCommand::Open(args),
        } => (vec![&mut args.project], args.server_url.as_deref()),
        Command::Wayfinder {
            command: WayfinderCommand::Open(args),
        } => (
            args.project.iter_mut().collect(),
            args.server_url.as_deref(),
        ),
        Command::Grilling {
            command: GrillingCommand::Open(args),
        } => (
            args.project.iter_mut().flatten().collect(),
            args.server_url.as_deref(),
        ),
        Command::Task {
            command: TaskCommand::Open(args),
        } => (
            args.project.iter_mut().collect(),
            args.server_url.as_deref(),
        ),
        Command::Task {
            command: TaskCommand::UnderWayfinder(args),
        } => (
            args.project.iter_mut().collect(),
            args.server_url.as_deref(),
        ),
        Command::Record {
            command: RecordCommand::Open(args),
        } => (
            args.project.iter_mut().collect(),
            args.server_url.as_deref(),
        ),
        Command::Record {
            command: RecordCommand::List(args),
        } => (
            args.project.iter_mut().collect(),
            args.server_url.as_deref(),
        ),
        Command::Dispatcher {
            command: AgentDispatchCommand::Status(args),
        } => (
            args.project.iter_mut().collect(),
            args.server_url.as_deref(),
        ),
        Command::Dispatcher {
            command: AgentDispatchCommand::Config { command },
        } => match command {
            DispatcherConfigCommand::Get(args) => (
                args.project.iter_mut().collect(),
                args.server_url.as_deref(),
            ),
            DispatcherConfigCommand::Set(args) => (
                args.project.iter_mut().collect(),
                args.server_url.as_deref(),
            ),
            DispatcherConfigCommand::Unset(args) => (
                args.project.iter_mut().collect(),
                args.server_url.as_deref(),
            ),
        },
        Command::Project {
            command: ProjectCommand::SetupHook { command },
        } => match command {
            ProjectSetupHookCommand::Set(args) => {
                (vec![&mut args.project], args.server_url.as_deref())
            }
            ProjectSetupHookCommand::Clear(args) => {
                (vec![&mut args.project], args.server_url.as_deref())
            }
        },
        _ => return Ok(()),
    };
    if projects.is_empty() {
        return Ok(());
    }
    let endpoint = super::resolve_endpoint(endpoint).await;
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    // Resolve every selector before the command is executed, so a rejected scope
    // cannot leave a partially applied mutation behind.
    for project in projects {
        *project = forge_cli::request_select_project(
            &endpoint,
            &cwd.to_string_lossy(),
            workspace.map(str::to_owned),
            Some(project.clone()),
        )
        .await?
        .reference;
    }
    Ok(())
}
