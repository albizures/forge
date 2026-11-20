//! Retained interrupted effects; no external observation inside a transaction.
use super::{authorize, error, exact, load};
use crate::{SqliteProjectRepository, now_millis, storage_error};
use forge_app::{
    RegistrationError,
    durable_execution::{
        Launch,
        effects::{InterruptedEffect, Repository},
    },
};
use rusqlite::{TransactionBehavior, params};
use std::collections::BTreeMap;

impl Repository for SqliteProjectRepository {
    fn retain_interrupted_effects(
        &self,
        workspace: &str,
        launch: &Launch,
        effects: &[InterruptedEffect],
    ) -> Result<(), RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        // Scope and immutable binding precede all replay. Observations can arrive
        // after closure, revocation, or a lost admission reply; none renew authority.
        authorize(&tx, workspace, launch, false)?;
        let runtime =
            load(&tx, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
        exact(&runtime, launch)?;
        if effects.len() > 1000 {
            return Err(error("durable-effect-limit"));
        }
        let mut known = BTreeMap::new();
        {
            let mut statement = tx.prepare("SELECT resource_id FROM agent_assignment_resources WHERE assignment_id = ?1 AND resource_kind = 'interrupted-effect' LIMIT 1001").map_err(storage_error)?;
            let rows = statement
                .query_map([&launch.assignment_id], |row| row.get::<_, String>(0))
                .map_err(storage_error)?;
            for row in rows {
                let effect: InterruptedEffect = serde_json::from_str(&row.map_err(storage_error)?)
                    .map_err(|_| error("durable-storage-invalid"))?;
                effect.validate()?;
                if known.insert(effect.task_id.clone(), effect).is_some() {
                    return Err(error("durable-effect-conflict"));
                }
            }
        }
        for effect in effects {
            effect.validate()?;
            if let Some(previous) = known.get(&effect.task_id) {
                if previous != effect {
                    return Err(error("durable-effect-conflict"));
                }
            } else {
                known.insert(effect.task_id.clone(), effect.clone());
            }
        }
        if known.len() > 1000 {
            return Err(error("durable-effect-limit"));
        }
        for effect in effects {
            // This existing opaque resource contract supplies atomic identity,
            // original timestamp and retained capacity, without a second ledger.
            tx.execute("INSERT OR IGNORE INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) VALUES (?1, 'interrupted-effect', ?2, ?3)",
                params![launch.assignment_id, serde_json::to_string(effect).map_err(|_| error("durable-storage-invalid"))?, now_millis()?],
            ).map_err(storage_error)?;
        }
        if !known.is_empty() {
            tx.execute("INSERT OR IGNORE INTO dispatch_holds(record_id, reason) SELECT record_id, 'Durable execution has interrupted external effects with unknown outcomes; inspect protected runtime evidence. No automatic replay or recovery is authorized.' FROM agent_assignments WHERE id = ?1", [&launch.assignment_id]).map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)
    }
}
