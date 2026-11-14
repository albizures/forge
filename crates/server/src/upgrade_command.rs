//! Versioned offline adapter used by Forgeup; never starts a listener.
use std::path::Path;

pub(super) fn run(arguments: &[String]) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let value = match arguments {
        [operation] if operation == "capabilities" => serde_json::to_value(forge_store::upgrade::capabilities())?,
        [operation, path] if operation == "inspect" => serde_json::to_value(forge_store::upgrade::inspect(Path::new(path)).map_err(storage_error)?)?,
        [operation, path, output] if operation == "snapshot" => serde_json::to_value(forge_store::upgrade::snapshot(Path::new(path), Path::new(output)).map_err(storage_error)?)?,
        [operation, path] if operation == "migrate" => serde_json::to_value(forge_store::upgrade::migrate(Path::new(path)).map_err(storage_error)?)?,
        _ => return Err("usage: forge-server --state-upgrade capabilities|inspect PATH|snapshot SOURCE OUTPUT|migrate STAGING".into()),
    };
    Ok(value)
}

fn storage_error(error: forge_app::RegistrationError) -> std::io::Error {
    let message = error.message;
    std::io::Error::other(format!("{}: {message}", error.code))
}
