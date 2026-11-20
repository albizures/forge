//! Storage evidence checks before the first grant, never writable recovery.
use forge_app::{
    RegistrationError,
    durable_execution::{Admission, Launch, Repository as _},
};
use forge_store::SqliteProjectRepository;
use std::{io::ErrorKind, path::Path};

pub(super) fn denied() -> RegistrationError {
    RegistrationError::new(
        "durable-effect-admission-unavailable",
        "retained runtime requires matched committed inspection before any grant; preserve evidence and capacity",
    )
}

fn exists(path: &Path) -> Result<bool, RegistrationError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(_) => Err(denied()),
    }
}

pub(super) fn has_database(directory: &Path) -> Result<bool, RegistrationError> {
    exists(&directory.join("runtime.sqlite"))
}

/// The legacy synchronous binding is fresh-only: it cannot bypass committed
/// inspection by presenting an existing store or leftover SQLite sidecars.
pub(super) fn require_fresh(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    directory: &Path,
) -> Result<(), RegistrationError> {
    for name in ["runtime.sqlite", "runtime.sqlite-wal", "runtime.sqlite-shm"] {
        if exists(&directory.join(name))? {
            return Err(denied());
        }
    }
    let runtime = repository
        .runtime(workspace, &launch.assignment_id)?
        .ok_or_else(denied)?;
    if runtime.launch != *launch
        || matches!(
            runtime.admission,
            Admission::Accepted { .. } | Admission::Rejected { .. }
        )
    {
        return Err(denied());
    }
    Ok(())
}
