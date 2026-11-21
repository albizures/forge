//! Read-only durable status; no writes, locks or host I/O.
use super::{error, load, scope};
use crate::{SqliteProjectRepository, storage_error};
use forge_app::{
    RegistrationError,
    authorization_lease::Lease,
    durable_execution::status::{LeaseSummary, Repository, Status},
};
use rusqlite::OptionalExtension;

impl Repository for SqliteProjectRepository {
    fn durable_status(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<Option<Status>, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        // One deferred read transaction gives a consistent snapshot of all facts.
        let tx = c.transaction().map_err(storage_error)?;
        scope(&tx, workspace, assignment)?;
        let Some(runtime) = load(&tx, assignment)? else {
            return Ok(None);
        };
        let closed: bool = tx
            .query_row(
                "SELECT closed_at_millis IS NOT NULL FROM agent_assignments WHERE id = ?1",
                [assignment],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        let count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM assignment_authorization_leases WHERE assignment_id = ?1",
                [assignment],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        let latest: Option<(u32, String)> = tx
            .query_row(
                "SELECT sequence, lease_json FROM assignment_authorization_leases WHERE assignment_id = ?1 ORDER BY sequence DESC LIMIT 1",
                [assignment],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        let latest_lease = latest
            .map(|(sequence, json)| {
                let lease: Lease =
                    serde_json::from_str(&json).map_err(|_| error("durable-storage-invalid"))?;
                if lease.sequence != sequence || lease.launch != runtime.launch {
                    return Err(error("durable-storage-invalid"));
                }
                Ok(LeaseSummary {
                    sequence,
                    generation: lease.generation,
                    issued_boottime_millis: lease.issued_boottime_millis,
                    expires_boottime_millis: lease.expires_boottime_millis,
                })
            })
            .transpose()?;
        let launch = runtime.launch;
        Ok(Some(Status {
            backend: runtime.backend,
            assignment_id: launch.assignment_id,
            runtime_id: launch.runtime_id,
            operation_id: launch.operation_id,
            attempt_id: launch.attempt_id,
            incarnation_id: launch.incarnation_id,
            profile_revision: launch.profile_revision,
            forge_revision: launch.forge_revision,
            component_hash: launch.component_hash,
            admission: runtime.admission,
            lease_count: u32::try_from(count).map_err(|_| error("durable-storage-invalid"))?,
            latest_lease,
            assignment_closed: closed,
        }))
    }
}
