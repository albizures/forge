//! Atomic operator resolution over schema 21's opaque retained-resource contract.
use super::{authorize, error, exact, load};
use crate::{SqliteProjectRepository, now_millis, storage_error};
use forge_app::{
    RegistrationError,
    durable_execution::{
        Launch,
        effect_resolution::{Repository, Resolution, ResolutionReceipt},
    },
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

fn authorize_resolution(
    c: &Connection,
    workspace: &str,
    launch: &Launch,
    resolution: &Resolution,
) -> Result<(), RegistrationError> {
    authorize(c, workspace, launch, false)?;
    let runtime = load(c, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
    exact(&runtime, launch)?;
    resolution.validate()
}

fn accepted(
    c: &Connection,
    launch: &Launch,
    resolution: &Resolution,
) -> Result<(usize, Option<ResolutionReceipt>), RegistrationError> {
    let mut count = 0;
    let mut replay = None;
    // Retired audit resources do not reserve capacity. Their original
    // identity/timestamp remain visible through existing CLI/RPC reads.
    let mut statement = c.prepare("SELECT resource_id, created_at_millis, retired_at_millis FROM agent_assignment_resources WHERE assignment_id = ?1 AND resource_kind = 'interrupted-effect-resolution' LIMIT 1001").map_err(storage_error)?;
    let rows = statement
        .query_map([&launch.assignment_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })
        .map_err(storage_error)?;
    for row in rows {
        let (json, accepted_at_millis, retired) = row.map_err(storage_error)?;
        let previous: Resolution =
            serde_json::from_str(&json).map_err(|_| error("durable-storage-invalid"))?;
        previous.validate()?;
        if accepted_at_millis < 0 || retired != Some(accepted_at_millis) {
            return Err(error("durable-storage-invalid"));
        }
        count += 1;
        if previous.operation_id == resolution.operation_id {
            if previous != *resolution || replay.is_some() {
                return Err(error("durable-effect-resolution-conflict"));
            }
            replay = Some(ResolutionReceipt {
                resolution: previous,
                accepted_at_millis: accepted_at_millis.cast_unsigned(),
            });
        } else if previous.effect.task_id == resolution.effect.task_id {
            return Err(error("durable-effect-resolution-conflict"));
        }
    }
    if count > 1000 {
        return Err(error("durable-effect-limit"));
    }
    Ok((count, replay))
}

impl Repository for SqliteProjectRepository {
    fn accepted_effect_resolution(
        &self,
        workspace: &str,
        launch: &Launch,
        resolution: &Resolution,
    ) -> Result<Option<ResolutionReceipt>, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c.transaction().map_err(storage_error)?;
        authorize_resolution(&tx, workspace, launch, resolution)?;
        let (_, replay) = accepted(&tx, launch, resolution)?;
        tx.commit().map_err(storage_error)?;
        Ok(replay)
    }

    fn resolve_interrupted_effect(
        &self,
        workspace: &str,
        launch: &Launch,
        resolution: &Resolution,
    ) -> Result<ResolutionReceipt, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        authorize_resolution(&tx, workspace, launch, resolution)?;
        let (count, replay) = accepted(&tx, launch, resolution)?;
        if let Some(receipt) = replay {
            tx.commit().map_err(storage_error)?;
            return Ok(receipt);
        }
        if count == 1000 {
            return Err(error("durable-effect-limit"));
        }
        // Operator confirmation is additional to revocation, never a substitute.
        // This check and all dependent writes share the immediate transaction.
        let closed: bool = tx
            .query_row(
                "SELECT closed_at_millis IS NOT NULL FROM agent_assignments WHERE id = ?1",
                [&launch.assignment_id],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        if !closed {
            return Err(error("durable-effect-resolution-revocation-required"));
        }
        let effect_json = serde_json::to_string(&resolution.effect)
            .map_err(|_| error("durable-storage-invalid"))?;
        let retained: Option<Option<i64>> = tx.query_row(
            "SELECT retired_at_millis FROM agent_assignment_resources WHERE assignment_id = ?1 AND resource_kind = 'interrupted-effect' AND resource_id = ?2",
            params![launch.assignment_id, effect_json], |r| r.get(0),
        ).optional().map_err(storage_error)?;
        if retained != Some(None) {
            return Err(error("durable-effect-resolution-effect-missing"));
        }
        let accepted_at_millis = now_millis()?;
        let audit =
            serde_json::to_string(resolution).map_err(|_| error("durable-storage-invalid"))?;
        tx.execute("INSERT INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis, retired_at_millis) VALUES (?1, 'interrupted-effect-resolution', ?2, ?3, ?3)",
            params![launch.assignment_id, audit, accepted_at_millis],
        ).map_err(storage_error)?;
        tx.execute("UPDATE agent_assignment_resources SET retired_at_millis = ?3 WHERE assignment_id = ?1 AND resource_kind = 'interrupted-effect' AND resource_id = ?2 AND retired_at_millis IS NULL",
            params![launch.assignment_id, effect_json, accepted_at_millis],
        ).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(ResolutionReceipt {
            resolution: resolution.clone(),
            accepted_at_millis: accepted_at_millis.cast_unsigned(),
        })
    }
}
