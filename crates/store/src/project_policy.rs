//! Persistence for Project Base Branch Policy.
use forge_app::{BaseBranchPolicy, RegistrationError};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{SqliteProjectRepository, storage_error};

pub(super) fn schema(connection: &Connection) -> Result<(), RegistrationError> {
    connection
        .execute_batch(
            include_str!("upgrade/base_branch_policy.sql")
                .replace("CREATE TABLE ", "CREATE TABLE IF NOT EXISTS ")
                .as_str(),
        )
        .map_err(storage_error)
}

pub(super) fn get(
    store: &SqliteProjectRepository,
    reference: &str,
) -> Result<BaseBranchPolicy, RegistrationError> {
    let connection = store
        .connection
        .lock()
        .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE slug = ?1)",
            [reference],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if !exists {
        return Err(RegistrationError::new(
            "project-not-found",
            "Project does not exist",
        ));
    }
    let value: Option<String> = connection
        .query_row(
            "SELECT policy FROM project_base_branch_policy WHERE project_slug = ?1",
            [reference],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    value.map_or(Ok(BaseBranchPolicy::Ask), |s| {
        s.parse()
            .map_err(|_| RegistrationError::new("storage", "invalid persisted Base Branch Policy"))
    })
}

pub(super) fn set(
    store: &SqliteProjectRepository,
    reference: &str,
    policy: Option<BaseBranchPolicy>,
) -> Result<(), RegistrationError> {
    let mut connection = store
        .connection
        .lock()
        .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(storage_error)?;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE slug = ?1)",
            [reference],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if !exists {
        return Err(RegistrationError::new(
            "project-not-found",
            "Project does not exist",
        ));
    }
    if let Some(policy) = policy {
        tx.execute("INSERT INTO project_base_branch_policy (project_slug, policy) VALUES (?1, ?2) ON CONFLICT(project_slug) DO UPDATE SET policy = excluded.policy", params![reference, policy.as_str()]).map_err(storage_error)?;
    } else {
        tx.execute(
            "DELETE FROM project_base_branch_policy WHERE project_slug = ?1",
            [reference],
        )
        .map_err(storage_error)?;
    }
    tx.commit().map_err(storage_error)
}
