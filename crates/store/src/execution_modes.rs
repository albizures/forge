//! Transactional execution-policy handoff; never retires ownership or clears safety state.
use super::{SqliteProjectRepository, insert_update, load_record, outcomes, storage_error};
use forge_app::{
    ExecutionMode, Record, RecordStatus, RecordType, RecordUpdateKind, RegistrationError,
};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn parse(value: &str) -> Result<ExecutionMode, RegistrationError> {
    match value {
        "manual" => Ok(ExecutionMode::Manual),
        "dispatched" => Ok(ExecutionMode::Dispatched),
        _ => Err(RegistrationError::new("storage", "invalid Execution Mode")),
    }
}

pub(super) fn safe_ownership(c: &Connection, record: u64) -> Result<(), RegistrationError> {
    let unsafe_state: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM agent_assignments a WHERE a.record_id = ?1 AND (a.closed_at_millis IS NULL OR EXISTS(SELECT 1 FROM agent_assignment_resources r WHERE r.assignment_id = a.id AND r.retired_at_millis IS NULL) OR (a.closure_reason IS NOT 'retired' AND NOT EXISTS(SELECT 1 FROM assignment_unknown_resource_resolutions u WHERE u.assignment_id = a.id) AND NOT EXISTS(SELECT 1 FROM agent_assignment_resources r WHERE r.assignment_id = a.id))))", [record.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
    if unsafe_state {
        return Err(RegistrationError::new(
            "execution-mode-unsafe-ownership",
            "confirm Assignment retirement and reconcile retained/uncertain resources before changing Execution Mode or doing Manual work",
        ));
    }
    Ok(())
}

impl SqliteProjectRepository {
    pub(super) fn start_with_ownership(
        &self,
        id: u64,
        assignment: Option<&str>,
    ) -> Result<Record, RegistrationError> {
        let mut c = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock poisoned"))?;
        // Reserve the writer before reading prerequisites or ownership; wait within
        // the bounded busy timeout rather than failing a deferred read/write upgrade.
        let tx = c
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        self.ensure_record_workspace(&tx, id)?;
        let record = load_record(&tx, id)?;
        if let Some(assignment) = assignment {
            let owned: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_assignments WHERE id = ?1 AND record_id = ?2 AND closed_at_millis IS NULL)", params![assignment, id.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
            if !owned {
                return Err(RegistrationError::new(
                    "assignment-inactive",
                    "a matching open Assignment is required",
                ));
            }
        } else if record.record_type == RecordType::Task {
            if record.execution_mode == ExecutionMode::Dispatched {
                return Err(RegistrationError::new(
                    "dispatched-start-forbidden",
                    "ordinary operator start requires explicit Manual Execution Mode; start does not change mode",
                ));
            }
            safe_ownership(&tx, id)?;
        }
        if super::stacks::load(&tx, id)?.is_some_and(|s| s.base_commit.is_none()) {
            return Err(RegistrationError::new(
                "stack-blocked",
                "prepare the Task stack before starting",
            ));
        }
        let unfinished = super::unfinished_prerequisite_ids(&tx, id)?;
        if !unfinished.is_empty() {
            return Err(super::dependencies_not_done_error(&unfinished));
        }
        if tx
            .execute(
                "UPDATE records SET status = 'in_progress' WHERE id = ?1 AND status = 'open'",
                [id.cast_signed()],
            )
            .map_err(storage_error)?
            == 0
        {
            return super::record_or_transition_error(&tx, id);
        }
        insert_update(
            &tx,
            id,
            RecordUpdateKind::StatusTransition {
                from: RecordStatus::Open,
                to: RecordStatus::InProgress,
            },
        )?;
        let result = load_record(&tx, id)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }

    pub(super) fn change_execution_mode(
        &self,
        id: u64,
        expected: ExecutionMode,
        mode: ExecutionMode,
        actor: &str,
        message: &str,
    ) -> Result<Record, RegistrationError> {
        let mut c = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock poisoned"))?;
        let tx = c
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        self.ensure_record_workspace(&tx, id)?;
        let record = load_record(&tx, id)?;
        if actor.trim().is_empty() || message.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-execution-mode",
                "actor and explanation must be nonblank",
            ));
        }
        if record.status == RecordStatus::Done {
            return Err(RegistrationError::new(
                "invalid-transition",
                "Done Records cannot change Execution Mode",
            ));
        }
        if record.execution_mode != expected {
            return Err(RegistrationError::new(
                "stale-execution-mode",
                "Execution Mode changed; inspect the Record before retrying",
            ));
        }
        if expected == mode {
            return Ok(record);
        }
        safe_ownership(&tx, id)?;
        let open: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM record_attempts WHERE record_id = ?1 AND closed = 0)",
                [id.cast_signed()],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        if open {
            return Err(RegistrationError::new(
                "attempt-active",
                "close or explicitly invalidate the current attempt before changing Execution Mode",
            ));
        }
        if record.record_type == RecordType::Task {
            if mode == ExecutionMode::Dispatched {
                // Intrinsic validation also protects direct repository callers. External
                // prerequisites are checked by the application before this transaction.
                let invalid: Option<(i64, String)> = tx.query_row("SELECT c.id, c.name FROM record_checks c LEFT JOIN agent_profiles p ON p.record_type = 'task' AND p.revision = c.profile_revision WHERE c.record_id = ?1 AND (c.profile_revision IS NULL OR p.revision IS NULL OR p.launch_policy_json IS NULL) ORDER BY c.position LIMIT 1", [id.cast_signed()], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(storage_error)?;
                if let Some((check, name)) = invalid {
                    return Err(RegistrationError::new(
                        "invalid-check-profile",
                        format!(
                            "Check {check} ({name}): requires a valid pinned Agent Profile with registered launch policy"
                        ),
                    ));
                }
            }
            // A pristine Task has no work evidence or Assessments to invalidate.
            let has_work: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM record_work WHERE record_id = ?1) OR EXISTS(SELECT 1 FROM check_assessments a JOIN record_checks c ON c.id = a.check_id WHERE c.record_id = ?1)", [id.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
            if has_work {
                outcomes::invalidate(&tx, id, actor, message, None)?;
            }
        }
        tx.execute(
            "UPDATE records SET execution_mode = ?2 WHERE id = ?1",
            params![id.cast_signed(), mode.as_str()],
        )
        .map_err(storage_error)?;
        insert_update(
            &tx,
            id,
            RecordUpdateKind::ExecutionModeChanged {
                from: expected,
                to: mode,
                actor: actor.into(),
                message: message.into(),
            },
        )?;
        let result = load_record(&tx, id)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }
}
