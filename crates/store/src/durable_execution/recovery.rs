//! Forge recovery preflight and exclusive reservation. No grants or external I/O.
use super::{authorize, error, exact, load};
use crate::{SqliteProjectRepository, now_millis, storage_error, task_attempts};
use forge_app::{
    RegistrationError,
    authorization_lease::Lease,
    durable_execution::{
        Admission, Launch,
        recovery::{Candidate, Claim, Repository, validate_current_attempt},
    },
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

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
        let result = candidate(&tx, workspace, launch, None)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }

    fn claimed_recovery_candidate(
        &self,
        workspace: &str,
        claim: &Claim,
    ) -> Result<Candidate, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c.transaction().map_err(storage_error)?;
        let current = claimed_candidate(&tx, workspace, claim)?;
        tx.commit().map_err(storage_error)?;
        Ok(current)
    }

    fn claim_recovery(&self, workspace: &str, claim: &Claim) -> Result<Claim, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let launch = &claim.launch;
        // Authorize immutable scope before historical replay, including after
        // closure. Replaying a fence cannot revive the old execution authority.
        authorize(&tx, workspace, launch, false)?;
        let runtime =
            load(&tx, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
        exact(&runtime, launch)?;
        claim.validate()?;
        let mut statement = tx.prepare(
            "SELECT resource_id FROM agent_assignment_resources WHERE assignment_id = ?1 AND resource_kind = 'recovery-claim' LIMIT 2",
        ).map_err(storage_error)?;
        // Include retired history: no implicit takeover/reuse after cleanup.
        let known = statement
            .query_map([&launch.assignment_id], |r| r.get::<_, String>(0))
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        drop(statement);
        if let [stored] = known.as_slice() {
            let stored: Claim =
                serde_json::from_str(stored).map_err(|_| error("durable-storage-invalid"))?;
            stored.validate()?;
            if stored != *claim {
                return Err(error("durable-recovery-claim-conflict"));
            }
            tx.commit().map_err(storage_error)?;
            return Ok(stored);
        }
        if !known.is_empty() {
            return Err(error("durable-storage-invalid"));
        }
        let current = candidate(&tx, workspace, launch, None)?;
        if Claim::for_candidate(
            &current,
            claim.operation_id.clone(),
            claim.daemon_epoch.clone(),
        )? != *claim
        {
            return Err(error("durable-recovery-candidate-stale"));
        }
        // The existing opaque resource ledger retains identity, timestamp and
        // capacity atomically. Its presence also blocks ordinary lease renewal.
        // Only specialized authorization or verified retirement can consume it.
        tx.execute(
            "INSERT INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) VALUES (?1, 'recovery-claim', ?2, ?3)",
            params![launch.assignment_id, serde_json::to_string(claim).map_err(|_| error("durable-storage-invalid"))?, now_millis()?],
        ).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(claim.clone())
    }
}

