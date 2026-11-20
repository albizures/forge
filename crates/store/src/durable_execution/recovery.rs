//! Snapshot-only Forge recovery preflight. No claims, grants or external I/O.
use super::{authorize, error, exact, load};
use crate::{SqliteProjectRepository, storage_error, task_attempts};
use forge_app::{
    RegistrationError,
    authorization_lease::Lease,
    durable_execution::{
        Admission, Launch,
        recovery::{Candidate, Repository, validate_current_attempt},
    },
};
use rusqlite::{OptionalExtension, params};

impl Repository for SqliteProjectRepository {
    fn recovery_candidate(
        &self,
        workspace: &str,
        launch: &Launch,
    ) -> Result<Candidate, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        // Every read uses the same snapshot, including across other connections.
        // No result is execution permission; eventual claims must revalidate it.
        let tx = c.transaction().map_err(storage_error)?;
        authorize(&tx, workspace, launch, false)?;
        launch.validate()?;
        let runtime =
            load(&tx, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
        exact(&runtime, launch)?;
        let Admission::Accepted { admission_id } = runtime.admission else {
            return Err(error("durable-recovery-not-admitted"));
        };
        if admission_id.trim().is_empty() {
            return Err(error("durable-storage-invalid"));
        }
        let attempt = task_attempts::binding(&tx, &launch.assignment_id)?
            .ok_or_else(|| error("durable-binding-denied"))?;
        let task = crate::load_record(&tx, attempt.record_id)?;
        validate_current_attempt(
            &task,
            &attempt,
            launch,
            crate::outcomes::state(&tx, attempt.record_id)?.generation,
        )?;
        crate::spec_plans::assignment_execution(&tx, &launch.assignment_id)?;
        if !crate::unfinished_prerequisite_ids(&tx, attempt.record_id)?.is_empty() {
            return Err(error("durable-recovery-blocked"));
        }
        // Provisioning/Working/Idle alone do not establish continued authority.
        // Require the original open attempt, live reservation, no holds, no
        // competing owner/resources and no retirement/effect/authorization fence.
        let eligible: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_assignments a JOIN record_attempts r ON r.id = ?2 WHERE a.id = ?1 AND a.closed_at_millis IS NULL AND a.state IN ('provisioning', 'working', 'idle') AND r.closed = 0
             AND NOT EXISTS(SELECT 1 FROM dispatch_holds h WHERE h.record_id = a.record_id)
             AND NOT EXISTS(SELECT 1 FROM durable_assignment_retirements d WHERE d.assignment_id = a.id)
             AND EXISTS(SELECT 1 FROM agent_assignment_resources x WHERE x.assignment_id = a.id AND x.resource_kind = 'durable-runtime' AND x.resource_id = ?3 AND x.retired_at_millis IS NULL)
             AND NOT EXISTS(SELECT 1 FROM agent_assignment_resources x WHERE x.assignment_id = a.id AND x.retired_at_millis IS NULL AND x.resource_kind NOT IN ('durable-runtime', 'provision-intent'))
             AND NOT EXISTS(SELECT 1 FROM agent_assignments other WHERE other.record_id = a.record_id AND other.id != a.id AND (other.closed_at_millis IS NULL OR EXISTS(SELECT 1 FROM agent_assignment_resources x WHERE x.assignment_id = other.id AND x.retired_at_millis IS NULL))))",
            params![launch.assignment_id, launch.attempt_id, launch.runtime_id], |r| r.get(0),
        ).map_err(storage_error)?;
        if !eligible {
            return Err(error("durable-recovery-blocked"));
        }
        let directory: Option<String> = tx.query_row(
            "SELECT b.directory FROM assignment_execution_bindings b JOIN agent_assignments a ON a.id = b.assignment_id JOIN worktree_claims w ON w.record_id = a.record_id AND w.project_slug = a.project_slug AND w.path = b.directory AND w.state = 'active' WHERE b.assignment_id = ?1",
            [&launch.assignment_id], |r| r.get(0),
        ).optional().map_err(storage_error)?;
        let execution_directory = directory
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| error("durable-recovery-worktree-unbound"))?;
        let last: Option<String> = tx.query_row(
            "SELECT lease_json FROM assignment_authorization_leases WHERE assignment_id = ?1 ORDER BY sequence DESC LIMIT 1",
            [&launch.assignment_id], |r| r.get(0),
        ).optional().map_err(storage_error)?;
        let last_lease: Lease =
            serde_json::from_str(&last.ok_or_else(|| error("durable-recovery-grant-missing"))?)
                .map_err(|_| error("durable-storage-invalid"))?;
        if last_lease.launch != *launch || last_lease.generation != attempt.generation {
            return Err(error("durable-binding-stale"));
        }
        // No clock/expiry check adopts this grant. It identifies the predecessor
        // owner for future cleanup proof; only a separate protocol may replace it.
        tx.commit().map_err(storage_error)?;
        Ok(Candidate {
            launch: launch.clone(),
            admission_id,
            attempt,
            execution_directory,
            last_lease,
        })
    }
}
