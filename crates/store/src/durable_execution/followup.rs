//! Transactional follow-up recording. No runner, model, tool, lease or process calls.
use super::{error, load, scope};
use crate::{SqliteProjectRepository, now_millis, outcomes, storage_error, task_attempts};
use forge_app::{
    RegistrationError,
    durable_execution::{
        Admission,
        followup::{Binding, Delivery, Receipt, Repository, Request},
    },
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

/// Retained facts that make a message unsafe to deliver. Missing-Outcome is
/// deliberately absent: explicit intervention is how that state is investigated.
const BLOCKING_KINDS: [&str; 5] = [
    "provision-intent",
    "interrupted-effect",
    "authorization-uncertain",
    "recovery-claim",
    "retirement-pending",
];

type Row = (
    String,
    u32,
    String,
    String,
    u64,
    Option<String>,
    Option<String>,
);

fn row_receipt(assignment: &str, row: Row, replayed: bool) -> Result<Receipt, RegistrationError> {
    let (operation_id, sequence, sha, binding, recorded, state, admission_id) = row;
    Ok(Receipt {
        assignment_id: assignment.into(),
        operation_id,
        sequence,
        message_sha256: sha,
        binding: serde_json::from_str(&binding).map_err(|_| error("durable-storage-invalid"))?,
        recorded_at_millis: recorded,
        replayed,
        delivery: state.unwrap_or_else(|| "recorded".into()),
        admission_id,
    })
}

fn existing(
    c: &Connection,
    assignment: &str,
    operation: &str,
) -> Result<Option<Row>, RegistrationError> {
    c.query_row(
        "SELECT f.operation_id, f.sequence, f.message_sha256, f.binding_json, f.recorded_at_millis, d.state, d.admission_id FROM durable_assignment_followups f LEFT JOIN durable_assignment_followup_deliveries d ON d.assignment_id = f.assignment_id AND d.operation_id = f.operation_id WHERE f.assignment_id = ?1 AND f.operation_id = ?2",
        params![assignment, operation],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)?.cast_unsigned(), r.get(5)?, r.get(6)?)),
    ).optional().map_err(storage_error)
}

/// New input is accepted only for an open, admitted, fresh-bound, unblocked Assignment.
fn gate(tx: &Connection, request: &Request) -> Result<Binding, RegistrationError> {
    let runtime = load(tx, &request.assignment_id)?.ok_or_else(|| {
        RegistrationError::new(
            "durable-intent-missing",
            "Assignment has no retained durable runtime",
        )
    })?;
    if !matches!(runtime.admission, Admission::Accepted { .. }) {
        return Err(RegistrationError::new(
            "followup-admission-unconfirmed",
            "execution admission is not confirmed accepted; reconcile admission first",
        ));
    }
    let closed: bool = tx
        .query_row(
            "SELECT closed_at_millis IS NOT NULL FROM agent_assignments WHERE id = ?1",
            [&request.assignment_id],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if closed {
        return Err(RegistrationError::new(
            "followup-assignment-revoked",
            "the Assignment is closed; revoked Assignments accept no new execution input",
        ));
    }
    let blocked: bool = tx
            .query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM agent_assignment_resources WHERE assignment_id = ?1 AND retired_at_millis IS NULL AND resource_kind IN ({}))",
                    BLOCKING_KINDS.map(|k| format!("'{k}'")).join(",")
                ),
                [&request.assignment_id],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
    if blocked {
        return Err(RegistrationError::new(
            "followup-blocked-by-uncertainty",
            "retained interrupted-effect, authorization, recovery, retirement or provisioning uncertainty must be resolved first; a message cannot resume unsafe work",
        ));
    }
    let attempt = task_attempts::binding(tx, &request.assignment_id)?
        .ok_or_else(|| error("durable-binding-denied"))?;
    let open: bool = tx
        .query_row(
            "SELECT closed = 0 FROM record_attempts WHERE id = ?1",
            [&attempt.id],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if attempt.id != runtime.launch.attempt_id || !open {
        return Err(error("durable-binding-denied"));
    }
    if outcomes::state(tx, attempt.record_id)?.generation != attempt.generation {
        return Err(error("durable-binding-stale"));
    }
    let binding = Binding {
        attempt_id: attempt.id,
        check_id: attempt.check_id,
        generation: attempt.generation,
        profile_revision: runtime.launch.profile_revision,
        runtime_id: runtime.launch.runtime_id,
    };
    Ok(binding)
}

