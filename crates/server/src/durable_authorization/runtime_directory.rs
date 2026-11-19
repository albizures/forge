//! Canonical mapping from retained Forge identity; no caller-selected path.
use forge_app::{
    RegistrationError, WorkspaceRepository,
    durable_execution::{Launch, Repository},
};
use forge_store::SqliteProjectRepository;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub(super) fn mapped(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    create: bool,
) -> Result<PathBuf, RegistrationError> {
    let runtime = repository
        .runtime(workspace, &launch.assignment_id)?
        .ok_or_else(|| {
            RegistrationError::new(
                "durable-intent-missing",
                "reserve the immutable runtime before preparing storage",
            )
        })?;
    if runtime.launch != *launch {
        return Err(RegistrationError::new(
            "durable-operation-conflict",
            "runtime storage requires the exact retained launch identity",
        ));
    }
    let root = repository.workspace(workspace)?.root;
    let digest = format!("{:x}", Sha256::digest(launch.store_id.as_bytes()));
    forge_runner_host::storage::runtime_directory(&root, &digest, create).map_err(|_| {
        RegistrationError::new(
            "durable-storage-boundary-invalid",
            "runtime storage must use the protected canonical Workspace mapping",
        )
    })
}
