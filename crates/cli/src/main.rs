use clap::{Parser, Subcommand};
use forge_cli::{render_human_status, render_json_status, request_status, server_url_from_process};

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
        /// JSON-RPC endpoint for the running Forge daemon.
        #[arg(long, value_name = "URL")]
        server_url: Option<String>,
        /// Print the status as stable JSON.
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let exit_code = match cli.command {
        Command::Status { server_url, json } => status(server_url.as_deref(), json).await,
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
