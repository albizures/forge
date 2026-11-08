//! Durable Record ownership. A scoped handle is request-local, never a saved default.
use super::{SqliteProjectRepository, storage_error, validate_projects};
use forge_app::RegistrationError;
use rusqlite::{Connection, OptionalExtension};

pub(super) fn initialize(connection: &Connection) -> Result<(), RegistrationError> {
    let unowned: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM records WHERE workspace_id IS NULL)",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if unowned {
        return Err(RegistrationError::new(
            "record-workspace-required",
            "this development store contains Records without Workspace ownership; select a new store (legacy Record import is unsupported)",
        ));
    }
    widen_update_relationships(connection)?;
    connection
        .execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS record_initiative_membership ON record_relationships(from_id) WHERE kind = 'initiative';
             CREATE INDEX IF NOT EXISTS workspace_records ON records(workspace_id, id);
         CREATE TRIGGER IF NOT EXISTS record_ownership_required
         BEFORE INSERT ON records WHEN NEW.workspace_id IS NULL
         BEGIN SELECT RAISE(ABORT, 'Record requires Workspace ownership'); END;
         CREATE TRIGGER IF NOT EXISTS record_workspace_immutable
         BEFORE UPDATE OF workspace_id ON records
         BEGIN SELECT RAISE(ABORT, 'Record Workspace is immutable'); END;
         CREATE TRIGGER IF NOT EXISTS record_project_workspace
         BEFORE INSERT ON record_projects
         WHEN (SELECT workspace_id FROM records WHERE id = NEW.record_id) !=
              (SELECT workspace_id FROM projects WHERE slug = NEW.project_slug)
         BEGIN SELECT RAISE(ABORT, 'cross-Workspace Record Project scope'); END;
         CREATE TRIGGER IF NOT EXISTS record_relationship_workspace
         BEFORE INSERT ON record_relationships
         WHEN (SELECT workspace_id FROM records WHERE id = NEW.from_id) !=
              (SELECT workspace_id FROM records WHERE id = NEW.to_id)
         BEGIN SELECT RAISE(ABORT, 'cross-Workspace Record relationship'); END;
         CREATE TRIGGER IF NOT EXISTS record_project_scope_immutable
         BEFORE UPDATE ON record_projects
         BEGIN SELECT RAISE(ABORT, 'Record Project scope is immutable'); END;
         CREATE TRIGGER IF NOT EXISTS record_relationship_endpoints_immutable
         BEFORE UPDATE ON record_relationships
         BEGIN SELECT RAISE(ABORT, 'Record relationship endpoints are immutable'); END;",
        )
        .map_err(storage_error)
}

fn widen_update_relationships(connection: &Connection) -> Result<(), RegistrationError> {
    let schema: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name = 'record_updates'",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if schema.contains("'initiative'") {
        return Ok(());
    }
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(storage_error)?;
    let migration = (|| {
        let tx = connection.unchecked_transaction().map_err(storage_error)?;
        tx.execute_batch("CREATE TABLE record_updates_workspace (
            id INTEGER PRIMARY KEY NOT NULL, record_id INTEGER NOT NULL REFERENCES records(id),
            created_at_millis INTEGER NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('created', 'body-edited', 'status-transition', 'relationship-added', 'relationship-removed', 'comment-added', 'comment-deleted')),
            from_status TEXT, to_status TEXT, related_record_id INTEGER REFERENCES records(id), comment_id INTEGER,
            relationship_kind TEXT CHECK (relationship_kind IN ('parent-child', 'depends-on', 'generated-by', 'initiative'))
        );
        INSERT INTO record_updates_workspace SELECT id, record_id, created_at_millis, kind, from_status, to_status, related_record_id, comment_id, relationship_kind FROM record_updates;
        DROP TABLE record_updates;
        ALTER TABLE record_updates_workspace RENAME TO record_updates;").map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    })();
    let foreign_keys = connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(storage_error);
    migration.and(foreign_keys)
}

