//! Trusted committed-effect observation, separate from recovery authorization.
//! No profile source, model credential, capability, writable Harness or Git access.
use super::runtime_directory;
use forge_app::{
    RegistrationError,
    durable_execution::{
        Launch,
        effects::{InterruptedEffect, Repository},
    },
};
use forge_store::SqliteProjectRepository;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;

const MAX_RESPONSE: u64 = 20 * 1024 * 1024;

fn denied() -> RegistrationError {
    RegistrationError::new(
        "durable-effect-observation-unavailable",
        "committed effect inspection failed; preserve runtime evidence and capacity; no recovery is authorized",
    )
}

/// Inspect with explicitly selected matched components, then atomically retain
/// the observed identities. Paths are trusted daemon configuration, not RPC input
/// or PATH discovery; the runner verifies its hash against the retained Launch.
/// Storage derives only from the authorized immutable Workspace/runtime mapping.
///
/// This bounded read-only operation does not acquire execution ownership, prove
/// process absence, resolve effects, clear holds, or authorize recovery. An empty
/// observation never clears earlier blockers. Unknown/corrupt/missing stores fail
/// closed. Callers must not interpret an error or successful scan as permission
/// to open writable storage. Production scheduling remains separately gated.
/// # Errors
/// Rejects scope/binding/path mismatches before spawning, incompatible components,
/// malformed/oversized responses, deadline expiry and transactional retention errors.
pub async fn inspect_and_retain(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    node: &Path,
    inspector: &Path,
) -> Result<(), RegistrationError> {
    let directory = runtime_directory::mapped(repository, workspace, launch, false)?;
    launch.validate()?;
    if !node.is_absolute() || !inspector.is_absolute() {
        return Err(denied());
    }
    let input = serde_json::to_vec(&serde_json::json!({"protocol": 1, "launch": launch}))
        .map_err(|_| denied())?;
    if input.len() > 16384 {
        return Err(denied());
    }
    let effects = read_effects(
        node,
        inspector,
        &directory,
        &input,
        launch,
        Duration::from_secs(30),
        MAX_RESPONSE,
    )
    .await?;
    // External work is complete; authorize scope and immutable binding again in
    // the retention transaction, even after closure or a lost admission reply.
    repository.retain_interrupted_effects(workspace, launch, &effects)
}

async fn read_effects(
    node: &Path,
    inspector: &Path,
    directory: &Path,
    input: &[u8],
    launch: &Launch,
    deadline: Duration,
    max_response: u64,
) -> Result<Vec<InterruptedEffect>, RegistrationError> {
    let mut child = tokio::process::Command::new(node)
        .arg(inspector)
        .arg(directory)
        // In particular, exclude NODE_OPTIONS, ambient Pi settings and secrets.
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| denied())?;
    let result = tokio::time::timeout(deadline, async {
        let mut stdin = child.stdin.take().ok_or_else(denied)?;
        let stdout = child.stdout.take().ok_or_else(denied)?;
        stdin.write_all(input).await.map_err(|_| denied())?;
        drop(stdin);
        let mut bytes = Vec::new();
        stdout
            .take(max_response + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| denied())?;
        if bytes.len() as u64 > max_response {
            return Err(denied());
        }
        if !child.wait().await.map_err(|_| denied())?.success() {
            return Err(denied());
        }
        decode(&bytes, launch)
    })
    .await;
    let Ok(Ok(effects)) = result else {
        // Reap only this directly spawned read-only inspector. This is not
        // Assignment retirement or evidence that external effects stopped.
        // Even a read-only child can be stuck in kernel I/O. Do not turn the
        // observation deadline into an unbounded wait or infer exit on timeout.
        tokio::time::timeout(Duration::from_secs(1), child.kill())
            .await
            .map_err(|_| denied())?
            .map_err(|_| denied())?;
        return Err(denied());
    };
    Ok(effects)
}

fn decode(bytes: &[u8], launch: &Launch) -> Result<Vec<InterruptedEffect>, RegistrationError> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| denied())?;
    let object = value.as_object().ok_or_else(denied)?;
    if object.len() != 3 || value["protocol"].as_u64() != Some(1) {
        return Err(denied());
    }
    let observed: Launch = serde_json::from_value(value["launch"].clone()).map_err(|_| denied())?;
    if observed != *launch {
        return Err(denied());
    }
    let effects: Vec<InterruptedEffect> =
        serde_json::from_value(value["effects"].clone()).map_err(|_| denied())?;
    if effects.len() > 1000 {
        return Err(denied());
    }
    for effect in &effects {
        effect.validate().map_err(|_| denied())?;
    }
    Ok(effects)
}
