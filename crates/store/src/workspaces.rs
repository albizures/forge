use super::{SqliteProjectRepository, storage_error};
use forge_app::{RegistrationError, Workspace, WorkspaceRepository};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::{Path, PathBuf};

pub(super) fn initialize(connection: &Connection) -> Result<(), RegistrationError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS workspaces (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL CHECK(length(trim(name)) > 0),
        root TEXT UNIQUE NOT NULL
    );
    CREATE TRIGGER IF NOT EXISTS workspace_identity_immutable
    BEFORE UPDATE OF id, root ON workspaces
    BEGIN SELECT RAISE(ABORT, 'Workspace ID and root are immutable'); END;",
        )
        .map_err(storage_error)?;
    // Projects and Records require ownership. Assignment ownership is populated by
    // its follow-on slice. RESTRICT also protects retained closed Assignments.
    for table in ["projects", "records", "agent_assignments"] {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(storage_error)?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        if !columns.iter().any(|name| name == "workspace_id") {
            connection.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN workspace_id TEXT REFERENCES workspaces(id) ON DELETE RESTRICT;")).map_err(storage_error)?;
        }
    }
    let has_local_slug = connection
        .prepare("PRAGMA table_info(projects)")
        .map_err(storage_error)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?
        .iter()
        .any(|column| column == "local_slug");
    if !has_local_slug {
        connection
            .execute_batch("ALTER TABLE projects ADD COLUMN local_slug TEXT;")
            .map_err(storage_error)?;
    }
    let unowned: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM projects WHERE workspace_id IS NULL OR local_slug IS NULL)", [], |row| row.get(0)).map_err(storage_error)?;
    if unowned {
        return Err(RegistrationError::new(
            "project-workspace-required",
            "this development store contains Projects without Workspace ownership; select a new store (legacy Project import is unsupported)",
        ));
    }
    connection.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS workspace_project_slugs ON projects(workspace_id, local_slug);
         CREATE TRIGGER IF NOT EXISTS project_identity_immutable_v2
         BEFORE UPDATE OF slug, registered_folder, workspace_id, local_slug ON projects
         BEGIN SELECT RAISE(ABORT, 'Project identity, root and Workspace are immutable'); END;
         CREATE TRIGGER IF NOT EXISTS project_ownership_required
         BEFORE INSERT ON projects
         WHEN NEW.workspace_id IS NULL OR NEW.local_slug IS NULL OR length(trim(NEW.local_slug)) = 0
         BEGIN SELECT RAISE(ABORT, 'Project requires Workspace ownership and a local slug'); END;"
    ).map_err(storage_error)?;
    Ok(())
}

fn row_workspace(row: &rusqlite::Row<'_>) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: row.get(0)?,
        name: row.get(1)?,
        root: PathBuf::from(row.get::<_, String>(2)?),
    })
}
pub(super) fn get(connection: &Connection, id: &str) -> Result<Workspace, RegistrationError> {
    connection
        .query_row(
            "SELECT id, name, root FROM workspaces WHERE id = ?1",
            [id],
            row_workspace,
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(|| {
            RegistrationError::new(
                "workspace-not-found",
                format!("Workspace '{id}' is not registered; use forge workspace list"),
            )
        })
}
fn validate_name(name: &str) -> Result<(), RegistrationError> {
    if name.trim().is_empty() {
        return Err(RegistrationError::new(
            "invalid-workspace-name",
            "Workspace display name must not be blank",
        ));
    }
    Ok(())
}

impl WorkspaceRepository for SqliteProjectRepository {
    fn create_workspace(
        &self,
        id: &str,
        name: &str,
        root: &Path,
    ) -> Result<Workspace, RegistrationError> {
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        {
            return Err(RegistrationError::new(
                "invalid-workspace-id",
                "Workspace ID must contain only ASCII letters, digits, hyphens or underscores",
            ));
        }
        validate_name(name)?;
        let root = root.canonicalize().map_err(|_| {
            RegistrationError::new(
                "invalid-workspace-root",
                "Workspace root must be an existing, resolvable directory",
            )
        })?;
        if !root.is_dir() {
            return Err(RegistrationError::new(
                "invalid-workspace-root",
                "Workspace root must be a directory",
            ));
        }
        let root_text = root.to_str().ok_or_else(|| {
            RegistrationError::new("invalid-workspace-root", "Workspace root must be UTF-8")
        })?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Workspace storage lock is poisoned"))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        {
            let mut statement = transaction
                .prepare("SELECT id, name, root FROM workspaces")
                .map_err(storage_error)?;
            let existing = statement
                .query_map([], row_workspace)
                .map_err(storage_error)?;
            for workspace in existing {
                let workspace = workspace.map_err(storage_error)?;
                if workspace.id == id {
                    return Err(RegistrationError::new(
                        "workspace-id-taken",
                        "Workspace ID is already registered",
                    ));
                }
                if root.starts_with(&workspace.root) || workspace.root.starts_with(&root) {
                    return Err(RegistrationError::new(
                        "workspace-root-overlap",
                        format!("Workspace root overlaps Workspace '{}'", workspace.id),
                    ));
                }
            }
        }
        transaction
            .execute(
                "INSERT INTO workspaces(id, name, root) VALUES (?1, ?2, ?3)",
                params![id, name, root_text],
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(Workspace {
            id: id.into(),
            name: name.into(),
            root,
        })
    }
    fn workspace(&self, id: &str) -> Result<Workspace, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Workspace storage lock is poisoned"))?;
        get(&connection, id)
    }
    fn workspaces(&self) -> Result<Vec<Workspace>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Workspace storage lock is poisoned"))?;
        let mut statement = connection
            .prepare("SELECT id, name, root FROM workspaces ORDER BY id")
            .map_err(storage_error)?;
        statement
            .query_map([], row_workspace)
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)
    }
    fn rename_workspace(&self, id: &str, name: &str) -> Result<Workspace, RegistrationError> {
        validate_name(name)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Workspace storage lock is poisoned"))?;
        let mut workspace = get(&connection, id)?;
        connection
            .execute(
                "UPDATE workspaces SET name = ?2 WHERE id = ?1",
                params![id, name],
            )
            .map_err(storage_error)?;
        workspace.name = name.into();
        Ok(workspace)
    }
    fn remove_workspace(&self, id: &str) -> Result<(), RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Workspace storage lock is poisoned"))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        get(&transaction, id)?;
        for table in ["projects", "records", "agent_assignments"] {
            let count: i64 = transaction
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE workspace_id = ?1"),
                    [id],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            if count != 0 {
                return Err(RegistrationError::new(
                    "workspace-not-empty",
                    format!("Workspace retains {table}; only empty Workspaces can be removed"),
                ));
            }
        }
        let configured: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM workspace_dispatch_configuration WHERE workspace_id = ?1 AND (task_profile_revision IS NOT NULL OR grilling_profile_revision IS NOT NULL OR herdr_workspace_id IS NOT NULL))",
            [id], |row| row.get(0),
        ).map_err(storage_error)?;
        if configured {
            return Err(RegistrationError::new(
                "workspace-not-empty",
                "Workspace retains dispatch configuration; unset its profile selections and Herdr binding before removal",
            ));
        }
        transaction
            .execute("DELETE FROM workspaces WHERE id = ?1", [id])
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)
    }
}