impl forge_app::durable_execution::recovery::worktree::Repository for SqliteProjectRepository {
    fn claimed_worktree_binding(
        &self,
        workspace: &str,
        claim: &Claim,
    ) -> Result<forge_app::durable_execution::recovery::worktree::Binding, RegistrationError> {
        use forge_app::durable_execution::recovery::worktree::Binding;
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c.transaction().map_err(storage_error)?;
        let candidate = claimed_candidate(&tx, workspace, claim)?;
        let (root, base, check_name): (String, String, String) = tx.query_row(
            "SELECT p.registered_folder, b.base_commit, k.name FROM agent_assignments a JOIN projects p ON p.slug = a.project_slug JOIN task_base_commits b ON b.record_id = a.record_id JOIN record_checks k ON k.record_id = a.record_id AND k.id = ?2 WHERE a.id = ?1",
            params![claim.launch.assignment_id, candidate.attempt.check_id.cast_signed()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional().map_err(storage_error)?.ok_or_else(|| error("durable-recovery-worktree-unbound"))?;
        let binding = Binding {
            worktree_path: crate::workspace_worktree_path(&tx, candidate.attempt.record_id)?,
            candidate,
            project_root: root.into(),
            base_commit: base,
            check_name,
        };
        binding.validate()?;
        tx.commit().map_err(storage_error)?;
        Ok(binding)
    }
}

// Shared snapshot read for candidate-only and repository-progress inspection.
// Scope precedes claim inspection; history never reconstructs live authority.
fn claimed_candidate(
    c: &Connection,
    workspace: &str,
    claim: &Claim,
) -> Result<Candidate, RegistrationError> {
    authorize(c, workspace, &claim.launch, false)?;
    claim.validate()?;
    let claims = c.prepare(
        "SELECT resource_id, retired_at_millis FROM agent_assignment_resources WHERE assignment_id = ?1 AND resource_kind = 'recovery-claim' LIMIT 2",
    ).map_err(storage_error)?.query_map([&claim.launch.assignment_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
    }).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
    let [(json, None)] = claims.as_slice() else {
        return Err(error("durable-recovery-claim-missing"));
    };
    let stored: Claim = serde_json::from_str(json).map_err(|_| error("durable-storage-invalid"))?;
    if stored != *claim {
        return Err(error("durable-recovery-claim-conflict"));
    }
    let current = candidate(c, workspace, &claim.launch, Some(claim))?;
    if Claim::for_candidate(
        &current,
        claim.operation_id.clone(),
        claim.daemon_epoch.clone(),
    )? != *claim
    {
        return Err(error("durable-recovery-candidate-stale"));
    }
    Ok(current)
}

pub(super) fn candidate(
    c: &Connection,
    workspace: &str,
    launch: &Launch,
    claim: Option<&Claim>,
) -> Result<Candidate, RegistrationError> {
    authorize(c, workspace, launch, false)?;
    launch.validate()?;
    let runtime = load(c, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
    exact(&runtime, launch)?;
    let Admission::Accepted { admission_id } = runtime.admission else {
        return Err(error("durable-recovery-not-admitted"));
    };
    if admission_id.trim().is_empty() {
        return Err(error("durable-storage-invalid"));
    }
    let attempt = task_attempts::binding(c, &launch.assignment_id)?
        .ok_or_else(|| error("durable-binding-denied"))?;
    let task = crate::load_record(c, attempt.record_id)?;
    validate_current_attempt(
        &task,
        &attempt,
        launch,
        crate::outcomes::state(c, attempt.record_id)?.generation,
    )?;
    crate::spec_plans::assignment_execution(c, &launch.assignment_id)?;
    if !crate::unfinished_prerequisite_ids(c, attempt.record_id)?.is_empty() {
        return Err(error("durable-recovery-blocked"));
    }
    // Provisioning/Working/Idle alone do not establish continued authority.
    // Require the original open attempt, live reservation, no holds, no
    // competing owner/resources and no retirement/effect/authorization fence.
    // Only the exact specialized recovery operation may look through its own
    // fence. Every unrelated resource and claim remains an execution blocker.
    let claim_json = claim
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| error("durable-storage-invalid"))?;
    let eligible: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_assignments a JOIN record_attempts r ON r.id = ?2 WHERE a.id = ?1 AND a.closed_at_millis IS NULL AND a.state IN ('provisioning', 'working', 'idle') AND r.closed = 0
             AND NOT EXISTS(SELECT 1 FROM dispatch_holds h WHERE h.record_id = a.record_id)
             AND NOT EXISTS(SELECT 1 FROM durable_assignment_retirements d WHERE d.assignment_id = a.id)
             AND EXISTS(SELECT 1 FROM agent_assignment_resources x WHERE x.assignment_id = a.id AND x.resource_kind = 'durable-runtime' AND x.resource_id = ?3 AND x.retired_at_millis IS NULL)
             AND NOT EXISTS(SELECT 1 FROM agent_assignment_resources x WHERE x.assignment_id = a.id AND x.retired_at_millis IS NULL AND x.resource_kind NOT IN ('durable-runtime', 'provision-intent') AND NOT (x.resource_kind = 'recovery-claim' AND x.resource_id = COALESCE(?4, '')))
             AND NOT EXISTS(SELECT 1 FROM agent_assignments other WHERE other.record_id = a.record_id AND other.id != a.id AND (other.closed_at_millis IS NULL OR EXISTS(SELECT 1 FROM agent_assignment_resources x WHERE x.assignment_id = other.id AND x.retired_at_millis IS NULL))))",
            params![launch.assignment_id, launch.attempt_id, launch.runtime_id, claim_json], |r| r.get(0),
        ).map_err(storage_error)?;
    if !eligible {
        return Err(error("durable-recovery-blocked"));
    }
    let directory: Option<String> = c.query_row(
            "SELECT b.directory FROM assignment_execution_bindings b JOIN agent_assignments a ON a.id = b.assignment_id JOIN worktree_claims w ON w.record_id = a.record_id AND w.project_slug = a.project_slug AND w.path = b.directory AND w.state = 'active' WHERE b.assignment_id = ?1",
            [&launch.assignment_id], |r| r.get(0),
        ).optional().map_err(storage_error)?;
    let execution_directory = directory
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| error("durable-recovery-worktree-unbound"))?;
    let last: Option<(u32, String)> = c.query_row(
            "SELECT sequence, lease_json FROM assignment_authorization_leases WHERE assignment_id = ?1 ORDER BY sequence DESC LIMIT 1",
            [&launch.assignment_id], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional().map_err(storage_error)?;
    let (sequence, json) = last.ok_or_else(|| error("durable-recovery-grant-missing"))?;
    let last_lease: Lease =
        serde_json::from_str(&json).map_err(|_| error("durable-storage-invalid"))?;
    if last_lease.sequence != sequence {
        return Err(error("durable-storage-invalid"));
    }
    if last_lease.launch != *launch || last_lease.generation != attempt.generation {
        return Err(error("durable-binding-stale"));
    }
    // No clock/expiry check adopts this grant. It identifies the predecessor
    // owner for future cleanup proof; only a separate protocol may replace it.
    Ok(Candidate {
        launch: launch.clone(),
        admission_id,
        attempt,
        execution_directory,
        last_lease,
    })
}
