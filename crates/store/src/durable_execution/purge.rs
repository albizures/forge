//! Transactional runtime purge journal. No filesystem, process or runner calls.
use super::{authorize, error, exact, load, scope};
use crate::{SqliteProjectRepository, now_millis, storage_error};
use forge_app::{
    RegistrationError,
    durable_execution::{
        Admission, Launch,
        purge::{Finish, Record, Removal, Repository, State},
    },
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

type Row = (
    String,
    String,
    Option<String>,
    i64,
    Option<i64>,
    Option<String>,
);

fn read(c: &Connection, assignment: &str) -> Result<Option<Record>, RegistrationError> {
    let row: Option<Row> = c
        .query_row(
            "SELECT p.operation_id, p.state, p.diagnostic, p.requested_at_millis, p.completed_at_millis, r.removal FROM durable_runtime_purges p LEFT JOIN durable_runtime_purge_removals r ON r.assignment_id = p.assignment_id WHERE p.assignment_id = ?1",
            [assignment],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()
        .map_err(storage_error)?;
    row.map(
        |(operation_id, state, diagnostic, requested, completed, removal)| {
            let millis = |v: i64| u64::try_from(v).map_err(|_| error("durable-storage-invalid"));
            Ok(Record {
                assignment_id: assignment.into(),
                operation_id,
                state: State::parse(&state)?,
                diagnostic,
                requested_at_millis: millis(requested)?,
                completed_at_millis: completed.map(millis).transpose()?,
                removal: removal.as_deref().map(Removal::parse).transpose()?,
            })
        },
    )
    .transpose()
}

/// Closed and fully reconciled. Resources that are still unretired (including
/// unknown identities, effects and recovery claims) and Dispatch Holds block deletion.
fn eligible(c: &Connection, workspace: &str, launch: &Launch) -> Result<(), RegistrationError> {
    authorize(c, workspace, launch, false)?;
    let runtime = load(c, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
    exact(&runtime, launch)?;
    let blocked = |code: &'static str| {
        Err(RegistrationError::new(
            code,
            "runtime history may be deleted only for a closed, retired Assignment with no retained uncertainty",
        ))
    };
    let (closed, held, open_resources): (bool, bool, bool) = c
        .query_row(
            "SELECT a.closed_at_millis IS NOT NULL, EXISTS(SELECT 1 FROM dispatch_holds h WHERE h.record_id = a.record_id), EXISTS(SELECT 1 FROM agent_assignment_resources r WHERE r.assignment_id = a.id AND r.retired_at_millis IS NULL) FROM agent_assignments a WHERE a.id = ?1",
            [&launch.assignment_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(storage_error)?;
    if !closed {
        return blocked("runtime-purge-assignment-open");
    }
    if held || open_resources {
        return blocked("runtime-purge-uncertainty-retained");
    }
    match runtime.admission {
        Admission::Rejected { .. } => Ok(()),
        Admission::Accepted { .. } => {
            let reconciled: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM durable_assignment_retirements WHERE assignment_id = ?1 AND reconciled_at_millis IS NOT NULL)",
                    [&launch.assignment_id],
                    |r| r.get(0),
                )
                .map_err(storage_error)?;
            if reconciled {
                Ok(())
            } else {
                blocked("runtime-purge-retirement-unreconciled")
            }
        }
        _ => blocked("runtime-purge-admission-unknown"),
    }
}

impl Repository for SqliteProjectRepository {
    fn begin_purge(
        &self,
        workspace: &str,
        launch: &Launch,
        operation_id: &str,
    ) -> Result<Record, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        scope(&tx, workspace, &launch.assignment_id)?;
        if let Some(existing) = read(&tx, &launch.assignment_id)? {
            if existing.operation_id != operation_id {
                return Err(error("runtime-purge-conflict"));
            }
            // A completed purge is returned as accepted history. A pending or retained
            // one is revalidated, because deletion will run again.
            if existing.state != State::Purged {
                eligible(&tx, workspace, launch)?;
                tx.execute(
                    "UPDATE durable_runtime_purges SET state = 'pending', diagnostic = NULL WHERE assignment_id = ?1",
                    [&launch.assignment_id],
                )
                .map_err(storage_error)?;
            }
            let record = read(&tx, &launch.assignment_id)?;
            tx.commit().map_err(storage_error)?;
            return record.ok_or_else(|| error("durable-storage-invalid"));
        }
        eligible(&tx, workspace, launch)?;
        tx.execute(
            "INSERT INTO durable_runtime_purges(assignment_id, operation_id, state, requested_at_millis) VALUES (?1, ?2, 'pending', ?3)",
            params![launch.assignment_id, operation_id, now_millis()?],
        )
        .map_err(storage_error)?;
        let record =
            read(&tx, &launch.assignment_id)?.ok_or_else(|| error("durable-storage-invalid"))?;
        tx.commit().map_err(storage_error)?;
        Ok(record)
    }

    fn finish_purge(
        &self,
        workspace: &str,
        launch: &Launch,
        operation_id: &str,
        finish: &Finish,
    ) -> Result<Record, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        scope(&tx, workspace, &launch.assignment_id)?;
        let existing = read(&tx, &launch.assignment_id)?
            .filter(|r| r.operation_id == operation_id)
            .ok_or_else(|| error("runtime-purge-intent-missing"))?;
        if existing.state == State::Purged {
            tx.commit().map_err(storage_error)?;
            return Ok(existing);
        }
        match finish {
            Finish::Purged { removal } => tx
                .execute(
                    "UPDATE durable_runtime_purges SET state = 'purged', diagnostic = NULL, completed_at_millis = ?2 WHERE assignment_id = ?1",
                    params![launch.assignment_id, now_millis()?],
                )
                .and_then(|_| {
                    tx.execute(
                        "INSERT INTO durable_runtime_purge_removals(assignment_id, removal) VALUES (?1, ?2)",
                        params![launch.assignment_id, removal.as_str()],
                    )
                }),
            Finish::Retained { code } => tx.execute(
                "UPDATE durable_runtime_purges SET state = 'retained', diagnostic = ?2 WHERE assignment_id = ?1",
                params![launch.assignment_id, code],
            ),
        }
        .map_err(storage_error)?;
        let record =
            read(&tx, &launch.assignment_id)?.ok_or_else(|| error("durable-storage-invalid"))?;
        tx.commit().map_err(storage_error)?;
        Ok(record)
    }

    fn purge_record(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<Option<Record>, RegistrationError> {
        let c = self.connection.lock().map_err(|_| error("storage"))?;
        scope(&c, workspace, assignment)?;
        read(&c, assignment)
    }
}
