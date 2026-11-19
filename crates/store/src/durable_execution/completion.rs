//! Atomic missing-Outcome retention; no runtime calls or lifecycle transitions.
use super::{authorize, error, exact, load};
use crate::{SqliteProjectRepository, now_millis, storage_error};
use forge_app::{
    RegistrationError,
    durable_execution::{
        Admission, Launch,
        completion::{AnswerObservation, Repository},
    },
};
use rusqlite::{TransactionBehavior, params};

impl Repository for SqliteProjectRepository {
    fn observe_answer(
        &self,
        workspace: &str,
        launch: &Launch,
    ) -> Result<AnswerObservation, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        // An answer may arrive after revocation or invalidation. It supplies no
        // authority, but must still belong to the retained runtime and attempt.
        authorize(&tx, workspace, launch, false)?;
        let runtime =
            load(&tx, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
        exact(&runtime, launch)?;
        if !matches!(runtime.admission, Admission::Accepted { .. }) {
            return Err(error("durable-answer-before-admission"));
        }
        let accepted: bool = tx
            .query_row(
                "SELECT outcome_json IS NOT NULL AND result_json IS NOT NULL FROM record_attempts WHERE id = ?1",
                [&launch.attempt_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let observation = if accepted {
            AnswerObservation::OutcomeAccepted
        } else {
            // Store only a static diagnostic. Prose, tool output and capability
            // material never cross this boundary. Retention is not proof of exit.
            tx.execute(
                "INSERT OR IGNORE INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) VALUES (?1, 'missing-outcome', ?2, ?3)",
                params![launch.assignment_id, launch.attempt_id, now_millis()?],
            ).map_err(storage_error)?;
            tx.execute(
                "INSERT OR IGNORE INTO dispatch_holds(record_id, reason) SELECT record_id, 'Durable runtime answered without an accepted Outcome; inspect the bound Assignment and attempt before intervention. Runtime completion does not establish safe retirement.' FROM agent_assignments WHERE id = ?1",
                [&launch.assignment_id],
            ).map_err(storage_error)?;
            AnswerObservation::MissingOutcome
        };
        tx.commit().map_err(storage_error)?;
        Ok(observation)
    }
}
