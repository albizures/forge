//! Assignment bootstrap provenance, independent of ownership and recovery evidence.
use crate::{SqliteProjectRepository, now_millis, storage_error};
use forge_app::{AssignmentSession, RegistrationError};
use rusqlite::{Connection, params};

#[cfg(test)]
#[path = "assignment_sessions_tests.rs"]
mod tests;

impl SqliteProjectRepository {
    pub(super) fn persist_assignment_session(
        &self,
        assignment_id: &str,
        session_id: &str,
    ) -> Result<(), RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "store lock poisoned"))?;
        // One statement atomically revalidates mutable bootstrap preconditions and
        // deduplicates observations, including writes from other store connections.
        let changed = connection.execute(
            "INSERT INTO assignment_sessions (assignment_id, session_id, first_bootstrap_at_millis, last_bootstrap_at_millis)
             SELECT a.id, ?2, ?3, ?3 FROM agent_assignments a
             JOIN records r ON r.id = a.record_id
             JOIN assignment_context_bootstrap b ON b.assignment_id = a.id
             WHERE a.id = ?1 AND a.closed_at_millis IS NULL
               AND a.acknowledged_at_millis IS NOT NULL AND r.status = 'in_progress'
             ON CONFLICT (assignment_id, session_id) DO UPDATE SET
               last_bootstrap_at_millis = MAX(assignment_sessions.last_bootstrap_at_millis, excluded.last_bootstrap_at_millis)",
            params![assignment_id, session_id, now_millis()?],
        ).map_err(storage_error)?;
        if changed != 1 {
            return Err(RegistrationError::new(
                "assignment-bootstrap-incomplete",
                "session registration requires an open Assignment with acknowledged start and trusted context",
            ));
        }
        Ok(())
    }
}

pub(super) fn load(
    connection: &Connection,
    assignment_id: &str,
) -> Result<Vec<AssignmentSession>, RegistrationError> {
    connection
        .prepare("SELECT session_id, first_bootstrap_at_millis, last_bootstrap_at_millis FROM assignment_sessions WHERE assignment_id = ?1 ORDER BY first_bootstrap_at_millis, session_id")
        .map_err(storage_error)?
        .query_map([assignment_id], |row| {
            Ok(AssignmentSession {
                session_id: row.get(0)?,
                first_bootstrap_at_millis: row.get::<_, i64>(1)?.cast_unsigned(),
                last_bootstrap_at_millis: row.get::<_, i64>(2)?.cast_unsigned(),
            })
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)
}
