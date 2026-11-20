//! Canonical committed-input inspection, not a `RecoveryHost` implementation.
//! No lease, process-safety proof, profile/authentication or writable open.
use super::{inspection_process, runtime_directory};
use forge_app::{
    RegistrationError,
    durable_execution::{Admission, Launch, Repository as _},
};
use forge_rpc::durable_execution::{
    RUNNER_PROTOCOL_VERSION,
    recovery_inspection::{MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, PendingInput, Request, Response},
};
use forge_runner_host::inspection::ExclusiveInspection;
use forge_store::SqliteProjectRepository;
use std::{path::Path, time::Duration};

#[cfg(test)]
#[path = "recovery_inspection_tests.rs"]
mod tests;

fn denied() -> RegistrationError {
    RegistrationError::new(
        "durable-recovery-inspection-unavailable",
        "original pending runtime inspection failed; preserve evidence and capacity; no recovery is authorized",
    )
}

/// Retains the existing host lock across inspection and dependent operations.
/// This is not process-death proof: escaped descendants/effects remain separate
/// obligations. Dropping it releases the lock without changing Forge state.
#[must_use = "retain the guard through all dependent inspection/commit operations"]
pub struct Inspection {
    ownership: ExclusiveInspection,
    input: PendingInput,
}
impl Inspection {
    #[must_use]
    pub fn input(&self) -> &PendingInput {
        &self.input
    }

    /// Recheck filesystem/lock identity only, not current Forge execution policy.
    /// # Errors
    /// Rejects unsafe or replaced canonical directory/ownership evidence.
    pub fn verify(&self) -> Result<(), RegistrationError> {
        self.ownership.verify().map_err(|_| denied())
    }
}

/// Inspect the exact persisted original accepted input under exclusive ownership.
/// Components are explicit trusted daemon configuration, not RPC input or PATH
/// discovery. Storage derives only from the authorized immutable runtime mapping.
/// This does not validate live recovery authority, descendant cleanup, repository
/// freshness or a recovery claim. A caller must separately retain failure/effect
/// diagnostics; this read-only operation never clears blockers or issues grants.
/// # Errors
/// Rejects foreign scope/binding/admission, live ownership, unsafe paths, missing
/// components, malformed/oversized replies, deadline expiry and runtime failures.
pub async fn inspect_exclusive(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    admission_id: &str,
    node: &Path,
    inspector: &Path,
) -> Result<Inspection, RegistrationError> {
    launch.validate()?;
    if !node.is_absolute() || !inspector.is_absolute() || admission_id.trim().is_empty() {
        return Err(denied());
    }
    require_admission(repository, workspace, launch, admission_id)?;
    let directory = runtime_directory::mapped(repository, workspace, launch, false)?;
    let ownership = ExclusiveInspection::acquire(&directory).map_err(|_| denied())?;
    let input = read_input(&directory, launch, admission_id, node, inspector).await?;
    // External work completed outside a database transaction. Recheck persisted
    // scope/binding and canonical ownership before returning the retained guard.
    require_admission(repository, workspace, launch, admission_id)?;
    if runtime_directory::mapped(repository, workspace, launch, false)? != directory {
        return Err(denied());
    }
    let inspection = Inspection { ownership, input };
    inspection.verify()?;
    Ok(inspection)
}

/// Shared bounded read-only transport. Callers must independently retain and
/// reverify exclusive inspection OR directly spawned host ownership throughout.
/// It cannot verify host safety or current Forge authority by itself.
pub(super) async fn read_input(
    directory: &Path,
    launch: &Launch,
    admission_id: &str,
    node: &Path,
    inspector: &Path,
) -> Result<PendingInput, RegistrationError> {
    launch.validate()?;
    if !node.is_absolute() || !inspector.is_absolute() || admission_id.trim().is_empty() {
        return Err(denied());
    }
    let input = serde_json::to_vec(&Request {
        protocol: RUNNER_PROTOCOL_VERSION,
        launch: launch.clone(),
        admission_id: admission_id.into(),
    })
    .map_err(|_| denied())?;
    if input.len() > MAX_REQUEST_BYTES {
        return Err(denied());
    }
    let bytes = inspection_process::read(
        node,
        inspector,
        directory,
        &input,
        Duration::from_secs(30),
        MAX_RESPONSE_BYTES,
    )
    .await
    .map_err(|_| denied())?;
    decode(&bytes, launch, admission_id)
}

fn require_admission(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    admission_id: &str,
) -> Result<(), RegistrationError> {
    let runtime = repository
        .runtime(workspace, &launch.assignment_id)?
        .ok_or_else(denied)?;
    if runtime.launch != *launch
        || runtime.admission
            != (Admission::Accepted {
                admission_id: admission_id.into(),
            })
    {
        return Err(denied());
    }
    Ok(())
}

fn decode(
    bytes: &[u8],
    launch: &Launch,
    admission_id: &str,
) -> Result<PendingInput, RegistrationError> {
    let response: Response = serde_json::from_slice(bytes).map_err(|_| denied())?;
    response
        .validate(launch, admission_id)
        .map_err(|_| denied())?;
    Ok(response.candidate)
}
