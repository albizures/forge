use forge_server::{DaemonConfig, ManagedLaunchInputs, serve};
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

    let options = parse_options()?;
    let config = DaemonConfig::from_process(options.bind_addr.as_deref(), options.managed)?;
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

struct ServerOptions {
    bind_addr: Option<String>,
    managed: Option<ManagedLaunchInputs>,
}

fn parse_options() -> Result<ServerOptions, String> {
    let mut arguments = std::env::args().skip(1);
    let mut bind_addr = None;
    let mut managed = false;
    let mut store_path = None;
    let mut runtime_record_path = None;
    let mut lifecycle_control_endpoint = None;
    let mut lifecycle_credential = None;
    let mut daemon_instance_id = None;

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--bind-addr" => bind_addr = Some(argument_value(&mut arguments, "--bind-addr")?),
            "--managed" => managed = true,
            "--store-path" => store_path = Some(argument_value(&mut arguments, "--store-path")?),
            "--runtime-record-path" => {
                runtime_record_path =
                    Some(argument_value(&mut arguments, "--runtime-record-path")?);
            }
            "--lifecycle-control-endpoint" => {
                lifecycle_control_endpoint = Some(argument_value(
                    &mut arguments,
                    "--lifecycle-control-endpoint",
                )?);
            }
            "--lifecycle-credential" => {
                lifecycle_credential =
                    Some(argument_value(&mut arguments, "--lifecycle-credential")?);
            }
            "--daemon-instance-id" => {
                daemon_instance_id = Some(argument_value(&mut arguments, "--daemon-instance-id")?);
            }
            "--help" | "-h" => {
                println!(
                    "Usage: forge-server [--bind-addr IP:PORT] [--managed --store-path PATH --runtime-record-path PATH --lifecycle-control-endpoint PATH --lifecycle-credential VALUE --daemon-instance-id ID]\n\nFORGE_BIND_ADDR and FORGE_STORE_PATH supply manual-launch defaults. A managed launch must explicitly provide every managed input. Remote bindings are unauthenticated; use only a trusted private network."
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument `{argument}`")),
        }
    }

    let managed = if managed {
        Some(ManagedLaunchInputs {
            bind_addr: bind_addr.clone().ok_or("--managed requires --bind-addr")?,
            store_path: store_path.ok_or("--managed requires --store-path")?,
            runtime_record_path: runtime_record_path
                .ok_or("--managed requires --runtime-record-path")?,
            lifecycle_control_endpoint: lifecycle_control_endpoint
                .ok_or("--managed requires --lifecycle-control-endpoint")?,
            lifecycle_credential: lifecycle_credential
                .ok_or("--managed requires --lifecycle-credential")?,
            daemon_instance_id: daemon_instance_id
                .ok_or("--managed requires --daemon-instance-id")?,
        })
    } else {
        if store_path.is_some()
            || runtime_record_path.is_some()
            || lifecycle_control_endpoint.is_some()
            || lifecycle_credential.is_some()
            || daemon_instance_id.is_some()
        {
            return Err("managed launch inputs require --managed".into());
        }
        None
    };

    Ok(ServerOptions { bind_addr, managed })
}

fn argument_value(
    arguments: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{flag} requires a value"))
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
