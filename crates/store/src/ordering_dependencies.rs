//! Explicit dependency intent; ordinary dependencies retain their ordering semantics.
use forge_app::{Record, RecordRelationshipKind, RecordUpdateKind, RegistrationError};
use rusqlite::{Connection, TransactionBehavior, params};

use super::{
    SqliteProjectRepository, insert_relationship, insert_update, load_record, record_workspaces,
    stacks, storage_error,
};

pub(super) fn schema(c: &Connection) -> Result<(), RegistrationError> {
    c.execute_batch(include_str!("upgrade/ordering_dependencies.sql"))
        .map_err(storage_error)?;
    c.execute_batch(include_str!("upgrade/builds_on_dependencies.sql"))
        .map_err(storage_error)
}

pub(super) fn designated(c: &Connection, from: u64, to: u64) -> Result<bool, RegistrationError> {
    // Offline inspection also reads frozen pre-designation schemas. Normal opens
    // reject them before initialization; absence never invents an exception.
    let exists: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'ordering_only_dependencies')", [], |r| r.get(0)).map_err(storage_error)?;
    if !exists {
        return Ok(false);
    }
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM ordering_only_dependencies WHERE from_id = ?1 AND to_id = ?2)",
        params![from.cast_signed(), to.cast_signed()],
        |r| r.get(0),
    )
    .map_err(storage_error)
}

/// Validate removal intent in the caller's writer transaction, including retained state.
pub(super) fn removal_intent(
    c: &Connection,
    from: u64,
    to: u64,
    kind: RecordRelationshipKind,
) -> Result<RecordRelationshipKind, RegistrationError> {
    let kind = if kind == RecordRelationshipKind::WaitsFor {
        if !designated(c, from, to)? {
            return Err(RegistrationError::new(
                "relationship-not-found",
                "Waits-For Relationship does not exist; legacy intent must be removed explicitly",
            ));
        }
        RecordRelationshipKind::DependsOn
    } else {
        kind
    };
    if kind == RecordRelationshipKind::BuildsOn {
        if stacks::builds_on(c, from)? != Some(to) {
            return Err(RegistrationError::new(
                "relationship-not-found",
                "Builds-On Relationship does not exist",
            ));
        }
        stacks::editable_topology(c, from)?;
        c.execute(
            "DELETE FROM task_stacks WHERE record_id = ?1",
            [from.cast_signed()],
        )
        .map_err(storage_error)?;
    }
    if kind == RecordRelationshipKind::DependsOn {
        if stacks::builds_on(c, from)? == Some(to) {
            return Err(RegistrationError::new(
                "invalid-relationship",
                "remove the Builds-On Relationship explicitly",
            ));
        }
        super::spec_plans::editable_task(c, from)?;
    }
    Ok(kind)
}

fn validate_builds_on(
    c: &Connection,
    child: &Record,
    parent: &Record,
) -> Result<(), RegistrationError> {
    forge_app::stacks::validate_stack_predecessor(child, parent)?;
    stacks::editable_topology(c, child.id)?;
    let competing: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM builds_on_dependencies WHERE from_id = ?1 OR to_id = ?2)
         OR EXISTS(SELECT 1 FROM task_stacks WHERE record_id = ?1 OR predecessor = ?2)",
            params![child.id.cast_signed(), parent.id.cast_signed()],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if competing {
        return Err(RegistrationError::new(
            "stack-blocked",
            "remove existing predecessor, successor or root configuration explicitly before declaring Builds-On",
        ));
    }
    Ok(())
}

impl SqliteProjectRepository {
    pub(super) fn relate_with_intent(
        &self,
        from: u64,
        to: u64,
        kind: RecordRelationshipKind,
        ordering_only: bool,
    ) -> Result<Record, RegistrationError> {
        if from == to {
            return Err(RegistrationError::new(
                "invalid-relationship",
                "a Record cannot relate to itself",
            ));
        }
        let mut c = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let child = load_record(&tx, from)?;
        let parent = load_record(&tx, to)?;
        self.ensure_record_workspace(&tx, from)?;
        self.ensure_record_workspace(&tx, to)?;
        record_workspaces::same_workspace(&tx, from, to)?;
        let builds_on = kind == RecordRelationshipKind::BuildsOn;
        let stored_kind = if builds_on {
            RecordRelationshipKind::DependsOn
        } else {
            kind
        };
        if stored_kind == RecordRelationshipKind::DependsOn {
            super::spec_plans::editable_task(&tx, from)?;
        }
        if builds_on {
            validate_builds_on(&tx, &child, &parent)?;
        }
        if ordering_only
            && (kind != RecordRelationshipKind::DependsOn
                || stacks::predecessor(&tx, from)? == Some(to))
        {
            return Err(RegistrationError::new(
                "stack-blocked",
                "a Stack Predecessor cannot be an ordering-only dependency",
            ));
        }
        insert_relationship(&tx, &child, &parent, stored_kind)?;
        if builds_on {
            tx.execute(
                "INSERT INTO builds_on_dependencies(from_id, to_id) VALUES (?1, ?2)",
                params![from.cast_signed(), to.cast_signed()],
            )
            .map_err(storage_error)?;
            // Execution evidence has no independent topology for canonical successors.
            tx.execute(
                "INSERT INTO task_stacks(record_id, source_json) VALUES (?1, '{}')",
                [from.cast_signed()],
            )
            .map_err(storage_error)?;
            tx.execute(
                "DELETE FROM task_base_commits WHERE record_id = ?1",
                [from.cast_signed()],
            )
            .map_err(storage_error)?;
        }
        if ordering_only {
            tx.execute("INSERT INTO ordering_only_dependencies(from_id, to_id, kind) VALUES (?1, ?2, 'depends-on')", params![from.cast_signed(), to.cast_signed()]).map_err(storage_error)?;
        }
        for (id, other) in [(from, to), (to, from)] {
            insert_update(
                &tx,
                id,
                RecordUpdateKind::RelationshipAdded {
                    related_record_id: other,
                    relationship_kind: Some(stored_kind),
                },
            )?;
            if ordering_only || builds_on {
                let message = if builds_on {
                    format!("Builds-On Relationship designated: {from} builds on {to}")
                } else {
                    format!("Waits-For Relationship designated: {from} waits for {to}")
                };
                tx.execute("INSERT INTO record_comments(record_id, author, body, created_at_millis) VALUES (?1, 'forge-dependency', ?2, ?3)", params![id.cast_signed(), message, super::now_millis()?]).map_err(storage_error)?;
                insert_update(
                    &tx,
                    id,
                    RecordUpdateKind::CommentAdded {
                        comment_id: forge_app::RecordCommentId(
                            tx.last_insert_rowid().cast_unsigned(),
                        ),
                    },
                )?;
            }
        }
        let record = load_record(&tx, from)?;
        tx.commit().map_err(storage_error)?;
        Ok(record)
    }
}
