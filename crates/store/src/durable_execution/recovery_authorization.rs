//! Specialized recovery transition. External safety is verified before entry;
//! all mutable Forge gates and accepted effects commit in one writer transaction.
use super::{authorize, error, recovery::candidate};
use crate::{SqliteProjectRepository, now_millis, storage_error};
use forge_app::{
    RegistrationError,
    authorization_lease::Lease,
    durable_execution::recovery::authorization::{Grant, Prepared, Repository},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

fn accepted_grant(
    c: &rusqlite::Connection,
    wanted: &Grant,
    candidate: &forge_app::durable_execution::recovery::Candidate,
) -> Result<Option<Grant>, RegistrationError> {
    let known = c.prepare("SELECT resource_id FROM agent_assignment_resources WHERE assignment_id = ?1 AND resource_kind = 'recovery-grant'")
        .map_err(storage_error)?.query_map([&wanted.claim.launch.assignment_id], |r| r.get::<_, String>(0))
        .map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
    let mut seen = std::collections::HashSet::new();
    let mut accepted = None;
    for json in known {
        let grant: Grant =
            serde_json::from_str(&json).map_err(|_| error("durable-storage-invalid"))?;
        grant.claim.validate()?;
        if grant.claim.launch != wanted.claim.launch
            || !seen.insert(grant.claim.operation_id.clone())
        {
            return Err(error("durable-storage-invalid"));
        }
        let predecessor = grant
            .lease
            .sequence
            .checked_sub(1)
            .filter(|seq| *seq > 0)
            .ok_or_else(|| error("durable-storage-invalid"))?;
        let json: String = c.query_row("SELECT lease_json FROM assignment_authorization_leases WHERE assignment_id = ?1 AND sequence = ?2",
            params![wanted.claim.launch.assignment_id, predecessor], |r| r.get(0)).map_err(storage_error)?;
        let mut historical = candidate.clone();
        historical.last_lease =
            serde_json::from_str(&json).map_err(|_| error("durable-storage-invalid"))?;
        grant.validate_candidate(&historical, grant.lease.issued_boottime_millis)?;
        if grant.claim.operation_id == wanted.claim.operation_id {
            accepted = Some(grant);
        }
    }
    Ok(accepted)
}

impl Repository for SqliteProjectRepository {
    fn grant_recovery(
        &self,
        workspace: &str,
        prepared: &Prepared,
        now: u64,
    ) -> Result<Grant, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let grant = prepared.grant();
        let launch = &grant.claim.launch;
        // Scope precedes even retained-result inspection. A retained audit alone
        // never reconstructs Prepared or revives revoked/expired permission.
        authorize(&tx, workspace, launch, false)?;
        prepared.validate(now)?;
        let claims = super::recovery::claims(&tx, &launch.assignment_id)?;
        let (claim, retired, id) = claims
            .iter()
            .find(|(claim, _, _)| *claim == grant.claim)
            .ok_or_else(|| error("durable-recovery-claim-missing"))?;
        let accepted = accepted_grant(&tx, grant, prepared.candidate())?;
        // Revalidate every live gate before considering delivery replay. Only
        // this exact claim is ignored; holds/effects/unknown resources still deny.
        let mut current = candidate(&tx, workspace, launch, Some(claim))?;
        if let Some(accepted) = accepted {
            if accepted != *grant || !*retired || current.last_lease != grant.lease {
                return Err(error("durable-recovery-grant-conflict"));
            }
            // Current history must still contain the exact predecessor committed
            // by the claim, not a caller-provided substitute for today's owner.
            let predecessor: Option<String> = tx.query_row(
                "SELECT lease_json FROM assignment_authorization_leases WHERE assignment_id = ?1 AND sequence = ?2",
                params![launch.assignment_id, prepared.candidate().last_lease.sequence], |r| r.get(0),
            ).optional().map_err(storage_error)?;
            let predecessor: Lease =
                serde_json::from_str(&predecessor.ok_or_else(|| error("durable-storage-invalid"))?)
                    .map_err(|_| error("durable-storage-invalid"))?;
            current.last_lease = predecessor;
            if current != *prepared.candidate() {
                return Err(error("durable-recovery-candidate-stale"));
            }
            tx.commit().map_err(storage_error)?;
            return Ok(accepted);
        }
        // A cancelled reservation is historical only and can never grant work.
        if *retired || current != *prepared.candidate() {
            return Err(error("durable-recovery-candidate-stale"));
        }
        let now_wall = now_millis()?;
        tx.execute(
            "INSERT INTO assignment_authorization_leases(assignment_id, sequence, lease_json) VALUES (?1, ?2, ?3)",
            params![launch.assignment_id, grant.lease.sequence, serde_json::to_string(&grant.lease).map_err(|_| error("durable-storage-invalid"))?],
        ).map_err(storage_error)?;
        tx.execute(
            "INSERT INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis, retired_at_millis) VALUES (?1, 'recovery-grant', ?2, ?3, ?3)",
            params![launch.assignment_id, serde_json::to_string(grant).map_err(|_| error("durable-storage-invalid"))?, now_wall],
        ).map_err(storage_error)?;
        let changed = tx.execute(
            "UPDATE agent_assignment_resources SET retired_at_millis = ?3 WHERE assignment_id = ?1 AND resource_kind = 'recovery-claim' AND resource_id = ?2 AND retired_at_millis IS NULL",
            params![launch.assignment_id, id, now_wall],
        ).map_err(storage_error)?;
        if changed != 1 {
            return Err(error("durable-recovery-claim-conflict"));
        }
        tx.commit().map_err(storage_error)?;
        Ok(grant.clone())
    }
}
