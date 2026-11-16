//! Administrative closure: observe gates and reserve safety in the same writer transaction.
use super::{SqliteProjectRepository, execution_modes, insert_update, load_record, storage_error};
use forge_app::completion::{BypassedGate, CompletionResult, ForcedCompletion};
use forge_app::{Record, RecordStatus, RecordType, RecordUpdateKind, RegistrationError};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn schema(c: &Connection) -> Result<(), RegistrationError> {
    c.execute_batch(include_str!("upgrade/forced_completions.sql"))
        .map_err(storage_error)
}

pub(super) fn load(c: &Connection, id: u64) -> Result<Option<ForcedCompletion>, RegistrationError> {
    c.query_row("SELECT update_id, actor, resolution, bypassed_gates_json FROM forced_completions WHERE record_id = ?1", [id.cast_signed()], |r| {
        let json: String = r.get(3)?;
        Ok((r.get::<_, i64>(0)?.cast_unsigned(), r.get::<_, String>(1)?, r.get::<_, String>(2)?, json))
    }).optional().map_err(storage_error)?.map(|(update_id, actor, resolution, json)| {
        Ok(ForcedCompletion { update_id, actor, resolution, bypassed_gates: serde_json::from_str(&json).map_err(|e| RegistrationError::new("storage", e.to_string()))? })
    }).transpose()
}

fn observe(
    gates: &mut Vec<BypassedGate>,
    result: Result<(), RegistrationError>,
) -> Result<(), RegistrationError> {
    if let Err(e) = result {
        // Only known workflow denials are bypassable; storage/invariant failures remain errors.
        match e.code {
            "stack-blocked" | "invalid-spec-plan" | "spec-execution-blocked" => {
                gates.push(BypassedGate {
                    code: e.code.into(),
                    message: e.message,
                });
            }
            _ => return Err(e),
        }
    }
    Ok(())
}

fn gates(c: &Connection, record: &Record) -> Result<Vec<BypassedGate>, RegistrationError> {
    let mut gates = Vec::new();
    if record.status == RecordStatus::Open {
        gates.push(BypassedGate {
            code: "not-started".into(),
            message: "Record is Open; no start was performed".into(),
        });
    }
    let mut children = c.prepare("SELECT child.id FROM record_relationships r JOIN records child ON child.id = r.child_id WHERE r.parent_id = ?1 AND r.kind = 'parent-child' AND child.status != 'done' ORDER BY child.id").map_err(storage_error)?;
    let children = children
        .query_map([record.id.cast_signed()], |r| r.get::<_, i64>(0))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !children.is_empty() {
        gates.push(BypassedGate {
            code: "children-not-done".into(),
            message: format!("unfinished child IDs: {children:?}"),
        });
    }
    let prerequisites = super::unfinished_prerequisite_ids(c, record.id)?;
    if !prerequisites.is_empty() {
        let e = super::dependencies_not_done_error(&prerequisites);
        gates.push(BypassedGate {
            code: e.code.into(),
            message: e.message,
        });
    }
    if record.record_type == RecordType::Spec {
        observe(&mut gates, super::spec_plans::validate(c, record.id))?;
    }
    if record.record_type == RecordType::Task {
        // Record parent execution and legacy-plan conditions independently from completion gates.
        for relationship in &record.relationships {
            if relationship.kind == forge_app::RecordRelationshipKind::ParentChild
                && relationship.child_id == record.id
            {
                let parent = load_record(c, relationship.parent_id)?;
                if parent.record_type == RecordType::Spec {
                    observe(
                        &mut gates,
                        forge_app::spec_plans::validate_execution_parent(record.id, &parent),
                    )?;
                    observe(&mut gates, super::spec_plans::validate(c, parent.id))?;
                }
            }
        }
        if record.execution_mode == forge_app::ExecutionMode::Dispatched {
            gates.push(BypassedGate {
                code: "dispatched-completion-forbidden".into(),
                message: "ordinary Task completion requires Manual Execution Mode".into(),
            });
        }
        super::checks::completion_bypasses(c, record.id, &mut gates)?;
    }
    Ok(gates)
}

impl SqliteProjectRepository {
    pub(super) fn force_complete(
        &self,
        id: u64,
        resolution: Option<&str>,
        actor: &str,
    ) -> Result<CompletionResult, RegistrationError> {
        let mut c = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock poisoned"))?;
        let tx = c
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        self.ensure_record_workspace(&tx, id)?;
        let record = load_record(&tx, id)?;
        // A retry cannot rewrite any metadata, even with a missing/changed explanation.
        if record.status == RecordStatus::Done {
            return Ok(CompletionResult {
                record,
                already_done: true,
            });
        }
        let resolution = resolution.filter(|r| !r.trim().is_empty()).ok_or_else(|| {
            RegistrationError::new(
                "invalid-resolution",
                "forced completion requires a nonblank completion resolution",
            )
        })?;
        if actor.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-actor",
                "operator attribution must be nonblank",
            ));
        }
        execution_modes::safe_ownership(&tx, id)?;
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
                "close or invalidate the current attempt before forced completion",
            ));
        }
        let unsafe_checkout: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM worktree_claims WHERE record_id = ?1 AND state != 'active')", [id.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
        if unsafe_checkout {
            return Err(RegistrationError::new(
                "worktree-unsafe",
                "resolve the reserving or cleanup-required Managed Worktree before forced completion",
            ));
        }
        let bypassed = gates(&tx, &record)?;
        let json = serde_json::to_string(&bypassed)
            .map_err(|e| RegistrationError::new("storage", e.to_string()))?;
        tx.execute(
            "UPDATE records SET status = 'done', completion_resolution = ?2 WHERE id = ?1",
            params![id.cast_signed(), resolution],
        )
        .map_err(storage_error)?;
        insert_update(
            &tx,
            id,
            RecordUpdateKind::StatusTransition {
                from: record.status,
                to: RecordStatus::Done,
            },
        )?;
        let update_id = tx.last_insert_rowid();
        tx.execute("INSERT INTO forced_completions (record_id, update_id, actor, resolution, bypassed_gates_json) VALUES (?1, ?2, ?3, ?4, ?5)", params![id.cast_signed(), update_id, actor, resolution, json]).map_err(storage_error)?;
        let record = load_record(&tx, id)?;
        tx.commit().map_err(storage_error)?;
        Ok(CompletionResult {
            record,
            already_done: false,
        })
    }
}
