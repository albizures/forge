use forge_server::{DaemonConfig, serve};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(build_info) = build_info_json(std::env::args().skip(1)) {
        println!("{build_info}");
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let bind_addr = parse_bind_addr()?;
    let config = DaemonConfig::from_process(bind_addr.as_deref())?;
    serve(config).await?;
    Ok(())
}

fn build_info_json(arguments: impl IntoIterator<Item = String>) -> Option<String> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    (arguments.len() == 2
        && arguments.iter().any(|argument| argument == "--build-info")
        && arguments.iter().any(|argument| argument == "--json"))
    .then(|| serde_json::to_string(&forge_build_info::current()).expect("BuildInfo must serialize"))
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

#[cfg(test)]
mod tests {
    use super::build_info_json;

    #[test]
    fn build_info_is_available_without_constructing_a_daemon() {
        let json = build_info_json(["--build-info".into(), "--json".into()]).unwrap();
        let info = serde_json::from_str::<serde_json::Value>(&json).unwrap();
        assert!(info["commit"].is_string());
        assert!(info["management_protocol_revision"].is_u64());
    }
}
