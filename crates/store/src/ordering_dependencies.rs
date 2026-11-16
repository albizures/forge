//! Explicit dependency intent; ordinary dependencies retain their ordering semantics.
use forge_app::{Record, RecordRelationshipKind, RecordUpdateKind, RegistrationError};
use rusqlite::{Connection, TransactionBehavior, params};

use super::{
    SqliteProjectRepository, insert_relationship, insert_update, load_record, record_workspaces,
    stacks, storage_error,
};

pub(super) fn schema(c: &Connection) -> Result<(), RegistrationError> {
    c.execute_batch(include_str!("upgrade/ordering_dependencies.sql"))
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
        if kind == RecordRelationshipKind::DependsOn {
            super::spec_plans::editable_task(&tx, from)?;
        }
        if ordering_only
            && (kind != RecordRelationshipKind::DependsOn
                || stacks::load(&tx, from)?.is_some_and(|s| s.predecessor == Some(to)))
        {
            return Err(RegistrationError::new(
                "stack-blocked",
                "a Stack Predecessor cannot be an ordering-only dependency",
            ));
        }
        insert_relationship(&tx, &child, &parent, kind)?;
        if ordering_only {
            tx.execute("INSERT INTO ordering_only_dependencies(from_id, to_id, kind) VALUES (?1, ?2, 'depends-on')", params![from.cast_signed(), to.cast_signed()]).map_err(storage_error)?;
        }
        for (id, other) in [(from, to), (to, from)] {
            insert_update(
                &tx,
                id,
                RecordUpdateKind::RelationshipAdded {
                    related_record_id: other,
                    relationship_kind: Some(kind),
                },
            )?;
            if ordering_only {
                tx.execute("INSERT INTO record_comments(record_id, author, body, created_at_millis) VALUES (?1, 'forge-dependency', ?2, ?3)", params![id.cast_signed(), format!("Waits-For Relationship designated: {from} waits for {to}"), super::now_millis()?]).map_err(storage_error)?;
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
