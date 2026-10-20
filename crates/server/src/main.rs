use forge_server::{DaemonConfig, serve};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let bind_addr = parse_bind_addr()?;
    let config = DaemonConfig::from_process(bind_addr.as_deref())?;
    serve(config).await?;
    Ok(())
}

fn parse_bind_addr() -> Result<Option<String>, String> {
    let mut arguments = std::env::args().skip(1);
    let mut bind_addr = None;

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--bind-addr" => {
                let value = arguments.next().ok_or("--bind-addr requires an address")?;
                bind_addr = Some(value);
            }
            "--help" | "-h" => {
                println!(
                    "Usage: forge-server [--bind-addr IP:PORT]\n\nFORGE_BIND_ADDR supplies the address when the flag is absent. Remote bindings are unauthenticated; use only a trusted private network."
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument `{argument}`")),
        }
    }

    Ok(bind_addr)
}
