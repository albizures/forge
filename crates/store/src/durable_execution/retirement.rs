//! Atomic revoked retirement journal and resource reconciliation; no host I/O.
use super::{authorize, error, exact, load};
use crate::{SqliteProjectRepository, now_millis, storage_error};
use forge_app::{
    RegistrationError,
    durable_execution::{
        Launch,
        recovery::{Claim, validate_retirement},
        retirement::{
            Operation, Receipt, Repository, Retirement, reconciles_resource, validate_begin,
            validate_owner, validate_receipt,
        },
    },
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

fn revoked(c: &Connection, workspace: &str, launch: &Launch) -> Result<(), RegistrationError> {
    authorize(c, workspace, launch, false)?;
    let runtime = load(c, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
    exact(&runtime, launch)?;
    let closed: bool = c
        .query_row(
            "SELECT closed_at_millis IS NOT NULL FROM agent_assignments WHERE id = ?1",
            [&launch.assignment_id],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    validate_begin(closed, &runtime.admission)
}
fn read(c: &Connection, assignment: &str) -> Result<Option<Retirement>, RegistrationError> {
    let row: Option<(String, Option<i64>)> = c.query_row("SELECT operation_json, reconciled_at_millis FROM durable_assignment_retirements WHERE assignment_id = ?1", [assignment], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(storage_error)?;
    row.map(|(json, time)| {
        Ok(Retirement {
            operation: serde_json::from_str(&json).map_err(|_| error("durable-storage-invalid"))?,
            reconciled_at_millis: time
                .map(|v| u64::try_from(v).map_err(|_| error("durable-storage-invalid")))
                .transpose()?,
        })
    })
    .transpose()
}
impl Repository for SqliteProjectRepository {
    fn revoked_lease(
        &self,
        workspace: &str,
        launch: &Launch,
    ) -> Result<Option<forge_app::authorization_lease::Lease>, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c.transaction().map_err(storage_error)?;
        revoked(&tx, workspace, launch)?;
        let row: Option<(u32, String)> = tx.query_row("SELECT sequence, lease_json FROM assignment_authorization_leases WHERE assignment_id = ?1 ORDER BY sequence DESC LIMIT 1",
            [&launch.assignment_id], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(storage_error)?;
        let lease = row
            .map(|(sequence, json)| {
                let lease: forge_app::authorization_lease::Lease =
                    serde_json::from_str(&json).map_err(|_| error("durable-storage-invalid"))?;
                if sequence == 0 || lease.sequence != sequence || lease.launch != *launch {
                    return Err(error("durable-storage-invalid"));
                }
                let mut shape = lease.clone();
                shape.sequence = 1;
                shape.validate(None, shape.issued_boottime_millis)?;
                Ok(lease)
            })
            .transpose()?;
        tx.commit().map_err(storage_error)?;
        Ok(lease)
    }

    fn begin_retirement(
        &self,
        workspace: &str,
        operation: &Operation,
    ) -> Result<Retirement, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        revoked(&tx, workspace, &operation.launch)?;
        if let Some(existing) = read(&tx, &operation.launch.assignment_id)? {
            if existing.operation != *operation {
                return Err(error("durable-retirement-conflict"));
            }
            tx.commit().map_err(storage_error)?;
            return Ok(existing);
        }
        operation.validate()?;
        let last: Option<String> = tx.query_row("SELECT lease_json FROM assignment_authorization_leases WHERE assignment_id = ?1 ORDER BY sequence DESC LIMIT 1", [&operation.launch.assignment_id], |r| r.get(0)).optional().map_err(storage_error)?;
        let last: Option<forge_app::authorization_lease::Lease> = last
            .map(|json| serde_json::from_str(&json).map_err(|_| error("durable-storage-invalid")))
            .transpose()?;
        validate_owner(last.as_ref().map(|lease| &lease.owner), &operation.owner)?;
        tx.execute("INSERT INTO durable_assignment_retirements(assignment_id, operation_json) VALUES (?1, ?2)", params![operation.launch.assignment_id, serde_json::to_string(operation).map_err(|_| error("durable-storage-invalid"))?]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(Retirement {
            operation: operation.clone(),
            reconciled_at_millis: None,
        })
    }
    fn retirement(
        &self,
        workspace: &str,
        launch: &Launch,
    ) -> Result<Option<Retirement>, RegistrationError> {
        let c = self.connection.lock().map_err(|_| error("storage"))?;
        revoked(&c, workspace, launch)?;
        read(&c, &launch.assignment_id)
    }
    fn reconcile_retirement(
        &self,
        workspace: &str,
        receipt: &Receipt,
    ) -> Result<Retirement, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        revoked(&tx, workspace, &receipt.operation.launch)?;
        let mut existing = read(&tx, &receipt.operation.launch.assignment_id)?
            .ok_or_else(|| error("durable-retirement-intent-missing"))?;
        if existing.operation != receipt.operation {
            return Err(error("durable-retirement-conflict"));
        }
        validate_receipt(receipt)?;
        if existing.reconciled_at_millis.is_none() {
            let now = now_millis()?;
            tx.execute("UPDATE durable_assignment_retirements SET reconciled_at_millis = ?2 WHERE assignment_id = ?1", params![receipt.operation.launch.assignment_id, now]).map_err(storage_error)?;
            // Only execution-liveness diagnostics and matching recovery fences
            // are discharged. Effects and unknown identities remain blockers.
            // Claim history survives cancellation, so retries cannot transfer it.
            let resources = tx.prepare("SELECT resource_kind, resource_id FROM agent_assignment_resources WHERE assignment_id = ?1 AND retired_at_millis IS NULL").map_err(storage_error)?
                .query_map([&receipt.operation.launch.assignment_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(storage_error)?
                .collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
            for (kind, id) in resources {
                let recovery_claim = kind == "recovery-claim";
                if recovery_claim {
                    let claim: Claim =
                        serde_json::from_str(&id).map_err(|_| error("durable-storage-invalid"))?;
                    validate_retirement(&claim, &receipt.operation.launch)?;
                }
                if recovery_claim || reconciles_resource(&receipt.operation.launch, &kind, &id) {
                    tx.execute("UPDATE agent_assignment_resources SET retired_at_millis = ?4 WHERE assignment_id = ?1 AND resource_kind = ?2 AND resource_id = ?3 AND retired_at_millis IS NULL", params![receipt.operation.launch.assignment_id, kind, id, now]).map_err(storage_error)?;
                }
            }
            existing.reconciled_at_millis =
                Some(u64::try_from(now).map_err(|_| error("durable-storage-invalid"))?);
        }
        tx.commit().map_err(storage_error)?;
        Ok(existing)
    }
}
