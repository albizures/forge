//! Explicit runtime-history deletion. Forge journals intent first, the host removes
//! only the canonically mapped protected directory under exclusive ownership, and the
//! result is recorded. Nothing here starts a runner or touches Forge's audit database.
use forge_app::{
    RegistrationError, WorkspaceRepository,
    durable_execution::{
        Launch,
        purge::{Finish, Record, Removal, Repository as _, State},
    },
};
use forge_runner_host::{
    purge::{Purged, purge},
    storage::existing_runtime_directory,
};
use forge_store::SqliteProjectRepository;
use sha2::{Digest, Sha256};
use std::io::ErrorKind;

fn failure(code: &'static str) -> RegistrationError {
    RegistrationError::new(
        code,
        "runtime history was retained; inspect status for the diagnostic and retry the same operation after resolving it",
    )
}

fn delete(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
) -> Result<Removal, &'static str> {
    let root = repository
        .workspace(workspace)
        .map_err(|_| "runtime-purge-boundary-invalid")?
        .root;
    let digest = format!("{:x}", Sha256::digest(launch.store_id.as_bytes()));
    let directory =
        existing_runtime_directory(&root, &digest).map_err(|_| "runtime-purge-boundary-invalid")?;
    let removal = |purged| match purged {
        Purged::Removed => Removal::Removed,
        Purged::AlreadyAbsent => Removal::AlreadyAbsent,
    };
    purge(directory.as_deref()).map(removal).map_err(|error| {
        if error.kind() == ErrorKind::WouldBlock {
            "runtime-purge-owner-active"
        } else {
            "runtime-purge-storage-unsafe"
        }
    })
}

/// # Errors
/// Rejects ineligible Assignments before any deletion. A deletion failure is
/// recorded as retained history and returned; an exact retry resumes.
pub fn run(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    operation_id: &str,
) -> Result<Record, RegistrationError> {
    let record = repository.begin_purge(workspace, launch, operation_id)?;
    if record.state == State::Purged {
        return Ok(record);
    }
    match delete(repository, workspace, launch) {
        Ok(removal) => {
            repository.finish_purge(workspace, launch, operation_id, &Finish::Purged { removal })
        }
        Err(code) => {
            repository.finish_purge(
                workspace,
                launch,
                operation_id,
                &Finish::Retained { code: code.into() },
            )?;
            Err(failure(code))
        }
    }
}
