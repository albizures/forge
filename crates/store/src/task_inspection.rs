//! Bounded read-only activity projection. No provider/resource identities leave storage.
use super::{SqliteProjectRepository, assignment_state, load_record, storage_error};
use forge_app::{
    RegistrationError,
    task_inspection::{AssignmentActivity, TaskActivity},
};
use rusqlite::OptionalExtension;

impl SqliteProjectRepository {
    pub(super) fn inspect_task_activity(
        &self,
        record_id: u64,
    ) -> Result<TaskActivity, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let tx = connection.transaction().map_err(storage_error)?;
        load_record(&tx, record_id)?;
        // Prefer an open attempt, otherwise show the most recent historical attempt.
        let latest = tx.query_row(
            "SELECT id, state, closed_at_millis, acknowledged_at_millis FROM agent_assignments WHERE record_id = ?1 ORDER BY (closed_at_millis IS NULL) DESC, attempt_number DESC LIMIT 1",
            [record_id.cast_signed()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<i64>>(2)?, row.get::<_, Option<i64>>(3)?)),
        ).optional().map_err(storage_error)?;
        let assignment = latest
            .map(|(id, state, closed, acknowledged)| {
                Ok(AssignmentActivity {
                    id,
                    state: assignment_state(&state)?,
                    closed_at_millis: closed.map(i64::cast_unsigned),
                    acknowledged_at_millis: acknowledged.map(i64::cast_unsigned),
                })
            })
            .transpose()?;
        let retained_resources = tx.query_row(
            "SELECT COUNT(*) FROM agent_assignment_resources r JOIN agent_assignments a ON a.id = r.assignment_id WHERE a.record_id = ?1 AND r.retired_at_millis IS NULL",
            [record_id.cast_signed()], |row| row.get::<_, i64>(0).map(i64::cast_unsigned),
        ).map_err(storage_error)?;
        let dispatch_hold = tx
            .query_row(
                "SELECT reason FROM dispatch_holds WHERE record_id = ?1",
                [record_id.cast_signed()],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let stack = super::stacks::load(&tx, record_id)?
            .map(|stack| {
                let mut inspection = forge_app::task_inspection::StackInspection {
                    predecessor: stack.predecessor,
                    expected_commit: stack.base_commit,
                    verified_predecessor_commit: None,
                    problem: None,
                };
                if let Some(predecessor) = stack.predecessor {
                    let observed = super::stacks::load(&tx, predecessor)?.map(|previous| {
                        let work = super::outcomes::state(&tx, predecessor)?;
                        previous.verified_commit(&work).map(str::to_owned)
                    });
                    match observed {
                        Some(Ok(commit)) => {
                            if inspection
                                .expected_commit
                                .as_ref()
                                .is_some_and(|expected| expected != &commit)
                            {
                                inspection.problem = Some(
                                    "prepared input differs from verified predecessor result"
                                        .into(),
                                );
                            }
                            inspection.verified_predecessor_commit = Some(commit);
                        }
                        Some(Err(error)) => inspection.problem = Some(error.message),
                        None => {
                            inspection.problem =
                                Some("predecessor Stack evidence is absent".into());
                        }
                    }
                }
                Ok::<_, RegistrationError>(inspection)
            })
            .transpose()?;
        tx.commit().map_err(storage_error)?;
        Ok(TaskActivity {
            assignment,
            retained_resources,
            dispatch_hold,
            stack,
        })
    }
}
