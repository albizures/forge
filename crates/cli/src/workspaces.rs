use clap::{Args, Subcommand};
use forge_rpc::{
    WorkspaceApiClient, WorkspaceCreateRequest, WorkspaceIdRequest, WorkspaceRenameRequest,
    WorkspaceSelectRequest,
};
use jsonrpsee::http_client::HttpClientBuilder;

#[derive(Debug, Args)]
#[command(
    after_long_help = "A Forge Workspace has a fixed canonical filesystem root, distinct from a Herdr terminal container. Equal or overlapping roots (including symlink aliases) are rejected. IDs and roots are immutable; names are editable. Projects must be main Git checkouts at or below the root, outside .forge/worktrees. Removal requires no Projects, Records, Assignments, retained resources or dispatch configuration; it never deletes files or Herdr containers. Root relocation and Project reparenting are unavailable."
)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    command: WorkspaceCommand,
    #[arg(long, global = true)]
    server_url: Option<String>,
    #[arg(long, global = true)]
    json: bool,
}
#[derive(Debug, Subcommand)]
enum WorkspaceCommand {
    /// Register a fixed canonical filesystem root (not a Herdr container).
    Add {
        id: String,
        #[arg(long)]
        root: std::path::PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    /// List every registration without requiring a selected Workspace.
    List,
    /// Inspect the explicitly selected Workspace, otherwise infer from cwd.
    Show,
    /// Identify the Workspace containing cwd, unless explicitly selected.
    Here,
    /// Change a display name, never its ID or root.
    Rename { id: String, name: String },
    /// Remove an empty registration; never delete files or Herdr containers.
    Remove { id: String },
}

pub async fn run(args: WorkspaceArgs, workspace: Option<String>) -> i32 {
    match request(&args, workspace).await {
        Ok(value) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("Workspace response is JSON")
                );
            } else if let Some(values) = value.as_array() {
                for value in values {
                    render(value);
                }
            } else if !value.is_null() {
                render(&value);
            }
            0
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}
fn render(value: &serde_json::Value) {
    println!(
        "{}\t{}\t{}",
        value["id"].as_str().unwrap_or_default(),
        value["name"].as_str().unwrap_or_default(),
        value["root"].as_str().unwrap_or_default()
    );
}
async fn request(
    args: &WorkspaceArgs,
    workspace: Option<String>,
) -> Result<serde_json::Value, String> {
    let endpoint = super::resolve_endpoint(args.server_url.as_deref()).await;
    let client = HttpClientBuilder::default()
        .build(&endpoint)
        .map_err(|error| error.to_string())?;
    // IDs on management commands are explicit selectors too; do not silently
    // accept a contradictory global --workspace value.
    let id = match &args.command {
        WorkspaceCommand::Add { id, .. }
        | WorkspaceCommand::Rename { id, .. }
        | WorkspaceCommand::Remove { id } => Some(id),
        _ => None,
    };
    if let (Some(id), Some(selected)) = (id, workspace.as_ref())
        && id != selected
    {
        return Err("Workspace selectors conflict".into());
    }
    let result = match &args.command {
        WorkspaceCommand::Add { id, root, name } => {
            let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
            let root = if root.is_absolute() {
                root.clone()
            } else {
                cwd.join(root)
            };
            serde_json::to_value(
                client
                    .create(WorkspaceCreateRequest {
                        id: id.clone(),
                        name: name.as_ref().unwrap_or(id).clone(),
                        root: root.to_string_lossy().into_owned(),
                    })
                    .await
                    .map_err(|error| error.to_string())?,
            )
        }
        WorkspaceCommand::List => {
            serde_json::to_value(client.list().await.map_err(|error| error.to_string())?)
        }
        WorkspaceCommand::Show | WorkspaceCommand::Here => {
            let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
            serde_json::to_value(
                client
                    .select(WorkspaceSelectRequest {
                        workspace,
                        project_workspace: None,
                        cwd: cwd.to_string_lossy().into_owned(),
                    })
                    .await
                    .map_err(|error| error.to_string())?,
            )
        }
        WorkspaceCommand::Rename { id, name } => serde_json::to_value(
            client
                .rename(WorkspaceRenameRequest {
                    id: id.clone(),
                    name: name.clone(),
                })
                .await
                .map_err(|error| error.to_string())?,
        ),
        WorkspaceCommand::Remove { id } => {
            client
                .remove(WorkspaceIdRequest { id: id.clone() })
                .await
                .map_err(|error| error.to_string())?;
            Ok(serde_json::Value::Null)
        }
    };
    result.map_err(|error| error.to_string())
}
