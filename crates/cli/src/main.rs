use clap::{Parser, Subcommand};
use forge_cli::{
    render_human_status, render_json_status, render_project_here, render_project_registered,
    request_project_here, request_register_project, request_status, server_url_from_process,
};

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
}

#[derive(Debug, Subcommand)]
enum AddCommand {
    /// Register a Project from its main worktree folder and immutable slug.
    Project {
        /// Git main worktree root to register.
        folder: String,
        /// Immutable Project slug.
        slug: String,
        #[arg(long, value_name = "URL")]
        server_url: Option<String>,
    },
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
    };
    std::process::exit(exit_code);
}

async fn status(flag_server_url: Option<&str>, json: bool) -> i32 {
    let server_url = server_url_from_process(flag_server_url);
    match request_status(&server_url).await {
        Ok(response) => {
            if json {
                println!("{}", render_json_status(&response));
            } else {
                println!("{}", render_human_status(&response));
            }
            0
        }
        Err(error) => {
            eprintln!("forge status: {error}");
            1
        }
    }
}

async fn here(flag_server_url: Option<&str>) -> i32 {
    let folder = match std::env::current_dir() {
        Ok(folder) => folder,
        Err(error) => {
            eprintln!("forge here: could not determine the current folder: {error}");
            return 1;
        }
    };
    let server_url = server_url_from_process(flag_server_url);
    match request_project_here(&server_url, &folder.to_string_lossy()).await {
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

async fn add_project(folder: &str, slug: &str, flag_server_url: Option<&str>) -> i32 {
    let server_url = server_url_from_process(flag_server_url);
    match request_register_project(&server_url, folder, slug).await {
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
