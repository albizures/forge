//! Failed durable launches (T26): retained diagnostics and reconciliation of a
//! launch that never received a grant. No host I/O happens here; the caller
//! holds any exclusive inspection guard through these commits.
use super::{authorize, error, exact, load, scope};
use crate::{SqliteProjectRepository, now_millis, storage_error};
use forge_app::{
    RegistrationError,
    durable_execution::{
        Launch,
        launch_failure::{Facts, Failure, Proof, Repository, reconciles_resource, validate},
    },
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

impl Repository for SqliteProjectRepository {
    fn record_launch_failure(
        &self,
        workspace: &str,
        launch: &Launch,
        failure: Failure,
    ) -> Result<(), RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        authorize(&tx, workspace, launch, false)?;
        let runtime =
            load(&tx, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
        exact(&runtime, launch)?;
        tx.execute(
            "INSERT OR IGNORE INTO agent_assignment_resources (assignment_id, resource_kind, resource_id, created_at_millis) VALUES (?1, 'durable-launch-failure', ?2, ?3)",
            params![launch.assignment_id, failure.code(), now_millis()?],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }

    fn reconcile_ungranted_launch(
        &self,
        workspace: &str,
        launch: &Launch,
        proof: Proof,
    ) -> Result<(), RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        authorize(&tx, workspace, launch, false)?;
        let runtime =
            load(&tx, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
        exact(&runtime, launch)?;
        let (closed, launch_started, lease_count, retirement_begun): (bool, bool, u32, bool) = tx
            .query_row(
                "SELECT a.closed_at_millis IS NOT NULL, a.launch_started_at_millis IS NOT NULL, (SELECT COUNT(*) FROM assignment_authorization_leases l WHERE l.assignment_id = a.id), EXISTS(SELECT 1 FROM durable_assignment_retirements r WHERE r.assignment_id = a.id) FROM agent_assignments a WHERE a.id = ?1",
                [&launch.assignment_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(storage_error)?;
        validate(
            &Facts {
                closed,
                admission: runtime.admission,
                lease_count,
                launch_started,
                retirement_begun,
            },
            proof,
        )?;
        let now = now_millis()?;
        // Immutable journal; an exact retry keeps the original proof and time.
        tx.execute(
            "INSERT OR IGNORE INTO durable_launch_reconciliations (assignment_id, proof, reconciled_at_millis) VALUES (?1, ?2, ?3)",
            params![launch.assignment_id, proof.code(), now],
        )
        .map_err(storage_error)?;
        let resources = tx
            .prepare("SELECT resource_kind, resource_id FROM agent_assignment_resources WHERE assignment_id = ?1 AND retired_at_millis IS NULL")
            .map_err(storage_error)?
            .query_map([&launch.assignment_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        for (kind, id) in resources {
            if reconciles_resource(launch, &kind, &id) {
                tx.execute(
                    "UPDATE agent_assignment_resources SET retired_at_millis = ?4 WHERE assignment_id = ?1 AND resource_kind = ?2 AND resource_id = ?3 AND retired_at_millis IS NULL",
                    params![launch.assignment_id, kind, id, now],
                )
                .map_err(storage_error)?;
            }
        }
        tx.commit().map_err(storage_error)
    }

    fn reconcile_unreserved_claim(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<(), RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        scope(&tx, workspace, assignment)?;
        let closed: bool = tx
            .query_row(
                "SELECT closed_at_millis IS NOT NULL FROM agent_assignments WHERE id = ?1",
                [assignment],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or_else(|| error("durable-scope-denied"))?;
        if !closed {
            return Err(RegistrationError::new(
                "assignment-not-revoked",
                "an unreserved launch claim is discharged only after revocation",
            ));
        }
        if load(&tx, assignment)?.is_some() {
            return Err(RegistrationError::new(
                "durable-launch-reserved",
                "a reserved runtime needs its own launch reconciliation",
            ));
        }
        // Closed: `claim`/`reserve` can never add a runtime now (both require an
        // open Assignment in the same transaction). The claim launched nothing.
        tx.execute(
            "UPDATE agent_assignment_resources SET retired_at_millis = ?2 WHERE assignment_id = ?1 AND resource_kind = 'provision-intent' AND resource_id = 'once' AND retired_at_millis IS NULL",
            params![assignment, now_millis()?],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }
}
