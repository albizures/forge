use clap::{Args, Subcommand, ValueEnum};
use forge_rpc::{AgentProfileApiClient, AgentProfileDocumentRequest, AgentProfileDocumentResponse};
use jsonrpsee::http_client::HttpClientBuilder;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum RecordType {
    Task,
    Grilling,
}
impl RecordType {
    fn name(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Grilling => "grilling",
        }
    }
}
#[derive(Debug, Args)]
#[command(
    after_long_help = "Requires explicit --workspace. Documents must be .md files beneath that Workspace's .agents/profiles/ directory (nested folders allowed); canonical containment rejects symlink escapes. Document paths resolve from that Workspace root, not cwd; resource paths resolve from the document directory. Profiles are independent of Skills and select them explicitly. Preview does not execute extensions, connect MCP servers, register or select. Registration creates an immutable revision without selecting it. Use dispatcher config set task-profile|grilling-profile <revision> to select explicitly. Runtime support is checked separately before dispatch."
)]
pub struct AgentProfileArgs {
    #[command(subcommand)]
    command: ProfileCommand,
    #[arg(long, global = true)]
    server_url: Option<String>,
    #[arg(long, global = true)]
    json: bool,
}
#[derive(Debug, Subcommand)]
enum ProfileCommand {
    /// Resolve and inspect policy without changing registration or selections.
    Preview {
        document: String,
        #[arg(long, value_enum, default_value = "task")]
        record_type: RecordType,
    },
    /// Persist a new immutable revision, without selecting it.
    Register {
        document: String,
        #[arg(long, value_enum)]
        record_type: RecordType,
    },
}
pub async fn run(args: AgentProfileArgs, workspace: Option<String>) -> i32 {
    match request(&args, workspace).await {
        Ok(value) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("profile response is JSON")
                );
            } else {
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
async fn request(
    args: &AgentProfileArgs,
    workspace: Option<String>,
) -> Result<AgentProfileDocumentResponse, String> {
    let workspace = workspace
        .filter(|value| !value.trim().is_empty())
        .ok_or("agent-profile requires explicit --workspace")?;
    let (ProfileCommand::Preview {
        document,
        record_type,
    }
    | ProfileCommand::Register {
        document,
        record_type,
    }) = &args.command;
    let request = AgentProfileDocumentRequest {
        workspace,
        document: document.clone(),
        record_type: record_type.name().into(),
    };
    let endpoint = super::resolve_endpoint(args.server_url.as_deref()).await;
    let client = HttpClientBuilder::default()
        .build(&endpoint)
        .map_err(|error| error.to_string())?;
    match args.command {
        ProfileCommand::Preview { .. } => client.preview(request).await,
        ProfileCommand::Register { .. } => client.register(request).await,
    }
    .map_err(|error| error.to_string())
}
fn render(value: &AgentProfileDocumentResponse) {
    println!(
        "Workspace {} / {} profile",
        value.workspace, value.record_type
    );
    if let Some(revision) = value.revision {
        println!("Registered revision {revision}; selection unchanged.");
    } else {
        println!("Preview only; registration and selection unchanged.");
    }
    println!(
        "Runtime: {}\nModel: {}\nBuilt-in tools: {}",
        value.policy.runtime,
        value.policy.model,
        value.policy.tools.join(", ")
    );
    if let Some(level) = value.policy.thinking_level {
        println!("Thinking level: {}", level.as_str());
    }
    for (category, resources) in [
        ("skill", &value.policy.skills),
        ("extension", &value.policy.extensions),
        ("MCP", &value.policy.mcp_servers),
    ] {
        if resources.is_empty() {
            println!("{category}: none");
        }
        for resource in resources {
            println!(
                "{category}: {}\t{}\tSHA-256 {}",
                resource.name, resource.path, resource.sha256
            );
        }
    }
    println!(
        "Lifecycle: {}",
        serde_json::to_string(&value.lifecycle).expect("lifecycle is JSON")
    );
    println!("Fingerprint coverage: {}", value.fingerprint_coverage);
    for limitation in &value.limitations {
        println!("- {limitation}");
    }
}
