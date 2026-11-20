//! Trusted committed-effect observation, separate from recovery authorization.
//! No profile source, model credential, capability, writable Harness or Git access.
use super::runtime_directory;
use forge_app::{
    RegistrationError,
    durable_execution::{
        Launch,
        effect_resolution::{Repository as ResolutionRepository, Resolution, ResolutionReceipt},
        effects::{InterruptedEffect, Repository},
    },
};
use forge_runner_host::inspection::ExclusiveInspection;
use forge_store::SqliteProjectRepository;
use std::{path::Path, time::Duration};

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
    inspect_mapped(
        repository, workspace, launch, node, inspector, &directory, None,
    )
    .await
    .map(|_| ())
}

/// Grant-controller fence for an already owned, not-yet-authorized supervisor.
/// The caller must verify its directly spawned host before and after this await.
/// Retain committed effects before applying the app-owned execution gate. An
/// external audit never manufactures a runtime receipt or permits blind replay.
/// # Errors
/// Rejects all inspection/retention failures and every interrupted intent.
pub(super) async fn inspect_before_grant(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    components: &super::operator::InspectionComponents,
) -> Result<(), RegistrationError> {
    let directory = runtime_directory::mapped(repository, workspace, launch, false)?;
    let effects = inspect_mapped(
        repository,
        workspace,
        launch,
        &components.node,
        &components.inspector,
        &directory,
        None,
    )
    .await?;
    forge_app::durable_execution::effects::validate_execution_observation(&effects)
}

/// Obtain a host-exclusive committed snapshot and retain its blockers while the
/// same lock remains held. The returned guard keeps competing supervisors out
/// until the caller finishes its dependent inspection. Never treat success as
/// permission to resume: this does not verify external process absence, resolve
/// any effect, clear a hold, or issue execution authority. Missing ownership
/// evidence fails closed rather than creating a new lock.
/// # Errors
/// Rejects scope/binding/path errors before lock acquisition, live owners,
/// replaced ownership evidence and all ordinary observation/retention failures.
pub async fn inspect_exclusive_and_retain(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    node: &Path,
    inspector: &Path,
) -> Result<ExclusiveInspection, RegistrationError> {
    let directory = runtime_directory::mapped(repository, workspace, launch, false)?;
    let ownership = ExclusiveInspection::acquire(&directory).map_err(|_| denied())?;
    inspect_mapped(
        repository,
        workspace,
        launch,
        node,
        inspector,
        &directory,
        Some(&ownership),
    )
    .await?;
    ownership.verify().map_err(|_| denied())?;
    Ok(ownership)
}

/// Resolve one externally investigated effect under the existing exclusive host
/// lock, after a fresh committed scan. Trusted operator input only, never an
/// Assignment capability operation. The store atomically validates the retained
/// effect and records the original evidence-based audit. A live Assignment keeps
/// its effect resource as an execution fence; only a closed effect is retired.
/// Exact accepted-result retries are scope-authorized read-only audit lookups;
/// they do not require current host ownership, runtime storage or components.
/// No runtime receipt is repaired and no recovery or command replay is authorized.
/// External children must separately be made safe, as explicitly asserted in the
/// request: the host lock alone cannot prove universal process absence.
/// # Errors
/// Rejects unconfirmed evidence, unsafe ownership, unavailable inspection, wrong
/// scope/binding and conflicting/missing effects. Errors do not
/// clear holds or other blockers. A post-commit path error may leave an accepted
/// audit; retry the exact operation rather than inventing a new resolution.
pub async fn resolve_exclusive(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    node: &Path,
    inspector: &Path,
    resolution: &Resolution,
) -> Result<ResolutionReceipt, RegistrationError> {
    if let Some(receipt) = repository.accepted_effect_resolution(workspace, launch, resolution)? {
        return Ok(receipt);
    }
    let ownership =
        inspect_exclusive_and_retain(repository, workspace, launch, node, inspector).await?;
    ownership.verify().map_err(|_| denied())?;
    let receipt = repository.resolve_interrupted_effect(workspace, launch, resolution)?;
    ownership.verify().map_err(|_| denied())?;
    Ok(receipt)
}

/// Synchronous operator-adapter bridge. Keep the host inspection lock held while
/// a dedicated executor reads committed effects and retains them in Forge; never
/// nest `block_on` in the HTTP executor or open writable runtime storage.
pub(super) fn inspect_retirement(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    components: &super::operator::InspectionComponents,
    directory: &Path,
    ownership: &ExclusiveInspection,
) -> Result<(), RegistrationError> {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| denied())?;
                runtime
                    .block_on(inspect_mapped(
                        repository,
                        workspace,
                        launch,
                        &components.node,
                        &components.inspector,
                        directory,
                        Some(ownership),
                    ))
                    .map(|_| ())
            })
            .join()
            .map_err(|_| denied())?
    })
}

async fn inspect_mapped(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    node: &Path,
    inspector: &Path,
    directory: &Path,
    ownership: Option<&ExclusiveInspection>,
) -> Result<Vec<InterruptedEffect>, RegistrationError> {
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
        directory,
        &input,
        launch,
        Duration::from_secs(30),
        MAX_RESPONSE,
    )
    .await?;
    // External work is complete; authorize scope and immutable binding again in
    // the retention transaction, even after closure or a lost admission reply.
    if let Some(ownership) = ownership {
        ownership.verify().map_err(|_| denied())?;
    }
    repository.retain_interrupted_effects(workspace, launch, &effects)?;
    Ok(effects)
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
    let bytes =
        super::inspection_process::read(node, inspector, directory, input, deadline, max_response)
            .await
            .map_err(|_| denied())?;
    decode(&bytes, launch)
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