impl Repository for SqliteProjectRepository {
    fn record_followup(
        &self,
        workspace: &str,
        request: &Request,
    ) -> Result<Receipt, RegistrationError> {
        request.validate()?;
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        scope(&tx, workspace, &request.assignment_id)?;
        let digest = request.digest();
        if let Some(row) = existing(&tx, &request.assignment_id, &request.operation_id)? {
            // A lost reply after revocation still discovers the original record.
            if row.2 != digest {
                return Err(RegistrationError::new(
                    "followup-operation-conflict",
                    "this operation ID already recorded different content; choose a new operation ID",
                ));
            }
            return row_receipt(&request.assignment_id, row, true);
        }
        let binding = gate(&tx, request)?;
        let sequence: u32 = tx
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM durable_assignment_followups WHERE assignment_id = ?1",
                [&request.assignment_id],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        let now = now_millis()?;
        let binding_json =
            serde_json::to_string(&binding).map_err(|_| error("durable-storage-invalid"))?;
        tx.execute(
            "INSERT INTO durable_assignment_followups(assignment_id, operation_id, sequence, message, message_sha256, binding_json, recorded_at_millis) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![request.assignment_id, request.operation_id, sequence, request.message, digest, binding_json, now],
        ).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(Receipt {
            assignment_id: request.assignment_id.clone(),
            operation_id: request.operation_id.clone(),
            sequence,
            message_sha256: digest,
            binding,
            recorded_at_millis: now.cast_unsigned(),
            replayed: false,
            delivery: "recorded".into(),
            admission_id: None,
        })
    }

    fn followups(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<Vec<Receipt>, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c.transaction().map_err(storage_error)?;
        scope(&tx, workspace, assignment)?;
        let mut stmt = tx
            .prepare("SELECT f.operation_id, f.sequence, f.message_sha256, f.binding_json, f.recorded_at_millis, d.state, d.admission_id FROM durable_assignment_followups f LEFT JOIN durable_assignment_followup_deliveries d ON d.assignment_id = f.assignment_id AND d.operation_id = f.operation_id WHERE f.assignment_id = ?1 ORDER BY f.sequence")
            .map_err(storage_error)?;
        let rows = stmt
            .query_map([assignment], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get::<_, i64>(4)?.cast_unsigned(),
                    r.get(5)?,
                    r.get(6)?,
                ))
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows.into_iter()
            .map(|row| row_receipt(assignment, row, false))
            .collect()
    }

    fn undelivered_followups(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<Vec<Request>, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c.transaction().map_err(storage_error)?;
        scope(&tx, workspace, assignment)?;
        let mut stmt = tx
            .prepare("SELECT f.operation_id, f.message FROM durable_assignment_followups f LEFT JOIN durable_assignment_followup_deliveries d ON d.assignment_id = f.assignment_id AND d.operation_id = f.operation_id WHERE f.assignment_id = ?1 AND COALESCE(d.state, '') <> 'delivered' ORDER BY f.sequence")
            .map_err(storage_error)?;
        stmt.query_map([assignment], |r| {
            Ok(Request {
                assignment_id: assignment.into(),
                operation_id: r.get(0)?,
                message: r.get(1)?,
            })
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)
    }

    fn mark_followup(
        &self,
        workspace: &str,
        assignment: &str,
        operation_id: &str,
        delivery: &Delivery,
    ) -> Result<(), RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        scope(&tx, workspace, assignment)?;
        if existing(&tx, assignment, operation_id)?.is_none() {
            return Err(RegistrationError::new(
                "followup-not-recorded",
                "no recorded follow-up has this operation ID",
            ));
        }
        let now = now_millis()?;
        let current: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT state, admission_id FROM durable_assignment_followup_deliveries WHERE assignment_id = ?1 AND operation_id = ?2",
                params![assignment, operation_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        match (delivery, current) {
            (Delivery::Unknown, None) => {
                tx.execute("INSERT INTO durable_assignment_followup_deliveries(assignment_id, operation_id, state, admission_id, updated_at_millis) VALUES (?1, ?2, 'unknown', NULL, ?3)", params![assignment, operation_id, now]).map_err(storage_error)?;
            }
            (Delivery::Unknown, Some(_)) => {}
            (Delivery::Delivered { admission_id }, None) => {
                tx.execute("INSERT INTO durable_assignment_followup_deliveries(assignment_id, operation_id, state, admission_id, updated_at_millis) VALUES (?1, ?2, 'delivered', ?3, ?4)", params![assignment, operation_id, admission_id, now]).map_err(storage_error)?;
            }
            (Delivery::Delivered { admission_id }, Some((state, prior))) => {
                if state == "delivered" {
                    if prior.as_deref() != Some(admission_id.as_str()) {
                        return Err(error("durable-storage-invalid"));
                    }
                } else {
                    tx.execute("UPDATE durable_assignment_followup_deliveries SET state = 'delivered', admission_id = ?3, updated_at_millis = ?4 WHERE assignment_id = ?1 AND operation_id = ?2", params![assignment, operation_id, admission_id, now]).map_err(storage_error)?;
                }
            }
        }
        tx.commit().map_err(storage_error)
    }
}
