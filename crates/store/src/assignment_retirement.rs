//! Final resource reconciliation after external provider cleanup, never during it.
use crate::{SqliteProjectRepository, now_millis, storage_error};
use forge_app::{RegistrationError, assignment_retirement::validate_finalization};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

impl SqliteProjectRepository {
    pub(super) fn finish_retirement(&self, assignment: &str) -> Result<(), RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let (revoked, unresolved) = tx.query_row(
            "SELECT closed_at_millis IS NOT NULL, EXISTS(SELECT 1 FROM agent_assignment_resources r WHERE r.assignment_id = a.id AND r.retired_at_millis IS NULL AND NOT (r.resource_kind = 'retirement-pending' AND r.resource_id = 'operator')) FROM agent_assignments a WHERE a.id = ?1",
            [assignment], |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?)),
        ).optional().map_err(storage_error)?.ok_or_else(|| {
            RegistrationError::new("assignment-not-found", "Agent Assignment does not exist")
        })?;
        validate_finalization(revoked, unresolved)?;
        // This is the only generic operator marker owned by this operation. A
        // differently identified marker is uncertainty, not an adopted operation.
        // Preserve the original timestamp on exact retries and after restart.
        tx.execute(
            "UPDATE agent_assignment_resources SET retired_at_millis = ?2 WHERE assignment_id = ?1 AND resource_kind = 'retirement-pending' AND resource_id = 'operator' AND retired_at_millis IS NULL",
            params![assignment, now_millis()?],
        ).map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }
}