pub(super) fn owner(connection: &Connection, id: u64) -> Result<String, RegistrationError> {
    connection
        .query_row(
            "SELECT workspace_id FROM records WHERE id = ?1",
            [id.cast_signed()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(|| {
            RegistrationError::new("record-not-found", format!("Record {id} does not exist"))
        })
}

pub(super) fn creation_workspace(
    connection: &Connection,
    selected: Option<&str>,
    projects: &[String],
) -> Result<String, RegistrationError> {
    validate_projects(connection, projects)?;
    let scoped_owner = projects
        .first()
        .map(|project| {
            connection
                .query_row(
                    "SELECT workspace_id FROM projects WHERE slug = ?1",
                    [project],
                    |row| row.get::<_, String>(0),
                )
                .map_err(storage_error)
        })
        .transpose()?;
    if let Some(selected) = selected {
        super::workspaces::get(connection, selected)?;
        if scoped_owner
            .as_deref()
            .is_some_and(|owner| owner != selected)
        {
            return Err(cross_workspace());
        }
        return Ok(selected.into());
    }
    scoped_owner.ok_or_else(|| {
        RegistrationError::new(
            "workspace-selection-required",
            "select a Workspace explicitly before opening a Projectless Record",
        )
    })
}

pub(super) fn attach_initiative(
    connection: &Connection,
    member: &forge_app::Record,
    initiative: &forge_app::Record,
) -> Result<(), RegistrationError> {
    if initiative.record_type != forge_app::RecordType::Initiative
        || !matches!(
            member.record_type,
            forge_app::RecordType::Spec | forge_app::RecordType::Wayfinder
        )
    {
        return Err(RegistrationError::new(
            "invalid-relationship",
            "Initiative membership attaches a Spec or Wayfinder to an Initiative",
        ));
    }
    connection
        .execute(
            "INSERT INTO record_relationships (from_id, to_id, kind) VALUES (?1, ?2, 'initiative')",
            rusqlite::params![member.id.cast_signed(), initiative.id.cast_signed()],
        )
        .map_err(|error| {
            if matches!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::ConstraintViolation)
            ) {
                RegistrationError::new(
                    "initiative-already-attached",
                    "Record already belongs to an Initiative; detach it before attaching another",
                )
            } else {
                storage_error(error)
            }
        })?;
    Ok(())
}

pub(super) fn same_workspace(
    connection: &Connection,
    from: u64,
    to: u64,
) -> Result<(), RegistrationError> {
    if owner(connection, from)? != owner(connection, to)? {
        return Err(cross_workspace());
    }
    Ok(())
}

pub(super) fn cross_workspace() -> RegistrationError {
    RegistrationError::new(
        "cross-workspace-record",
        "Record and relationship participants must belong to the selected Workspace",
    )
}

impl SqliteProjectRepository {
    /// Selects a request-local Record boundary while sharing the same durable store.
    ///
    /// # Errors
    /// Returns a validation or storage error when the Workspace does not exist.
    pub fn in_record_workspace(&self, workspace: &str) -> Result<Self, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Record storage lock is poisoned"))?;
        super::workspaces::get(&connection, workspace)?;
        Ok(Self {
            connection: self.connection.clone(),
            record_workspace: Some(workspace.into()),
        })
    }

    /// Returns immutable ownership, enforcing this handle's selection if present.
    ///
    /// # Errors
    /// Returns an error when the Record is missing or belongs to another Workspace.
    pub fn record_workspace(&self, id: u64) -> Result<String, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Record storage lock is poisoned"))?;
        self.ensure_record_workspace(&connection, id)?;
        owner(&connection, id)
    }

    pub(super) fn ensure_record_workspace(
        &self,
        connection: &Connection,
        id: u64,
    ) -> Result<(), RegistrationError> {
        let owner = owner(connection, id)?;
        if self
            .record_workspace
            .as_deref()
            .is_some_and(|selected| selected != owner)
        {
            return Err(cross_workspace());
        }
        Ok(())
    }
}
