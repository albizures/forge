//! SQLite implementation of the application persistence boundary.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use forge_app::{
    AgentAssignment, AgentAssignmentClosureReason, AgentAssignmentResource, AgentAssignmentState,
    AgentDispatchRepository, AgentProfile, AssignmentAuthorizationAudit,
    AssignmentAuthorizationOperation, AssignmentAuthorizationOutcome, DispatchWorkspace,
    ManagedWorktreeRepository, NewAgentAssignment, NewAgentAssignmentResource, NewRecord,
    NewTypedRecord, Project, ProjectRepository, Record, RecordComment, RecordCommentId,
    RecordListFilter, RecordRelationship, RecordRelationshipKind, RecordRepository, RecordStatus,
    RecordType, RecordUpdate, RecordUpdateId, RecordUpdateKind, RegistrationError, TaskKind,
    WorktreeClaim, WorktreeClaimState, is_worktree_eligible, managed_worktree_path,
};
use rusqlite::{Connection, ErrorCode, OptionalExtension, params};

mod check_templates;
mod checks;
mod findings;
mod record_workspaces;
mod workspace_dispatch;
mod workspaces;

/// SQLite-backed Project and Record storage.
pub struct SqliteProjectRepository {
    connection: Arc<Mutex<Connection>>,
    record_workspace: Option<String>,
}

impl SqliteProjectRepository {
    /// Opens a database and applies its schema.
    ///
    /// # Errors
    ///
    /// Returns a storage error when SQLite cannot open or initialize the database.
    pub fn open(path: &Path) -> Result<Self, RegistrationError> {
        Self::from_connection(Connection::open(path).map_err(storage_error)?)
    }

    /// Creates an in-memory repository, primarily for composition and tests.
    ///
    /// # Errors
    ///
    /// Returns a storage error when SQLite cannot initialize the database.
    pub fn in_memory() -> Result<Self, RegistrationError> {
        Self::from_connection(Connection::open_in_memory().map_err(storage_error)?)
    }

    #[allow(clippy::too_many_lines)]
    fn from_connection(connection: Connection) -> Result<Self, RegistrationError> {
        // projects.slug is the immutable persistence reference used by the existing
        // foreign keys; workspace_id/local_slug is the operator-facing routing identity.
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS projects (
                slug TEXT PRIMARY KEY NOT NULL,
                registered_folder TEXT UNIQUE NOT NULL,
                origin_url TEXT NOT NULL,
                workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE RESTRICT,
                local_slug TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS records (
                id INTEGER PRIMARY KEY NOT NULL,
                record_type TEXT NOT NULL CHECK (record_type IN ('generic', 'spec', 'task', 'wayfinder', 'grilling', 'initiative')),
                task_kind TEXT CHECK (task_kind IN ('research', 'prototype', 'review')),
                workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE RESTRICT,
                body TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('open', 'in_progress', 'done')),
                completion_resolution TEXT
            );
            CREATE TABLE IF NOT EXISTS record_projects (
                record_id INTEGER NOT NULL REFERENCES records(id),
                project_slug TEXT NOT NULL REFERENCES projects(slug),
                PRIMARY KEY (record_id, project_slug)
            );
            CREATE TABLE IF NOT EXISTS record_relationships (
                from_id INTEGER NOT NULL REFERENCES records(id),
                to_id INTEGER NOT NULL REFERENCES records(id),
                parent_id INTEGER REFERENCES records(id),
                child_id INTEGER REFERENCES records(id) UNIQUE,
                kind TEXT NOT NULL CHECK (kind IN ('parent-child', 'depends-on', 'generated-by', 'initiative')),
                PRIMARY KEY (from_id, to_id, kind)
            );
            CREATE TABLE IF NOT EXISTS record_updates (
                id INTEGER PRIMARY KEY NOT NULL,
                record_id INTEGER NOT NULL REFERENCES records(id),
                created_at_millis INTEGER NOT NULL,
                kind TEXT NOT NULL CHECK (kind IN ('created', 'body-edited', 'status-transition', 'relationship-added', 'relationship-removed', 'comment-added', 'comment-deleted', 'check-assessed', 'check-invalidated', 'finding-activity')),
                assessment_id INTEGER,
                finding_activity_id INTEGER,
                from_status TEXT,
                to_status TEXT,
                related_record_id INTEGER REFERENCES records(id),
                comment_id INTEGER,
                relationship_kind TEXT CHECK (relationship_kind IN ('parent-child', 'depends-on', 'generated-by', 'initiative'))
            );
            CREATE TABLE IF NOT EXISTS record_comments (
                id INTEGER PRIMARY KEY NOT NULL,
                record_id INTEGER NOT NULL REFERENCES records(id),
                created_at_millis INTEGER NOT NULL,
                author TEXT,
                body TEXT NOT NULL
            );
            DROP TABLE IF EXISTS record_update_subscription_targets;
            DROP TABLE IF EXISTS record_update_subscription_filters;
            DROP TABLE IF EXISTS record_update_subscriptions;
            DROP INDEX IF EXISTS record_updates_global_replay;
            CREATE TABLE IF NOT EXISTS agent_profiles (
                record_type TEXT NOT NULL CHECK (record_type IN ('spec', 'task', 'grilling')),
                revision INTEGER NOT NULL CHECK (revision > 0),
                provider TEXT NOT NULL,
                agent_kind TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_template TEXT NOT NULL,
                retry_limit INTEGER NOT NULL,
                retry_backoff_millis INTEGER NOT NULL,
                startup_timeout_millis INTEGER NOT NULL,
                unknown_state_grace_millis INTEGER NOT NULL,
                PRIMARY KEY (record_type, revision)
            );
            CREATE TABLE IF NOT EXISTS agent_assignments (
                id TEXT PRIMARY KEY NOT NULL,
                record_id INTEGER NOT NULL REFERENCES records(id),
                project_slug TEXT NOT NULL REFERENCES projects(slug),
                profile_record_type TEXT NOT NULL,
                profile_revision INTEGER NOT NULL,
                attempt_number INTEGER NOT NULL,
                claimed_at_millis INTEGER NOT NULL,
                state TEXT NOT NULL DEFAULT 'provisioning' CHECK (state IN ('provisioning', 'working', 'idle', 'blocked', 'unknown')),
                state_changed_at_millis INTEGER NOT NULL DEFAULT 0,
                next_retry_at_millis INTEGER NOT NULL DEFAULT 0,
                retry_count INTEGER NOT NULL DEFAULT 0,
                closed_at_millis INTEGER,
                closure_reason TEXT CHECK (closure_reason IN ('lost', 'crashed', 'retired', 'replaced')),
                UNIQUE (record_id, attempt_number)
            );
            CREATE UNIQUE INDEX IF NOT EXISTS one_open_agent_assignment_per_record
                ON agent_assignments(record_id) WHERE closed_at_millis IS NULL;
            CREATE TABLE IF NOT EXISTS agent_assignment_resources (
                assignment_id TEXT NOT NULL REFERENCES agent_assignments(id),
                resource_kind TEXT NOT NULL,
                resource_id TEXT NOT NULL,
                created_at_millis INTEGER NOT NULL,
                retired_at_millis INTEGER,
                PRIMARY KEY (assignment_id, resource_kind, resource_id)
            );
            CREATE TABLE IF NOT EXISTS assignment_execution_bindings (
                assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES agent_assignments(id),
                directory TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS assignment_context_bootstrap (
                assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES agent_assignments(id)
            );
            CREATE TABLE IF NOT EXISTS assignment_unknown_resource_resolutions (
                assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES agent_assignments(id),
                confirmed_at_millis INTEGER NOT NULL,
                authority TEXT NOT NULL,
                confirmation TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS assignment_authorization_audits (
                id INTEGER PRIMARY KEY NOT NULL,
                assignment_id TEXT NOT NULL REFERENCES agent_assignments(id),
                capability_id TEXT NOT NULL,
                created_at_millis INTEGER NOT NULL,
                operation TEXT NOT NULL CHECK (operation IN ('read-context', 'add-comment', 'complete-task', 'start-task', 'acknowledge')),
                target_record_id INTEGER NOT NULL REFERENCES records(id),
                outcome TEXT NOT NULL CHECK (outcome IN ('allowed', 'denied')),
                reason TEXT NOT NULL,
                correlation_id TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS project_managed_worktree_configuration (
                project_slug TEXT PRIMARY KEY NOT NULL REFERENCES projects(slug),
                setup_hook TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS worktree_claims (
                record_id INTEGER PRIMARY KEY NOT NULL REFERENCES records(id),
                project_slug TEXT NOT NULL REFERENCES projects(slug),
                path TEXT NOT NULL UNIQUE,
                state TEXT NOT NULL CHECK (state IN ('reserving', 'active', 'cleanup-required'))
            );
            CREATE INDEX IF NOT EXISTS worktree_claims_requiring_cleanup
                ON worktree_claims(state) WHERE state = 'cleanup-required';
            CREATE TABLE IF NOT EXISTS agent_dispatch_configuration (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                herdr_socket_path TEXT,
                dispatch_workspace_id TEXT,
                dispatch_workspace_root TEXT,
                scan_lease_owner TEXT,
                scan_lease_until_millis INTEGER
            );
            CREATE TABLE IF NOT EXISTS project_agent_dispatch_configuration (
                project_slug TEXT PRIMARY KEY NOT NULL REFERENCES projects(slug),
                task_concurrency_limit INTEGER NOT NULL CHECK (task_concurrency_limit BETWEEN 1 AND 1024)
            );
            CREATE TABLE IF NOT EXISTS dispatch_holds (
                record_id INTEGER PRIMARY KEY NOT NULL REFERENCES records(id),
                reason TEXT NOT NULL CHECK (length(trim(reason)) > 0)
            );
            CREATE TABLE IF NOT EXISTS task_base_commits (
                record_id INTEGER PRIMARY KEY NOT NULL REFERENCES records(id),
                base_commit TEXT NOT NULL CHECK (length(trim(base_commit)) > 0)
            );",
            )
            .map_err(storage_error)?;
        checks::schema(&connection)?;
        findings::schema(&connection)?;
        check_templates::schema(&connection)?;
        migrate_record_status_schema(&connection)?;
        migrate_record_type_schema(&connection)?;
        migrate_task_kind_schema(&connection)?;
        migrate_record_relationship_schema(&connection)?;
        migrate_record_updates_schema(&connection)?;
        migrate_agent_assignment_schema(&connection)?;
        migrate_agent_profile_schema(&connection)?;
        migrate_assignment_authorization_audit_schema(&connection)?;
        migrate_managed_worktree_schema(&connection)?;
        seed_agent_profiles(&connection)?;
        workspaces::initialize(&connection)?;
        record_workspaces::initialize(&connection)?;
        workspace_dispatch::initialize(&connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            record_workspace: None,
        })
    }
}

impl ProjectRepository for SqliteProjectRepository {
    fn project(&self, reference: &str) -> Result<Project, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection
            .query_row(
                "SELECT slug, registered_folder, origin_url, workspace_id, local_slug FROM projects WHERE slug = ?1",
                [reference],
                |row| {
                    Ok(Project {
                        reference: row.get(0)?,
                        project_root: PathBuf::from(row.get::<_, String>(1)?),
                        origin_url: row.get(2)?,
                        workspace_id: row.get(3)?,
                        slug: row.get(4)?,
                    })
                },
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    RegistrationError::new("project-not-found", "Project does not exist")
                }
                error => storage_error(error),
            })
    }

    fn project_in_workspace(
        &self,
        workspace: &str,
        slug: &str,
    ) -> Result<Project, RegistrationError> {
        let reference: Option<String> = {
            let connection = self.connection.lock().map_err(|_| {
                RegistrationError::new("storage", "Project storage lock is poisoned")
            })?;
            connection
                .query_row(
                    "SELECT slug FROM projects WHERE workspace_id = ?1 AND local_slug = ?2",
                    params![workspace, slug],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?
        };
        self.project(&reference.ok_or_else(|| {
            RegistrationError::new(
                "project-not-found",
                "Project does not exist in the selected Workspace",
            )
        })?)
    }

    fn register(&self, project: &Project) -> Result<Project, RegistrationError> {
        let folder = project.project_root.to_string_lossy();
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let connection = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let identity = forge_app::ProjectReference::new(&project.workspace_id, &project.slug)?;
        if project.reference.contains('/') {
            if forge_app::ProjectReference::parse(&project.reference)? != identity {
                return Err(RegistrationError::new(
                    "invalid-project-reference",
                    "qualified Project identity must match its Workspace and local slug",
                ));
            }
        } else {
            // Persistence identities may be opaque, but are also used as path
            // components by existing Worktree consumers. Keep those components safe.
            forge_app::ProjectReference::new(&project.workspace_id, &project.reference)?;
        }
        let workspace = workspaces::get(&connection, &project.workspace_id)?;
        let canonical = forge_app::canonical_project_root(&workspace, &project.project_root)?;
        if canonical != project.project_root {
            return Err(RegistrationError::new(
                "invalid-project-root",
                "persisted Project roots must be canonical",
            ));
        }
        let result = match connection.execute(
            "INSERT INTO projects (slug, registered_folder, origin_url, workspace_id, local_slug) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![project.reference, folder, project.origin_url, project.workspace_id, project.slug],
        ) {
            Ok(_) => Ok(project.clone()),
            Err(error)
                if matches!(
                    error.sqlite_error_code(),
                    Some(ErrorCode::ConstraintViolation)
                ) =>
            {
                let existing = connection.query_row("SELECT slug, registered_folder, origin_url, workspace_id, local_slug FROM projects WHERE slug = ?1 OR registered_folder = ?2 OR (workspace_id = ?3 AND local_slug = ?4)", params![project.reference, folder, project.workspace_id, project.slug], |row| Ok(Project { reference: row.get(0)?, project_root: PathBuf::from(row.get::<_, String>(1)?), origin_url: row.get(2)?, workspace_id: row.get(3)?, slug: row.get(4)? })).map_err(storage_error)?;
                if existing == *project {
                    Ok(existing)
                } else if existing.reference == project.reference || (existing.workspace_id == project.workspace_id && existing.slug == project.slug) {
                    Err(RegistrationError::new(
                        "slug-conflict",
                        format!(
                            "Project slug `{}` is already registered with different values",
                            project.slug
                        ),
                    ))
                } else {
                    Err(RegistrationError::new(
                        "folder-conflict",
                        format!(
                            "Project root `{folder}` is already registered as `{}`",
                            existing.reference
                        ),
                    ))
                }
            }
            Err(error) => Err(storage_error(error)),
        };
        if result.is_ok() {
            connection.commit().map_err(storage_error)?;
        }
        result
    }

    fn find_containing(&self, folder: &Path) -> Result<Project, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let projects = connection
            .prepare("SELECT slug, registered_folder, origin_url, workspace_id, local_slug FROM projects")
            .map_err(storage_error)?
            .query_map([], |row| {
                Ok(Project {
                    reference: row.get(0)?,
                    project_root: PathBuf::from(row.get::<_, String>(1)?),
                    origin_url: row.get(2)?,
                    workspace_id: row.get(3)?,
                    slug: row.get(4)?,
                })
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        projects
            .into_iter()
            .filter(|project| folder.starts_with(&project.project_root))
            .max_by_key(|project| project.project_root.components().count())
            .ok_or_else(|| {
                RegistrationError::new(
                    "project-not-found",
                    format!(
                        "`{}` is not a Project or within a registered Project",
                        folder.display()
                    ),
                )
            })
    }
}

fn workspace_worktree_path(
    connection: &Connection,
    record_id: u64,
) -> Result<PathBuf, RegistrationError> {
    let record = load_record(connection, record_id)?;
    if record.record_type != RecordType::Task || record.project_slugs.len() != 1 {
        return Err(RegistrationError::new(
            "worktree-ineligible-record",
            "Managed Worktrees require a single-Project Task",
        ));
    }
    let (root, slug): (String, String) = connection.query_row(
        "SELECT w.root, p.local_slug FROM projects p JOIN workspaces w ON w.id = p.workspace_id WHERE p.slug = ?1 AND p.workspace_id = ?2",
        params![record.project_slugs[0], record.workspace_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).map_err(storage_error)?;
    Ok(managed_worktree_path(Path::new(&root), &slug, record_id))
}

impl ManagedWorktreeRepository for SqliteProjectRepository {
    fn worktree_path(&self, record_id: u64) -> Result<PathBuf, RegistrationError> {
        self.record_workspace(record_id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        workspace_worktree_path(&connection, record_id)
    }

    fn ensure_worktree_removable(&self, record_id: u64) -> Result<(), RegistrationError> {
        self.record_workspace(record_id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let retained: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_assignments a WHERE a.record_id = ?1 AND (a.closed_at_millis IS NULL OR EXISTS (SELECT 1 FROM agent_assignment_resources r WHERE r.assignment_id = a.id AND r.retired_at_millis IS NULL)))",
            [record_id.cast_signed()], |row| row.get(0),
        ).map_err(storage_error)?;
        if retained {
            return Err(RegistrationError::new(
                "worktree-in-use",
                "Managed Worktree is retained by an open Assignment or unresolved resources; reconcile it before removal",
            ));
        }
        Ok(())
    }

    fn set_project_setup_hook(
        &self,
        project_slug: &str,
        hook: Option<&str>,
    ) -> Result<(), RegistrationError> {
        if hook.is_some_and(|hook| hook.trim().is_empty()) {
            return Err(RegistrationError::new(
                "invalid-setup-hook",
                "Project Setup Hook must not be blank",
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE slug = ?1)",
                [project_slug],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !exists {
            return Err(RegistrationError::new(
                "project-not-found",
                "Project does not exist",
            ));
        }
        if let Some(hook) = hook {
            connection.execute("INSERT INTO project_managed_worktree_configuration (project_slug, setup_hook) VALUES (?1, ?2) ON CONFLICT(project_slug) DO UPDATE SET setup_hook = excluded.setup_hook", params![project_slug, hook]).map_err(storage_error)?;
        } else {
            connection
                .execute(
                    "DELETE FROM project_managed_worktree_configuration WHERE project_slug = ?1",
                    [project_slug],
                )
                .map_err(storage_error)?;
        }
        Ok(())
    }

    fn project_setup_hook(&self, project_slug: &str) -> Result<Option<String>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE slug = ?1)",
                [project_slug],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !exists {
            return Err(RegistrationError::new(
                "project-not-found",
                "Project does not exist",
            ));
        }
        connection.query_row("SELECT setup_hook FROM project_managed_worktree_configuration WHERE project_slug = ?1", [project_slug], |row| row.get(0)).optional().map_err(storage_error)
    }

    fn recover_worktree_claim(
        &self,
        record_id: u64,
        path: &Path,
    ) -> Result<WorktreeClaim, RegistrationError> {
        self.record_workspace(record_id)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let tx = connection.transaction().map_err(storage_error)?;
        let record = load_record(&tx, record_id)?;
        if !is_worktree_eligible(&record) {
            return Err(RegistrationError::new(
                "task-not-open",
                "Recovery requires an Open single-Project Task",
            ));
        }
        let held: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM dispatch_holds WHERE record_id = ?1)",
                [record_id.cast_signed()],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let open: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_assignments WHERE record_id = ?1 AND closed_at_millis IS NULL)", [record_id.cast_signed()], |row| row.get(0)).map_err(storage_error)?;
        if !held || open {
            return Err(RegistrationError::new(
                "recovery-blocked",
                "Recovery requires a dispatch hold and no open Assignment",
            ));
        }
        if load_worktree_claim(&tx, record_id)?.is_some() {
            return Err(RegistrationError::new(
                "worktree-claim-exists",
                "Task already has a Worktree Claim",
            ));
        }
        let slug = &record.project_slugs[0];
        if workspace_worktree_path(&tx, record_id)? != path {
            return Err(RegistrationError::new(
                "recovery-path-changed",
                "Recovery requires the fixed Workspace-rooted Managed Worktree path",
            ));
        }
        tx.execute("INSERT INTO worktree_claims (record_id, project_slug, path, state) VALUES (?1, ?2, ?3, 'active')", params![record_id.cast_signed(), slug, path.to_string_lossy()]).map_err(storage_error)?;
        let claim = load_worktree_claim(&tx, record_id)?.expect("inserted claim");
        tx.commit().map_err(storage_error)?;
        Ok(claim)
    }

    fn reserve_worktree_claim(&self, record_id: u64) -> Result<WorktreeClaim, RegistrationError> {
        self.record_workspace(record_id)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        if let Some(claim) = load_worktree_claim(&transaction, record_id)? {
            return Ok(claim);
        }
        let record = load_record(&transaction, record_id)?;
        if !is_worktree_eligible(&record) {
            return Err(RegistrationError::new(
                "worktree-ineligible-record",
                "a Dispatcher Managed Worktree requires an Open Task scoped to exactly one Project",
            ));
        }
        let path = workspace_worktree_path(&transaction, record_id)?;
        let project_slug = record
            .project_slugs
            .into_iter()
            .next()
            .expect("eligible Record has one Project");
        transaction.execute("INSERT INTO worktree_claims (record_id, project_slug, path, state) VALUES (?1, ?2, ?3, 'reserving')", params![record_id.cast_signed(), project_slug, path.to_string_lossy()]).map_err(|error| if matches!(error.sqlite_error_code(), Some(ErrorCode::ConstraintViolation)) { RegistrationError::new("worktree-claim-exists", "Record already has a Worktree Claim") } else { storage_error(error) })?;
        let claim =
            load_worktree_claim(&transaction, record_id)?.expect("inserted Claim is visible");
        transaction.commit().map_err(storage_error)?;
        Ok(claim)
    }

    fn activate_worktree_claim(&self, record_id: u64) -> Result<WorktreeClaim, RegistrationError> {
        self.record_workspace(record_id)?;
        transition_worktree_claim(&self.connection, record_id, "reserving", "active")
    }

    fn require_worktree_claim_cleanup(
        &self,
        record_id: u64,
    ) -> Result<WorktreeClaim, RegistrationError> {
        self.record_workspace(record_id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let changed = connection.execute("UPDATE worktree_claims SET state = 'cleanup-required' WHERE record_id = ?1 AND state != 'cleanup-required'", [record_id.cast_signed()]).map_err(storage_error)?;
        if changed == 0 && load_worktree_claim(&connection, record_id)?.is_none() {
            return Err(RegistrationError::new(
                "worktree-claim-not-found",
                "Record has no Worktree Claim",
            ));
        }
        load_worktree_claim(&connection, record_id)?.ok_or_else(|| {
            RegistrationError::new("worktree-claim-not-found", "Record has no Worktree Claim")
        })
    }

    fn worktree_claim(&self, record_id: u64) -> Result<Option<WorktreeClaim>, RegistrationError> {
        self.record_workspace(record_id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        load_worktree_claim(&connection, record_id)
    }

    fn worktree_claims(&self) -> Result<Vec<WorktreeClaim>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection.prepare("SELECT c.record_id, c.project_slug, c.path, c.state FROM worktree_claims c JOIN records r ON r.id = c.record_id WHERE (?1 IS NULL OR r.workspace_id = ?1) ORDER BY c.record_id").map_err(storage_error)?.query_map([self.record_workspace.as_deref()], worktree_claim_from_row).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)
    }

    fn worktree_claims_requiring_cleanup(&self) -> Result<Vec<WorktreeClaim>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection.prepare("SELECT c.record_id, c.project_slug, c.path, c.state FROM worktree_claims c JOIN records r ON r.id = c.record_id WHERE c.state = 'cleanup-required' AND (?1 IS NULL OR r.workspace_id = ?1) ORDER BY c.record_id").map_err(storage_error)?.query_map([self.record_workspace.as_deref()], worktree_claim_from_row).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)
    }

    fn delete_worktree_claim(&self, record_id: u64) -> Result<(), RegistrationError> {
        self.ensure_worktree_removable(record_id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        if connection
            .execute(
                "DELETE FROM worktree_claims WHERE record_id = ?1",
                [record_id.cast_signed()],
            )
            .map_err(storage_error)?
            == 0
        {
            return Err(RegistrationError::new(
                "worktree-claim-not-found",
                "Record has no Worktree Claim",
            ));
        }
        Ok(())
    }
}

impl RecordRepository for SqliteProjectRepository {
    fn show_check_template(
        &self,
        project: &str,
        after: Option<u64>,
        limit: u32,
    ) -> Result<forge_app::check_templates::TemplatePage, RegistrationError> {
        self.template_page(project, after, limit)
    }
    fn replace_check_template(
        &self,
        project: &str,
        input: &forge_app::check_templates::TemplateReplacement,
    ) -> Result<forge_app::check_templates::CheckTemplate, RegistrationError> {
        self.replace_template(project, input)
    }
    fn invalidate_task_check(
        &self,
        task: u64,
        check: u64,
        input: &forge_app::checks::InvalidationInput,
    ) -> Result<forge_app::checks::TaskCheck, RegistrationError> {
        self.invalidate_check(task, check, input, None)
    }
    fn raise_task_finding(
        &self,
        task: u64,
        input: &forge_app::findings::FindingRaiseInput,
    ) -> Result<forge_app::findings::Finding, RegistrationError> {
        self.raise_finding(task, input)
    }
    fn resolve_task_finding(
        &self,
        task: u64,
        finding: u64,
        input: &forge_app::findings::FindingResolveInput,
    ) -> Result<forge_app::findings::Finding, RegistrationError> {
        self.resolve_finding(task, finding, input)
    }
    fn list_task_findings(
        &self,
        task: u64,
        after: Option<u64>,
        limit: u32,
    ) -> Result<forge_app::findings::FindingList, RegistrationError> {
        self.list_findings(task, after, limit)
    }
    fn show_task_finding(
        &self,
        task: u64,
        finding: u64,
        after: Option<u64>,
        limit: u32,
    ) -> Result<forge_app::findings::FindingPage, RegistrationError> {
        self.show_finding(task, finding, after, limit)
    }
    fn list_task_checks(
        &self,
        task: u64,
    ) -> Result<Vec<forge_app::checks::TaskCheck>, RegistrationError> {
        self.list_checks(task)
    }
    fn show_task_check(
        &self,
        task: u64,
        check: u64,
        after: Option<u64>,
        limit: u32,
    ) -> Result<forge_app::checks::CheckPage, RegistrationError> {
        self.show_check(task, check, after, limit)
    }
    fn assess_task_check(
        &self,
        task: u64,
        check: u64,
        input: &forge_app::checks::AssessmentInput,
    ) -> Result<forge_app::checks::TaskCheck, RegistrationError> {
        self.assess_check(task, check, input, None)
    }
    fn assess_assignment_check(
        &self,
        task: u64,
        check: u64,
        input: &forge_app::checks::AssessmentInput,
        assignment: &str,
    ) -> Result<forge_app::checks::TaskCheck, RegistrationError> {
        self.assess_check(task, check, input, Some(assignment))
    }
    fn invalidate_assignment_check(
        &self,
        task: u64,
        check: u64,
        input: &forge_app::checks::InvalidationInput,
        assignment: &str,
    ) -> Result<forge_app::checks::TaskCheck, RegistrationError> {
        self.invalidate_check(task, check, input, Some(assignment))
    }
    fn records_in_workspace(
        &self,
        workspace: &str,
    ) -> Result<Box<dyn RecordRepository>, RegistrationError> {
        Ok(Box::new(self.in_record_workspace(workspace)?))
    }

    fn set_task_base_commit(&self, record_id: u64, commit: &str) -> Result<(), RegistrationError> {
        self.record_workspace(record_id)?;
        if commit.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-base-commit",
                "Task Base Commit must not be blank",
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let record = load_record(&connection, record_id)?;
        if record.record_type != RecordType::Task || record.project_slugs.len() != 1 {
            return Err(RegistrationError::new(
                "invalid-base-commit",
                "only a single-Project Task has a Task Base Commit",
            ));
        }
        connection
            .execute(
                "INSERT INTO task_base_commits (record_id, base_commit) VALUES (?1, ?2)",
                params![record_id.cast_signed(), commit],
            )
            .map_err(|error| {
                if matches!(
                    error.sqlite_error_code(),
                    Some(ErrorCode::ConstraintViolation)
                ) {
                    RegistrationError::new("base-commit-exists", "Task Base Commit is immutable")
                } else {
                    storage_error(error)
                }
            })?;
        Ok(())
    }

    fn task_base_commit(&self, record_id: u64) -> Result<Option<String>, RegistrationError> {
        self.record_workspace(record_id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        load_record(&connection, record_id)?;
        connection
            .query_row(
                "SELECT base_commit FROM task_base_commits WHERE record_id = ?1",
                [record_id.cast_signed()],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)
    }

    fn open_record(&self, record: &NewRecord) -> Result<Record, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let workspace = record_workspaces::creation_workspace(
            &transaction,
            self.record_workspace.as_deref(),
            &record.project_slugs,
        )?;
        transaction
            .execute(
                "INSERT INTO records (record_type, body, status, workspace_id) VALUES ('generic', ?1, 'open', ?2)",
                params![record.body, workspace],
            )
            .map_err(storage_error)?;
        let sqlite_id = transaction.last_insert_rowid();
        let id = sqlite_id.cast_unsigned();
        for slug in &record.project_slugs {
            transaction
                .execute(
                    "INSERT INTO record_projects (record_id, project_slug) VALUES (?1, ?2)",
                    params![sqlite_id, slug],
                )
                .map_err(storage_error)?;
        }
        insert_update(&transaction, id, RecordUpdateKind::Created)?;
        add_generated_by_relationships(&transaction, id, &record.generated_by_ids)?;
        let opened = load_record(&transaction, id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(opened)
    }

    fn open_typed_record(&self, record: &NewTypedRecord) -> Result<Record, RegistrationError> {
        validate_typed_record(record)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let workspace = record_workspaces::creation_workspace(
            &transaction,
            self.record_workspace.as_deref(),
            &record.project_slugs,
        )?;
        transaction
            .execute(
                "INSERT INTO records (record_type, task_kind, body, status, workspace_id) VALUES (?1, ?2, ?3, 'open', ?4)",
                params![record_type_name(record.record_type), task_kind_name(record.task_kind), record.body, workspace],
            )
            .map_err(storage_error)?;
        let id = transaction.last_insert_rowid().cast_unsigned();
        if record.record_type == RecordType::Task {
            checks::provision(&transaction, id)?;
            check_templates::provision(
                &transaction,
                id,
                record.project_slugs.first().map(String::as_str),
            )?;
        }
        for slug in &record.project_slugs {
            transaction
                .execute(
                    "INSERT INTO record_projects (record_id, project_slug) VALUES (?1, ?2)",
                    params![id.cast_signed(), slug],
                )
                .map_err(storage_error)?;
        }
        insert_update(&transaction, id, RecordUpdateKind::Created)?;
        add_generated_by_relationships(&transaction, id, &record.generated_by_ids)?;
        let opened = load_record(&transaction, id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(opened)
    }

    fn open_task_under_spec(
        &self,
        body: &str,
        spec_id: u64,
        task_kind: Option<TaskKind>,
        generated_by_ids: Vec<u64>,
    ) -> Result<Record, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        self.ensure_record_workspace(&transaction, spec_id)?;
        let workspace = record_workspaces::owner(&transaction, spec_id)?;
        let spec = load_record(&transaction, spec_id)?;
        if spec.record_type != RecordType::Spec {
            return Err(RegistrationError::new(
                "invalid-parent-type",
                "a Task may be opened under a Spec only",
            ));
        }
        transaction
            .execute(
                "INSERT INTO records (record_type, task_kind, body, status, workspace_id) VALUES ('task', ?1, ?2, 'open', ?3)",
                params![task_kind_name(task_kind), body, workspace],
            )
            .map_err(storage_error)?;
        let task_id = transaction.last_insert_rowid().cast_unsigned();
        checks::provision(&transaction, task_id)?;
        check_templates::provision(
            &transaction,
            task_id,
            spec.project_slugs.first().map(String::as_str),
        )?;
        transaction
            .execute(
                "INSERT INTO record_projects (record_id, project_slug) SELECT ?1, project_slug FROM record_projects WHERE record_id = ?2",
                params![task_id.cast_signed(), spec_id.cast_signed()],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "INSERT INTO record_relationships (from_id, to_id, parent_id, child_id, kind) VALUES (?1, ?2, ?2, ?1, 'parent-child')",
                params![task_id.cast_signed(), spec_id.cast_signed()],
            )
            .map_err(storage_error)?;
        insert_update(&transaction, task_id, RecordUpdateKind::Created)?;
        insert_update(
            &transaction,
            task_id,
            RecordUpdateKind::RelationshipAdded {
                related_record_id: spec_id,
                relationship_kind: Some(RecordRelationshipKind::ParentChild),
            },
        )?;
        insert_update(
            &transaction,
            spec_id,
            RecordUpdateKind::RelationshipAdded {
                related_record_id: task_id,
                relationship_kind: Some(RecordRelationshipKind::ParentChild),
            },
        )?;
        add_generated_by_relationships(&transaction, task_id, &generated_by_ids)?;
        let task = load_record(&transaction, task_id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(task)
    }

    fn open_task_under_wayfinder(
        &self,
        body: &str,
        wayfinder_id: u64,
        project_slug: Option<String>,
        task_kind: Option<TaskKind>,
        generated_by_ids: Vec<u64>,
    ) -> Result<Record, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        self.ensure_record_workspace(&transaction, wayfinder_id)?;
        let workspace = record_workspaces::owner(&transaction, wayfinder_id)?;
        let wayfinder = load_record(&transaction, wayfinder_id)?;
        if wayfinder.record_type != RecordType::Wayfinder {
            return Err(RegistrationError::new(
                "invalid-parent-type",
                "a Task may be opened under a Wayfinder only",
            ));
        }
        let project_slugs = match (project_slug, wayfinder.project_slugs.as_slice()) {
            (None, []) => vec![],
            (Some(project_slug), scope) if scope.contains(&project_slug) => vec![project_slug],
            (Some(_), _) => {
                return Err(RegistrationError::new(
                    "scope-mismatch",
                    "Task Project must belong to the Wayfinder scope",
                ));
            }
            (None, [project_slug]) => vec![project_slug.clone()],
            (None, _) => {
                return Err(RegistrationError::new(
                    "missing-project",
                    "a multi-Project Wayfinder Task requires a Project",
                ));
            }
        };
        transaction
            .execute(
                "INSERT INTO records (record_type, task_kind, body, status, workspace_id) VALUES ('task', ?1, ?2, 'open', ?3)",
                params![task_kind_name(task_kind), body, workspace],
            )
            .map_err(storage_error)?;
        let task_id = transaction.last_insert_rowid().cast_unsigned();
        checks::provision(&transaction, task_id)?;
        check_templates::provision(
            &transaction,
            task_id,
            project_slugs.first().map(String::as_str),
        )?;
        for project_slug in project_slugs {
            transaction
                .execute(
                    "INSERT INTO record_projects (record_id, project_slug) VALUES (?1, ?2)",
                    params![task_id.cast_signed(), project_slug],
                )
                .map_err(storage_error)?;
        }
        transaction
            .execute(
                "INSERT INTO record_relationships (from_id, to_id, parent_id, child_id, kind) VALUES (?1, ?2, ?2, ?1, 'parent-child')",
                params![task_id.cast_signed(), wayfinder_id.cast_signed()],
            )
            .map_err(storage_error)?;
        insert_update(&transaction, task_id, RecordUpdateKind::Created)?;
        insert_update(
            &transaction,
            task_id,
            RecordUpdateKind::RelationshipAdded {
                related_record_id: wayfinder_id,
                relationship_kind: Some(RecordRelationshipKind::ParentChild),
            },
        )?;
        insert_update(
            &transaction,
            wayfinder_id,
            RecordUpdateKind::RelationshipAdded {
                related_record_id: task_id,
                relationship_kind: Some(RecordRelationshipKind::ParentChild),
            },
        )?;
        add_generated_by_relationships(&transaction, task_id, &generated_by_ids)?;
        let task = load_record(&transaction, task_id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(task)
    }

    fn open_grilling_under_wayfinder(
        &self,
        body: &str,
        wayfinder_id: u64,
        project_slugs: Option<Vec<String>>,
        generated_by_ids: Vec<u64>,
    ) -> Result<Record, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        self.ensure_record_workspace(&transaction, wayfinder_id)?;
        let workspace = record_workspaces::owner(&transaction, wayfinder_id)?;
        let wayfinder = load_record(&transaction, wayfinder_id)?;
        if wayfinder.record_type != RecordType::Wayfinder {
            return Err(RegistrationError::new(
                "invalid-parent-type",
                "a Grilling may be opened under a Wayfinder only",
            ));
        }
        let mut project_slugs = project_slugs.unwrap_or(wayfinder.project_slugs.clone());
        project_slugs.sort();
        if project_slugs.is_empty() && !wayfinder.project_slugs.is_empty() {
            return Err(RegistrationError::new(
                "empty-project-scope",
                "a Project-scoped Wayfinder's Grilling must select at least one Project",
            ));
        }
        if project_slugs.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(RegistrationError::new(
                "duplicate-project",
                "Grilling Project Scope must not contain duplicate Project slugs",
            ));
        }
        if project_slugs
            .iter()
            .any(|slug| !wayfinder.project_slugs.contains(slug))
        {
            return Err(RegistrationError::new(
                "scope-mismatch",
                "Grilling Project Scope must be a subset of the Wayfinder scope",
            ));
        }
        transaction
            .execute(
                "INSERT INTO records (record_type, body, status, workspace_id) VALUES ('grilling', ?1, 'open', ?2)",
                params![body, workspace],
            )
            .map_err(storage_error)?;
        let grilling_id = transaction.last_insert_rowid().cast_unsigned();
        for project_slug in &project_slugs {
            transaction
                .execute(
                    "INSERT INTO record_projects (record_id, project_slug) VALUES (?1, ?2)",
                    params![grilling_id.cast_signed(), project_slug],
                )
                .map_err(storage_error)?;
        }
        transaction
            .execute(
                "INSERT INTO record_relationships (from_id, to_id, parent_id, child_id, kind) VALUES (?1, ?2, ?2, ?1, 'parent-child')",
                params![grilling_id.cast_signed(), wayfinder_id.cast_signed()],
            )
            .map_err(storage_error)?;
        insert_update(&transaction, grilling_id, RecordUpdateKind::Created)?;
        insert_update(
            &transaction,
            grilling_id,
            RecordUpdateKind::RelationshipAdded {
                related_record_id: wayfinder_id,
                relationship_kind: Some(RecordRelationshipKind::ParentChild),
            },
        )?;
        insert_update(
            &transaction,
            wayfinder_id,
            RecordUpdateKind::RelationshipAdded {
                related_record_id: grilling_id,
                relationship_kind: Some(RecordRelationshipKind::ParentChild),
            },
        )?;
        add_generated_by_relationships(&transaction, grilling_id, &generated_by_ids)?;
        let grilling = load_record(&transaction, grilling_id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(grilling)
    }

    fn list_records(&self, filter: &RecordListFilter) -> Result<Vec<Record>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        if let Some(project) = &filter.project_slug {
            record_workspaces::creation_workspace(
                &connection,
                self.record_workspace.as_deref(),
                std::slice::from_ref(project),
            )?;
        }
        let ids = connection.prepare(
            "SELECT record.id FROM records record
             WHERE (?1 IS NULL OR record.workspace_id = ?1)
               AND (?2 IS NULL OR EXISTS(SELECT 1 FROM record_projects scope WHERE scope.record_id = record.id AND scope.project_slug = ?2))
               AND (?3 IS NULL OR record.status = ?3)
             ORDER BY record.id DESC"
        ).map_err(storage_error)?.query_map(params![self.record_workspace, filter.project_slug, filter.status.map(status_name)], |row| row.get::<_, i64>(0))
            .map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
        let records = ids
            .into_iter()
            .map(|id| load_record(&connection, id.cast_unsigned()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records
            .into_iter()
            .filter(|record| {
                filter
                    .record_type
                    .is_none_or(|record_type| record.record_type == record_type)
                    && filter
                        .task_kind
                        .is_none_or(|task_kind| record.task_kind == Some(task_kind))
            })
            .collect())
    }

    fn show_record(&self, id: u64) -> Result<Record, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        self.ensure_record_workspace(&connection, id)?;
        load_record(&connection, id)
    }

    fn edit_record(&self, id: u64, body: &str) -> Result<Record, RegistrationError> {
        self.record_workspace(id)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let updated = transaction
            .execute(
                "UPDATE records SET body = ?1 WHERE id = ?2 AND status != 'done'",
                params![body, id.cast_signed()],
            )
            .map_err(storage_error)?;
        if updated == 0 {
            return record_or_transition_error(&transaction, id);
        }
        insert_update(&transaction, id, RecordUpdateKind::BodyEdited)?;
        let record = load_record(&transaction, id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(record)
    }

    fn start_record(&self, id: u64) -> Result<Record, RegistrationError> {
        self.record_workspace(id)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        // Reserve the writer before reading prerequisites. A deferred transaction's
        // read-to-write upgrade can fail immediately (bypassing the busy timeout)
        // when another connection, e.g. resume/reconciliation, holds the reservation.
        // BEGIN IMMEDIATE instead waits within SQLite's bounded busy timeout before
        // taking a read snapshot; no lifecycle operation needs to be replayed.
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let unfinished = unfinished_prerequisite_ids(&transaction, id)?;
        if !unfinished.is_empty() {
            return Err(dependencies_not_done_error(&unfinished));
        }
        let updated = transaction
            .execute(
                "UPDATE records SET status = 'in_progress' WHERE id = ?1 AND status = 'open'",
                [id.cast_signed()],
            )
            .map_err(storage_error)?;
        if updated == 0 {
            return record_or_transition_error(&transaction, id);
        }
        insert_update(
            &transaction,
            id,
            RecordUpdateKind::StatusTransition {
                from: RecordStatus::Open,
                to: RecordStatus::InProgress,
            },
        )?;
        let record = load_record(&transaction, id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(record)
    }

    fn reopen_record(&self, id: u64) -> Result<Record, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let updated = transaction
            .execute(
                "UPDATE records SET status = 'open' WHERE id = ?1 AND record_type = 'task' AND status = 'in_progress'",
                [id.cast_signed()],
            )
            .map_err(storage_error)?;
        if updated == 0 {
            return record_or_transition_error(&transaction, id);
        }
        insert_update(
            &transaction,
            id,
            RecordUpdateKind::StatusTransition {
                from: RecordStatus::InProgress,
                to: RecordStatus::Open,
            },
        )?;
        let record = load_record(&transaction, id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(record)
    }

    fn done_record(
        &self,
        id: u64,
        completion_resolution: Option<&str>,
    ) -> Result<Record, RegistrationError> {
        self.record_workspace(id)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let incomplete_child: Option<i64> = transaction.query_row(
            "SELECT child.id FROM record_relationships relationship JOIN records child ON child.id = relationship.child_id WHERE relationship.parent_id = ?1 AND child.status != 'done' LIMIT 1",
            [id.cast_signed()], |row| row.get(0),
        ).optional().map_err(storage_error)?;
        if incomplete_child.is_some() {
            return Err(RegistrationError::new(
                "children-not-done",
                "all child Records must be Done before completing a parent",
            ));
        }
        let current = load_record(&transaction, id)?;
        if current.status != RecordStatus::InProgress {
            return record_or_transition_error(&transaction, id);
        }
        if current.record_type == RecordType::Task {
            checks::gate(&transaction, id)?;
        }
        let updated = transaction.execute("UPDATE records SET status = 'done', completion_resolution = ?1 WHERE id = ?2 AND status = 'in_progress'", params![completion_resolution, id.cast_signed()]).map_err(storage_error)?;
        if updated == 0 {
            return record_or_transition_error(&transaction, id);
        }
        insert_update(
            &transaction,
            id,
            RecordUpdateKind::StatusTransition {
                from: RecordStatus::InProgress,
                to: RecordStatus::Done,
            },
        )?;
        let record = load_record(&transaction, id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(record)
    }

    fn relate_records(
        &self,
        child_id: u64,
        parent_id: u64,
        kind: RecordRelationshipKind,
    ) -> Result<Record, RegistrationError> {
        if child_id == parent_id {
            return Err(RegistrationError::new(
                "invalid-relationship",
                "a Record cannot relate to itself",
            ));
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let child = load_record(&transaction, child_id)?;
        let parent = load_record(&transaction, parent_id)?;
        self.ensure_record_workspace(&transaction, child_id)?;
        self.ensure_record_workspace(&transaction, parent_id)?;
        record_workspaces::same_workspace(&transaction, child_id, parent_id)?;
        insert_relationship(&transaction, &child, &parent, kind)?;
        insert_update(
            &transaction,
            child_id,
            RecordUpdateKind::RelationshipAdded {
                related_record_id: parent_id,
                relationship_kind: Some(kind),
            },
        )?;
        insert_update(
            &transaction,
            parent_id,
            RecordUpdateKind::RelationshipAdded {
                related_record_id: child_id,
                relationship_kind: Some(kind),
            },
        )?;
        let record = load_record(&transaction, child_id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(record)
    }

    fn unrelate_records(
        &self,
        child_id: u64,
        parent_id: u64,
        kind: RecordRelationshipKind,
    ) -> Result<Record, RegistrationError> {
        self.record_workspace(child_id)?;
        self.record_workspace(parent_id)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let child = load_record(&transaction, child_id)?;
        let parent = load_record(&transaction, parent_id)?;
        match kind {
            RecordRelationshipKind::ParentChild
                if child.record_type != RecordType::Generic
                    || parent.record_type != RecordType::Generic =>
            {
                return Err(RegistrationError::new(
                    "invalid-relationship",
                    "only Generic Record Relationships may be removed",
                ));
            }
            RecordRelationshipKind::DependsOn if child.status != RecordStatus::Open => {
                return Err(RegistrationError::new(
                    "blocked-record-not-open",
                    "a dependency may change only while its blocked Record is Open",
                ));
            }
            RecordRelationshipKind::GeneratedBy if child.status != RecordStatus::Open => {
                return Err(RegistrationError::new(
                    "generated-record-not-open",
                    "provenance may change only while its generated Record is Open",
                ));
            }
            RecordRelationshipKind::DependsOn
                if !(child.record_type == parent.record_type
                    && matches!(child.record_type, RecordType::Task | RecordType::Spec)
                    || child.record_type == RecordType::Task
                        && parent.record_type == RecordType::Grilling
                        && (child.project_slugs.is_empty() && parent.project_slugs.is_empty()
                            || child.project_slugs.len() == 1
                                && parent.project_slugs.contains(&child.project_slugs[0]))) =>
            {
                return Err(RegistrationError::new(
                    "invalid-relationship",
                    "depends-on requires same-type Task or Spec Records, or a Task scoped within a Grilling",
                ));
            }
            _ => {}
        }
        let kind_name = match kind {
            RecordRelationshipKind::ParentChild => "parent-child",
            RecordRelationshipKind::DependsOn => "depends-on",
            RecordRelationshipKind::GeneratedBy => "generated-by",
            RecordRelationshipKind::Initiative => "initiative",
        };
        let deleted = transaction
            .execute(
                "DELETE FROM record_relationships WHERE from_id = ?1 AND to_id = ?2 AND kind = ?3",
                params![child_id.cast_signed(), parent_id.cast_signed(), kind_name],
            )
            .map_err(storage_error)?;
        if deleted == 0 {
            return Err(RegistrationError::new(
                "relationship-not-found",
                "Relationship does not exist",
            ));
        }
        insert_update(
            &transaction,
            child_id,
            RecordUpdateKind::RelationshipRemoved {
                related_record_id: parent_id,
                relationship_kind: Some(kind),
            },
        )?;
        insert_update(
            &transaction,
            parent_id,
            RecordUpdateKind::RelationshipRemoved {
                related_record_id: child_id,
                relationship_kind: Some(kind),
            },
        )?;
        let record = load_record(&transaction, child_id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(record)
    }

    fn add_record_comment(
        &self,
        id: u64,
        body: &str,
        author: Option<&str>,
    ) -> Result<RecordComment, RegistrationError> {
        self.record_workspace(id)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        load_record(&transaction, id)?;
        let created_at_millis = now_millis()?;
        transaction.execute("INSERT INTO record_comments (record_id, created_at_millis, author, body) VALUES (?1, ?2, ?3, ?4)", params![id.cast_signed(), created_at_millis, author, body]).map_err(storage_error)?;
        let comment = RecordComment {
            id: RecordCommentId(transaction.last_insert_rowid().cast_unsigned()),
            created_at_millis: created_at_millis.cast_unsigned(),
            author: author.map(str::to_owned),
            body: body.to_owned(),
        };
        insert_update(
            &transaction,
            id,
            RecordUpdateKind::CommentAdded {
                comment_id: comment.id,
            },
        )?;
        transaction.commit().map_err(storage_error)?;
        Ok(comment)
    }

    fn list_record_comments(&self, id: u64) -> Result<Vec<RecordComment>, RegistrationError> {
        self.record_workspace(id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        load_record(&connection, id)?;
        connection.prepare("SELECT id, created_at_millis, author, body FROM record_comments WHERE record_id = ?1 ORDER BY created_at_millis DESC, id DESC").map_err(storage_error)?.query_map([id.cast_signed()], |row| Ok(RecordComment { id: RecordCommentId(row.get::<_, i64>(0)?.cast_unsigned()), created_at_millis: row.get::<_, i64>(1)?.cast_unsigned(), author: row.get(2)?, body: row.get(3)? })).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)
    }

    fn delete_record_comment(
        &self,
        id: u64,
        comment_id: RecordCommentId,
    ) -> Result<(), RegistrationError> {
        self.record_workspace(id)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let deleted = transaction
            .execute(
                "DELETE FROM record_comments WHERE id = ?1 AND record_id = ?2",
                params![comment_id.0.cast_signed(), id.cast_signed()],
            )
            .map_err(storage_error)?;
        if deleted == 0 {
            return Err(RegistrationError::new(
                "comment-not-found",
                "Comment does not exist",
            ));
        }
        insert_update(
            &transaction,
            id,
            RecordUpdateKind::CommentDeleted { comment_id },
        )?;
        transaction.commit().map_err(storage_error)
    }

    fn record_comment_head(&self, id: u64) -> Result<u64, RegistrationError> {
        self.record_workspace(id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "store lock poisoned"))?;
        connection
            .query_row(
                "SELECT COALESCE(MAX(id), 0) FROM record_comments WHERE record_id = ?1",
                [id.cast_signed()],
                |row| Ok(row.get::<_, i64>(0)?.cast_unsigned()),
            )
            .map_err(storage_error)
    }

    fn record_comment_page(
        &self,
        id: u64,
        after: u64,
        through: u64,
        limit: u32,
    ) -> Result<Vec<RecordComment>, RegistrationError> {
        self.record_workspace(id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "store lock poisoned"))?;
        connection.prepare("SELECT id, created_at_millis, author, body FROM record_comments WHERE record_id = ?1 AND id > ?2 AND id <= ?3 ORDER BY id LIMIT ?4").map_err(storage_error)?.query_map(params![id.cast_signed(), after.cast_signed(), through.cast_signed(), limit], |row| Ok(RecordComment { id: RecordCommentId(row.get::<_, i64>(0)?.cast_unsigned()), created_at_millis: row.get::<_, i64>(1)?.cast_unsigned(), author: row.get(2)?, body: row.get(3)? })).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)
    }

    fn record_update_page(
        &self,
        id: u64,
        after: u64,
        through: u64,
        limit: u32,
    ) -> Result<Vec<RecordUpdate>, RegistrationError> {
        self.record_workspace(id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "store lock poisoned"))?;
        connection.prepare("SELECT id, created_at_millis, kind, from_status, to_status, related_record_id, comment_id, relationship_kind, assessment_id, finding_activity_id FROM record_updates WHERE record_id = ?1 AND id > ?2 AND id <= ?3 ORDER BY id LIMIT ?4").map_err(storage_error)?.query_map(params![id.cast_signed(), after.cast_signed(), through.cast_signed(), limit], update_from_row).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)
    }

    fn list_record_updates(&self, id: u64) -> Result<Vec<RecordUpdate>, RegistrationError> {
        self.record_workspace(id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        load_record(&connection, id)?;
        connection.prepare("SELECT id, created_at_millis, kind, from_status, to_status, related_record_id, comment_id, relationship_kind, assessment_id, finding_activity_id FROM record_updates WHERE record_id = ?1 ORDER BY created_at_millis DESC, id DESC")
            .map_err(storage_error)?
            .query_map([id.cast_signed()], update_from_row)
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)
    }
}

fn worktree_claim_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorktreeClaim> {
    let state: String = row.get(3)?;
    let state = match state.as_str() {
        "reserving" => WorktreeClaimState::Reserving,
        "active" => WorktreeClaimState::Active,
        "cleanup-required" => WorktreeClaimState::CleanupRequired,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(WorktreeClaim {
        record_id: row.get::<_, i64>(0)?.cast_unsigned(),
        project_slug: row.get(1)?,
        path: PathBuf::from(row.get::<_, String>(2)?),
        state,
    })
}

fn load_worktree_claim(
    connection: &Connection,
    record_id: u64,
) -> Result<Option<WorktreeClaim>, RegistrationError> {
    connection
        .query_row(
            "SELECT record_id, project_slug, path, state FROM worktree_claims WHERE record_id = ?1",
            [record_id.cast_signed()],
            worktree_claim_from_row,
        )
        .optional()
        .map_err(storage_error)
}

fn transition_worktree_claim(
    connection: &Mutex<Connection>,
    record_id: u64,
    expected: &str,
    next: &str,
) -> Result<WorktreeClaim, RegistrationError> {
    let connection = connection
        .lock()
        .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
    let changed = connection
        .execute(
            "UPDATE worktree_claims SET state = ?1 WHERE record_id = ?2 AND state = ?3",
            params![next, record_id.cast_signed(), expected],
        )
        .map_err(storage_error)?;
    if changed == 0 {
        let claim = load_worktree_claim(&connection, record_id)?;
        return match claim {
            None => Err(RegistrationError::new(
                "worktree-claim-not-found",
                "Record has no Worktree Claim",
            )),
            Some(_) => Err(RegistrationError::new(
                "invalid-worktree-claim-transition",
                "Worktree Claim is not in the required state",
            )),
        };
    }
    load_worktree_claim(&connection, record_id)?.ok_or_else(|| {
        RegistrationError::new("worktree-claim-not-found", "Record has no Worktree Claim")
    })
}

fn migrate_managed_worktree_schema(connection: &Connection) -> Result<(), RegistrationError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS project_managed_worktree_configuration (
            project_slug TEXT PRIMARY KEY NOT NULL REFERENCES projects(slug), setup_hook TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS worktree_claims (
            record_id INTEGER PRIMARY KEY NOT NULL REFERENCES records(id),
            project_slug TEXT NOT NULL REFERENCES projects(slug), path TEXT NOT NULL UNIQUE,
            state TEXT NOT NULL CHECK (state IN ('reserving', 'active', 'cleanup-required'))
        );
        CREATE INDEX IF NOT EXISTS worktree_claims_requiring_cleanup ON worktree_claims(state) WHERE state = 'cleanup-required';",
    ).map_err(storage_error)
}

fn migrate_agent_assignment_schema(connection: &Connection) -> Result<(), RegistrationError> {
    let columns = connection
        .prepare("PRAGMA table_info(agent_assignments)")
        .map_err(storage_error)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    for column in [
        "launch_started_at_millis",
        "acknowledgement_deadline_millis",
    ] {
        if !columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!("ALTER TABLE agent_assignments ADD COLUMN {column} INTEGER"),
                    [],
                )
                .map_err(storage_error)?;
        }
    }
    if !columns
        .iter()
        .any(|column| column == "safe_dispatch_generation")
    {
        connection.execute("ALTER TABLE agent_assignments ADD COLUMN safe_dispatch_generation INTEGER NOT NULL DEFAULT 0", []).map_err(storage_error)?;
    }
    if !columns
        .iter()
        .any(|column| column == "acknowledged_at_millis")
    {
        connection
            .execute(
                "ALTER TABLE agent_assignments ADD COLUMN acknowledged_at_millis INTEGER",
                [],
            )
            .map_err(storage_error)?;
    }
    if !columns.iter().any(|column| column == "state") {
        connection.execute("ALTER TABLE agent_assignments ADD COLUMN state TEXT NOT NULL DEFAULT 'provisioning'", []).map_err(storage_error)?;
    }
    if !columns.iter().any(|column| column == "closure_reason") {
        connection
            .execute(
                "ALTER TABLE agent_assignments ADD COLUMN closure_reason TEXT",
                [],
            )
            .map_err(storage_error)?;
    }
    if !columns
        .iter()
        .any(|column| column == "state_changed_at_millis")
    {
        connection.execute("ALTER TABLE agent_assignments ADD COLUMN state_changed_at_millis INTEGER NOT NULL DEFAULT 0", []).map_err(storage_error)?;
    }
    if !columns
        .iter()
        .any(|column| column == "next_retry_at_millis")
    {
        connection.execute("ALTER TABLE agent_assignments ADD COLUMN next_retry_at_millis INTEGER NOT NULL DEFAULT 0", []).map_err(storage_error)?;
    }
    if !columns.iter().any(|column| column == "retry_count") {
        connection
            .execute(
                "ALTER TABLE agent_assignments ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(storage_error)?;
    }
    for column in [
        "scan_lease_owner TEXT",
        "scan_lease_until_millis INTEGER",
        "dispatch_workspace_id TEXT",
        "dispatch_workspace_root TEXT",
    ] {
        connection
            .execute_batch(&format!(
                "ALTER TABLE agent_dispatch_configuration ADD COLUMN {column};"
            ))
            .or_else(|error| {
                if error.to_string().contains("duplicate column name") {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .map_err(storage_error)?;
    }
    connection.execute_batch("CREATE TABLE IF NOT EXISTS dispatch_holds (record_id INTEGER PRIMARY KEY NOT NULL REFERENCES records(id), reason TEXT NOT NULL CHECK (length(trim(reason)) > 0)); CREATE TABLE IF NOT EXISTS task_base_commits (record_id INTEGER PRIMARY KEY NOT NULL REFERENCES records(id), base_commit TEXT NOT NULL CHECK (length(trim(base_commit)) > 0));").map_err(storage_error)?;
    Ok(())
}

fn migrate_assignment_authorization_audit_schema(
    connection: &Connection,
) -> Result<(), RegistrationError> {
    let schema: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'assignment_authorization_audits'",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if schema.contains("acknowledge") && !schema.contains("subscription") {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction().map_err(storage_error)?;
    transaction.execute_batch(
        "ALTER TABLE assignment_authorization_audits RENAME TO assignment_authorization_audits_old;
        CREATE TABLE assignment_authorization_audits (
            id INTEGER PRIMARY KEY NOT NULL,
            assignment_id TEXT NOT NULL REFERENCES agent_assignments(id),
            capability_id TEXT NOT NULL,
            created_at_millis INTEGER NOT NULL,
            operation TEXT NOT NULL CHECK (operation IN ('read-context', 'add-comment', 'complete-task', 'start-task', 'acknowledge')),
            target_record_id INTEGER NOT NULL REFERENCES records(id),
            outcome TEXT NOT NULL CHECK (outcome IN ('allowed', 'denied')),
            reason TEXT NOT NULL,
            correlation_id TEXT NOT NULL
        );
        INSERT INTO assignment_authorization_audits SELECT * FROM assignment_authorization_audits_old
            WHERE operation NOT IN ('create-subscription', 'renew-subscription', 'delete-subscription');
        DROP TABLE assignment_authorization_audits_old;",
    ).map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn migrate_agent_profile_schema(connection: &Connection) -> Result<(), RegistrationError> {
    let schema: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'agent_profiles'",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if !schema.contains("'grilling'") {
        connection
            .execute_batch(
                "ALTER TABLE agent_profiles RENAME TO agent_profiles_old;
        CREATE TABLE agent_profiles (
            record_type TEXT NOT NULL CHECK (record_type IN ('spec', 'task', 'grilling')),
            revision INTEGER NOT NULL CHECK (revision > 0),
            provider TEXT NOT NULL,
            agent_kind TEXT NOT NULL,
            model TEXT NOT NULL,
            prompt_template TEXT NOT NULL,
            retry_limit INTEGER NOT NULL,
            retry_backoff_millis INTEGER NOT NULL,
            startup_timeout_millis INTEGER NOT NULL,
            unknown_state_grace_millis INTEGER NOT NULL,
            PRIMARY KEY (record_type, revision)
        );
        INSERT INTO agent_profiles SELECT * FROM agent_profiles_old;
        DROP TABLE agent_profiles_old;",
            )
            .map_err(storage_error)?;
    }
    let columns = connection
        .prepare("PRAGMA table_info(agent_profiles)")
        .map_err(storage_error)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns
        .iter()
        .any(|column| column == "acknowledgement_timeout_millis")
    {
        connection.execute("ALTER TABLE agent_profiles ADD COLUMN acknowledgement_timeout_millis INTEGER NOT NULL DEFAULT 300000", []).map_err(storage_error)?;
    }
    if !columns
        .iter()
        .any(|column| column == "acknowledgement_timeout_overridden")
    {
        connection.execute("ALTER TABLE agent_profiles ADD COLUMN acknowledgement_timeout_overridden INTEGER NOT NULL DEFAULT 0", []).map_err(storage_error)?;
        connection.execute("UPDATE agent_profiles SET acknowledgement_timeout_overridden = 1 WHERE record_type = 'task' AND revision > 1", []).map_err(storage_error)?;
    }
    if !columns.iter().any(|column| column == "launch_policy_json") {
        connection
            .execute(
                "ALTER TABLE agent_profiles ADD COLUMN launch_policy_json TEXT",
                [],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

fn seed_agent_profiles(connection: &Connection) -> Result<(), RegistrationError> {
    for (record_type, prompt_template) in [
        ("spec", "agent-dispatch/spec/v1"),
        ("task", "agent-dispatch/task/v1"),
        ("grilling", "agent-dispatch/grilling/v1"),
    ] {
        connection.execute(
            "INSERT OR IGNORE INTO agent_profiles (record_type, revision, provider, agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis) VALUES (?1, 1, 'herdr', 'pi', 'gpt-5.6-terra', ?2, 3, 1000, 30000, 60000)",
            params![record_type, prompt_template],
        ).map_err(storage_error)?;
    }
    Ok(())
}

fn load_agent_profile(
    connection: &Connection,
    profile_record_type: RecordType,
    revision: u32,
) -> Result<AgentProfile, RegistrationError> {
    connection.query_row(
        "SELECT record_type, revision, provider, agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis, acknowledgement_timeout_millis, acknowledgement_timeout_overridden, launch_policy_json FROM agent_profiles WHERE record_type = ?1 AND revision = ?2",
        params![record_type_name(profile_record_type), revision],
        |row| Ok(AgentProfile {
            record_type: match row.get::<_, String>(0)?.as_str() {
                "spec" => RecordType::Spec,
                "task" => RecordType::Task,
                "grilling" => RecordType::Grilling,
                _ => return Err(rusqlite::Error::InvalidQuery),
            },
            revision: row.get::<_, i64>(1)?.cast_unsigned().try_into().expect("profile revision fits u32"),
            provider: row.get(2)?, agent_kind: row.get(3)?, model: row.get(4)?, prompt_template: row.get(5)?,
            retry_limit: row.get::<_, i64>(6)?.cast_unsigned().try_into().expect("retry limit fits u32"), retry_backoff_millis: row.get::<_, i64>(7)?.cast_unsigned(),
            startup_timeout_millis: row.get::<_, i64>(8)?.cast_unsigned(), unknown_state_grace_millis: row.get::<_, i64>(9)?.cast_unsigned(),
            acknowledgement_timeout_millis: row.get::<_, i64>(10)?.cast_unsigned(),
            acknowledgement_timeout_overridden: row.get(11)?,
            launch_policy: row.get::<_, Option<String>>(12)?.map(|json| serde_json::from_str(&json).map_err(|error| rusqlite::Error::FromSqlConversionFailure(12, rusqlite::types::Type::Text, Box::new(error)))).transpose()?,
        }),
    ).map_err(|error| match error { rusqlite::Error::QueryReturnedNoRows => RegistrationError::new("agent-profile-not-found", "Agent Profile revision does not exist"), error => storage_error(error) })
}

fn assignment_state_name(state: AgentAssignmentState) -> &'static str {
    match state {
        AgentAssignmentState::Provisioning => "provisioning",
        AgentAssignmentState::Working => "working",
        AgentAssignmentState::Idle => "idle",
        AgentAssignmentState::Blocked => "blocked",
        AgentAssignmentState::Unknown => "unknown",
    }
}

fn assignment_state(value: &str) -> Result<AgentAssignmentState, RegistrationError> {
    match value {
        "provisioning" => Ok(AgentAssignmentState::Provisioning),
        "working" => Ok(AgentAssignmentState::Working),
        "idle" => Ok(AgentAssignmentState::Idle),
        "blocked" => Ok(AgentAssignmentState::Blocked),
        "unknown" => Ok(AgentAssignmentState::Unknown),
        _ => Err(RegistrationError::new(
            "storage",
            "stored Assignment has an invalid state",
        )),
    }
}

fn closure_reason_name(reason: AgentAssignmentClosureReason) -> &'static str {
    match reason {
        AgentAssignmentClosureReason::Lost => "lost",
        AgentAssignmentClosureReason::Crashed => "crashed",
        AgentAssignmentClosureReason::Retired => "retired",
        AgentAssignmentClosureReason::Replaced => "replaced",
    }
}

fn closure_reason(value: &str) -> Result<AgentAssignmentClosureReason, RegistrationError> {
    match value {
        "lost" => Ok(AgentAssignmentClosureReason::Lost),
        "crashed" => Ok(AgentAssignmentClosureReason::Crashed),
        "retired" => Ok(AgentAssignmentClosureReason::Retired),
        "replaced" => Ok(AgentAssignmentClosureReason::Replaced),
        _ => Err(RegistrationError::new(
            "storage",
            "stored Assignment has an invalid closure reason",
        )),
    }
}

fn agent_assignment_from_row(
    row: &rusqlite::Row<'_>,
    connection: &Connection,
) -> rusqlite::Result<AgentAssignment> {
    let profile_type: String = row.get(3)?;
    let revision: u32 = row
        .get::<_, i64>(4)?
        .cast_unsigned()
        .try_into()
        .expect("profile revision fits u32");
    let state: String = row.get(7)?;
    let stored_closure_reason: Option<String> = row.get(9)?;
    let state_changed_at_millis = row.get::<_, i64>(10)?.cast_unsigned();
    let next_retry_at_millis = row.get::<_, i64>(11)?.cast_unsigned();
    let retry_count = row
        .get::<_, i64>(12)?
        .cast_unsigned()
        .try_into()
        .expect("retry count fits u32");
    let profile = load_agent_profile(
        connection,
        record_type(&profile_type).map_err(|_| rusqlite::Error::InvalidQuery)?,
        revision,
    )
    .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(AgentAssignment {
        id: row.get(0)?,
        record_id: row.get::<_, i64>(1)?.cast_unsigned(),
        project_slug: row.get(2)?,
        profile,
        attempt_number: row
            .get::<_, i64>(5)?
            .cast_unsigned()
            .try_into()
            .expect("attempt number fits u32"),
        claimed_at_millis: row.get::<_, i64>(6)?.cast_unsigned(),
        state: assignment_state(&state).map_err(|_| rusqlite::Error::InvalidQuery)?,
        state_changed_at_millis,
        next_retry_at_millis,
        retry_count,
        closed_at_millis: row.get::<_, Option<i64>>(8)?.map(i64::cast_unsigned),
        closure_reason: stored_closure_reason
            .map(|reason| closure_reason(&reason).map_err(|_| rusqlite::Error::InvalidQuery))
            .transpose()?,
    })
}

fn effective_limit(connection: &Connection, project_slug: &str) -> Result<u16, RegistrationError> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE slug = ?1)",
            [project_slug],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if !exists {
        return Err(RegistrationError::new(
            "project-not-found",
            format!("Project `{project_slug}` is not registered"),
        ));
    }
    connection.query_row("SELECT task_concurrency_limit FROM project_agent_dispatch_configuration WHERE project_slug = ?1", [project_slug], |row| row.get::<_, i64>(0).map(i64::cast_unsigned)).optional().map_err(storage_error).map(|limit| limit.unwrap_or(1).try_into().expect("database constraint bounds concurrency limit"))
}

impl AgentDispatchRepository for SqliteProjectRepository {
    fn record_update_head(&self) -> Result<RecordUpdateId, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection
            .query_row(
                "SELECT COALESCE(MAX(id), 0) FROM record_updates",
                [],
                |row| row.get::<_, i64>(0).map(i64::cast_unsigned),
            )
            .map(RecordUpdateId)
            .map_err(storage_error)
    }

    fn dispatch_hold(
        &self,
        record_id: u64,
    ) -> Result<Option<forge_app::DispatchHold>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection
            .query_row(
                "SELECT record_id, reason FROM dispatch_holds WHERE record_id = ?1",
                [record_id.cast_signed()],
                |row| {
                    Ok(forge_app::DispatchHold {
                        record_id: row.get::<_, i64>(0)?.cast_unsigned(),
                        reason: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)
    }

    fn hold_dispatch(
        &self,
        record_id: u64,
        reason: &str,
    ) -> Result<forge_app::DispatchHold, RegistrationError> {
        if reason.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-dispatch-hold",
                "Dispatch Hold reason must not be blank",
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let record = load_record(&connection, record_id)?;
        if !matches!(record.record_type, RecordType::Task | RecordType::Grilling) {
            return Err(RegistrationError::new(
                "invalid-dispatch-hold",
                "only a Task or Grilling may have a Dispatch Hold",
            ));
        }
        connection.execute(
            "INSERT INTO dispatch_holds (record_id, reason) VALUES (?1, ?2) ON CONFLICT(record_id) DO UPDATE SET reason = excluded.reason",
            params![record_id.cast_signed(), reason],
        ).map_err(storage_error)?;
        Ok(forge_app::DispatchHold {
            record_id,
            reason: reason.into(),
        })
    }

    fn resume_dispatch(&self, record_id: u64) -> Result<(), RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection
            .execute(
                "DELETE FROM dispatch_holds WHERE record_id = ?1",
                [record_id.cast_signed()],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    fn agent_profile(
        &self,
        record_type: RecordType,
        revision: u32,
    ) -> Result<AgentProfile, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        load_agent_profile(&connection, record_type, revision)
    }

    fn active_task_agent_profile(&self) -> Result<AgentProfile, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let revision: u32 = connection
            .query_row(
                "SELECT MAX(revision) FROM agent_profiles WHERE record_type = 'task'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        load_agent_profile(&connection, RecordType::Task, revision)
    }

    fn configure_task_acknowledgement_timeout(
        &self,
        timeout_millis: Option<u64>,
    ) -> Result<AgentProfile, RegistrationError> {
        let timeout = i64::try_from(timeout_millis.unwrap_or(300_000))
            .ok()
            .filter(|v| *v > 0 && *v <= i64::MAX / 2)
            .ok_or_else(|| {
                RegistrationError::new(
                    "invalid-acknowledgement-timeout",
                    "acknowledgement timeout must be positive and in range",
                )
            })?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        transaction.execute("INSERT INTO agent_profiles (record_type, revision, provider, agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis, acknowledgement_timeout_millis, acknowledgement_timeout_overridden, launch_policy_json) SELECT record_type, revision + 1, provider, agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis, ?1, ?2, launch_policy_json FROM agent_profiles WHERE record_type = 'task' ORDER BY revision DESC LIMIT 1", params![timeout, timeout_millis.is_some()]).map_err(storage_error)?;
        let revision: u32 = transaction
            .query_row(
                "SELECT MAX(revision) FROM agent_profiles WHERE record_type = 'task'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let profile = load_agent_profile(&transaction, RecordType::Task, revision)?;
        transaction.commit().map_err(storage_error)?;
        Ok(profile)
    }

    fn create_agent_assignment(
        &self,
        assignment: &NewAgentAssignment,
    ) -> Result<AgentAssignment, RegistrationError> {
        if assignment.id.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-assignment-id",
                "assignment id must not be blank",
            ));
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let record = load_record(&transaction, assignment.record_id)?;
        if !matches!(
            record.record_type,
            RecordType::Spec | RecordType::Task | RecordType::Grilling
        ) || record.project_slugs.as_slice() != [assignment.project_slug.as_str()]
        {
            return Err(RegistrationError::new(
                "invalid-assignment-record",
                "an Assignment requires a single-Project Spec or Task",
            ));
        }
        let profile = load_agent_profile(
            &transaction,
            record.record_type,
            assignment.profile_revision,
        )?;
        let attempt_number: u32 = transaction.query_row(
            "SELECT COALESCE(MAX(attempt_number), 0) + 1 FROM agent_assignments WHERE record_id = ?1",
            [assignment.record_id.cast_signed()], |row| row.get::<_, i64>(0),
        ).map_err(storage_error)?.cast_unsigned().try_into().expect("attempt number fits u32");
        let claimed_at_millis = now_millis()?;
        match transaction.execute(
            "INSERT INTO agent_assignments (id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state_changed_at_millis, safe_dispatch_generation) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1)",
            params![assignment.id, assignment.record_id.cast_signed(), assignment.project_slug, record_type_name(record.record_type), assignment.profile_revision, attempt_number, claimed_at_millis],
        ) {
            Ok(_) => {},
            Err(error) if matches!(error.sqlite_error_code(), Some(ErrorCode::ConstraintViolation)) => return Err(RegistrationError::new("open-assignment-exists", "Record already has an open Agent Assignment")),
            Err(error) => return Err(storage_error(error)),
        }
        transaction.commit().map_err(storage_error)?;
        Ok(AgentAssignment {
            id: assignment.id.clone(),
            record_id: assignment.record_id,
            project_slug: assignment.project_slug.clone(),
            profile,
            attempt_number,
            claimed_at_millis: claimed_at_millis.cast_unsigned(),
            state: AgentAssignmentState::Provisioning,
            state_changed_at_millis: claimed_at_millis.cast_unsigned(),
            next_retry_at_millis: 0,
            retry_count: 0,
            closed_at_millis: None,
            closure_reason: None,
        })
    }

    fn claim_record_agent_assignment_with_selection(
        &self,
        record_id: u64,
        assignment_id: &str,
        selection: Option<&forge_app::DispatchPreflightSelection>,
    ) -> Result<(AgentAssignment, bool), RegistrationError> {
        if assignment_id.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-assignment-id",
                "assignment id must not be blank",
            ));
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let record = load_record(&transaction, record_id)?;
        if !matches!(record.record_type, RecordType::Task | RecordType::Grilling) {
            return Err(RegistrationError::new(
                "unsupported-record-type",
                "explicit dispatch supports only Tasks and Grillings",
            ));
        }
        let existing = transaction.query_row("SELECT id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state, closed_at_millis, closure_reason, state_changed_at_millis, next_retry_at_millis, retry_count FROM agent_assignments WHERE record_id = ?1 AND closed_at_millis IS NULL", [record_id.cast_signed()], |row| agent_assignment_from_row(row, &transaction)).optional().map_err(storage_error)?;
        if let Some(assignment) = existing {
            return Ok((assignment, true));
        }
        if record.status != RecordStatus::Open
            || record.project_slugs.is_empty()
            || (record.record_type == RecordType::Task && record.project_slugs.len() != 1)
        {
            return Err(RegistrationError::new(
                "record-not-ready",
                "dispatch requires an Open Record with Project scope; Tasks require exactly one Project",
            ));
        }
        let held: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM dispatch_holds WHERE record_id = ?1)",
                [record_id.cast_signed()],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if held {
            return Err(RegistrationError::new(
                "dispatch-held",
                "dispatch requires explicit operator resume; investigate the Dispatch Hold",
            ));
        }
        let blocked: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM record_relationships dependency LEFT JOIN records prerequisite ON prerequisite.id = dependency.to_id WHERE dependency.from_id = ?1 AND dependency.kind = 'depends-on' AND (prerequisite.id IS NULL OR prerequisite.status != 'done'))", [record_id.cast_signed()], |row| row.get(0)).map_err(storage_error)?;
        if blocked {
            return Err(RegistrationError::new(
                "record-not-ready",
                "Record has unresolved prerequisites",
            ));
        }
        let retained: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM agent_assignments a JOIN agent_assignment_resources r ON r.assignment_id = a.id WHERE a.record_id = ?1 AND r.retired_at_millis IS NULL)", [record_id.cast_signed()], |row| row.get(0)).map_err(storage_error)?;
        if retained {
            return Err(RegistrationError::new(
                "assignment-recovery-blocked",
                "retained Assignment resources prohibit replacement; inspect Assignment evidence and resolve recovery blockers",
            ));
        }
        let project_slug = &record.project_slugs[0];
        for scoped_project in &record.project_slugs {
            let limit = effective_limit(&transaction, scoped_project)?;
            let used: i64 = transaction.query_row("SELECT COUNT(*) FROM agent_assignments a JOIN record_projects p ON p.record_id = a.record_id WHERE p.project_slug = ?1 AND (a.closed_at_millis IS NULL OR EXISTS (SELECT 1 FROM agent_assignment_resources r WHERE r.assignment_id = a.id AND r.retired_at_millis IS NULL))", [scoped_project], |row| row.get(0)).map_err(storage_error)?;
            if used >= i64::from(limit) {
                return Err(RegistrationError::new(
                    "project-capacity",
                    "Project dispatch capacity is reserved; inspect open Assignments and recovery blockers",
                ));
            }
        }
        if let Some(selection) = selection {
            workspace_dispatch::verify_preflight_selection(&transaction, record_id, selection)?;
        }
        let revision =
            workspace_dispatch::selected_revision(&transaction, record_id, record.record_type)?;
        let profile = load_agent_profile(&transaction, record.record_type, revision)?;
        let attempt_number: u32 = transaction.query_row("SELECT COALESCE(MAX(attempt_number), 0) + 1 FROM agent_assignments WHERE record_id = ?1", [record_id.cast_signed()], |row| row.get(0)).map_err(storage_error)?;
        let claimed_at_millis = now_millis()?;
        transaction.execute("INSERT INTO agent_assignments (id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state_changed_at_millis, safe_dispatch_generation) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1)", params![assignment_id, record_id.cast_signed(), project_slug, record_type_name(record.record_type), revision, attempt_number, claimed_at_millis]).map_err(storage_error)?;
        let assignment = AgentAssignment {
            id: assignment_id.into(),
            record_id,
            project_slug: project_slug.clone(),
            profile,
            attempt_number,
            claimed_at_millis: claimed_at_millis.cast_unsigned(),
            state: AgentAssignmentState::Provisioning,
            state_changed_at_millis: claimed_at_millis.cast_unsigned(),
            next_retry_at_millis: 0,
            retry_count: 0,
            closed_at_millis: None,
            closure_reason: None,
        };
        transaction.commit().map_err(storage_error)?;
        Ok((assignment, false))
    }

    fn agent_assignment(&self, assignment_id: &str) -> Result<AgentAssignment, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection.query_row("SELECT id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state, closed_at_millis, closure_reason, state_changed_at_millis, next_retry_at_millis, retry_count FROM agent_assignments WHERE id = ?1", [assignment_id], |row| agent_assignment_from_row(row, &connection))
            .map_err(|error| match error { rusqlite::Error::QueryReturnedNoRows => RegistrationError::new("assignment-not-found", "Agent Assignment does not exist"), _ => storage_error(error) })
    }

    fn agent_assignment_info(
        &self,
        assignment_id: &str,
    ) -> Result<forge_app::AgentAssignmentInfo, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        // A transaction keeps reads coherent across other daemon/store connections.
        let transaction = connection.transaction().map_err(storage_error)?;
        let assignment = transaction.query_row("SELECT id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state, closed_at_millis, closure_reason, state_changed_at_millis, next_retry_at_millis, retry_count FROM agent_assignments WHERE id = ?1", [assignment_id], |row| agent_assignment_from_row(row, &transaction))
            .map_err(|error| match error { rusqlite::Error::QueryReturnedNoRows => RegistrationError::new("assignment-not-found", "Agent Assignment does not exist"), _ => storage_error(error) })?;
        let task_status = load_record(&transaction, assignment.record_id)?.status;
        let (start, deadline, acknowledged_at_millis) = transaction.query_row(
            "SELECT launch_started_at_millis, acknowledgement_deadline_millis, acknowledged_at_millis FROM agent_assignments WHERE id = ?1",
            [assignment_id],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, Option<i64>>(2)?.map(i64::cast_unsigned))),
        ).map_err(storage_error)?;
        let launch_intent = match (start, deadline) {
            (Some(start), Some(deadline)) => Some(forge_app::AgentLaunchIntent {
                started_at_millis: start.cast_unsigned(),
                acknowledgement_deadline_millis: deadline.cast_unsigned(),
            }),
            (None, None) => None,
            _ => {
                return Err(RegistrationError::new(
                    "storage",
                    "incomplete launch intent",
                ));
            }
        };
        let dispatch_hold = transaction
            .query_row(
                "SELECT reason FROM dispatch_holds WHERE record_id = ?1",
                [assignment.record_id.cast_signed()],
                |row| {
                    Ok(forge_app::DispatchHold {
                        record_id: assignment.record_id,
                        reason: row.get(0)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)?;
        let resources = transaction.prepare("SELECT assignment_id, resource_kind, resource_id, created_at_millis, retired_at_millis FROM agent_assignment_resources WHERE assignment_id = ?1 ORDER BY created_at_millis, resource_kind, resource_id").map_err(storage_error)?
            .query_map([assignment_id], |row| Ok(AgentAssignmentResource {
                assignment_id: row.get(0)?, resource_kind: row.get(1)?, resource_id: row.get(2)?,
                created_at_millis: row.get::<_, i64>(3)?.cast_unsigned(),
                retired_at_millis: row.get::<_, Option<i64>>(4)?.map(i64::cast_unsigned),
            })).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
        let unknown_resource_resolution = transaction.query_row(
            "SELECT confirmed_at_millis, authority, confirmation FROM assignment_unknown_resource_resolutions WHERE assignment_id = ?1",
            [assignment_id],
            |row| Ok(forge_app::UnknownResourceResolution {
                confirmed_at_millis: row.get::<_, i64>(0)?.cast_unsigned(),
                authority: row.get(1)?, confirmation: row.get(2)?,
            }),
        ).optional().map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(forge_app::AgentAssignmentInfo {
            assignment,
            task_status,
            launch_intent,
            acknowledged_at_millis,
            dispatch_hold,
            resources,
            unknown_resource_resolution,
        })
    }

    fn list_agent_assignments(
        &self,
        record_id: u64,
    ) -> Result<Vec<AgentAssignment>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        load_record(&connection, record_id)?;
        let rows = connection.prepare("SELECT id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state, closed_at_millis, closure_reason, state_changed_at_millis, next_retry_at_millis, retry_count FROM agent_assignments WHERE record_id = ?1 ORDER BY attempt_number DESC").map_err(storage_error)?
            .query_map([record_id.cast_signed()], |row| agent_assignment_from_row(row, &connection))
            .map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
        Ok(rows)
    }

    fn list_open_agent_assignments(&self) -> Result<Vec<AgentAssignment>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection.prepare("SELECT id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state, closed_at_millis, closure_reason, state_changed_at_millis, next_retry_at_millis, retry_count FROM agent_assignments WHERE closed_at_millis IS NULL ORDER BY id")
            .map_err(storage_error)?
            .query_map([], |row| agent_assignment_from_row(row, &connection))
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    fn try_acquire_agent_dispatch_scan_lease(
        &self,
        owner: &str,
        now_millis: u64,
        duration_millis: u64,
    ) -> Result<bool, RegistrationError> {
        if owner.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-scan-owner",
                "scan lease owner must not be blank",
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let until = now_millis.saturating_add(duration_millis).cast_signed();
        let changed = connection.execute(
            "INSERT INTO agent_dispatch_configuration (singleton, scan_lease_owner, scan_lease_until_millis) VALUES (1, ?1, ?2) ON CONFLICT(singleton) DO UPDATE SET scan_lease_owner = excluded.scan_lease_owner, scan_lease_until_millis = excluded.scan_lease_until_millis WHERE agent_dispatch_configuration.scan_lease_until_millis IS NULL OR agent_dispatch_configuration.scan_lease_until_millis <= ?3 OR agent_dispatch_configuration.scan_lease_owner = ?1",
            params![owner, until, now_millis.cast_signed()],
        ).map_err(storage_error)?;
        Ok(changed == 1)
    }

    fn claim_next_agent_assignment_with_selection(
        &self,
        project_slug: &str,
        assignment_id: &str,
        selection: Option<&forge_app::DispatchPreflightSelection>,
    ) -> Result<Option<AgentAssignment>, RegistrationError> {
        if assignment_id.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-assignment-id",
                "assignment id must not be blank",
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(storage_error)?;
        let result = (|| {
            let limit = effective_limit(&connection, project_slug)?;
            let used: u16 = connection.query_row("SELECT COUNT(*) FROM agent_assignments a JOIN record_projects p ON p.record_id = a.record_id WHERE p.project_slug = ?1 AND (a.closed_at_millis IS NULL OR EXISTS (SELECT 1 FROM agent_assignment_resources r WHERE r.assignment_id = a.id AND r.retired_at_millis IS NULL))", [project_slug], |row| row.get::<_, i64>(0)).map_err(storage_error)?.cast_unsigned().try_into().expect("assignment count fits u16");
            if used >= limit {
                return Ok(None);
            }
            let record_id = connection.query_row("SELECT record.id FROM records record JOIN record_projects scope ON scope.record_id = record.id WHERE record.record_type = 'task' AND record.status = 'open' AND scope.project_slug = ?1 AND (SELECT COUNT(*) FROM record_projects only_scope WHERE only_scope.record_id = record.id) = 1 AND NOT EXISTS (SELECT 1 FROM dispatch_holds hold WHERE hold.record_id = record.id) AND NOT EXISTS (SELECT 1 FROM agent_assignments assignment WHERE assignment.record_id = record.id AND (assignment.closed_at_millis IS NULL OR EXISTS (SELECT 1 FROM agent_assignment_resources resource WHERE resource.assignment_id = assignment.id AND resource.retired_at_millis IS NULL))) AND NOT EXISTS (SELECT 1 FROM record_relationships dependency JOIN records prerequisite ON prerequisite.id = dependency.to_id WHERE dependency.from_id = record.id AND dependency.kind = 'depends-on' AND prerequisite.status != 'done') ORDER BY record.id LIMIT 1", [project_slug], |row| row.get::<_, i64>(0).map(i64::cast_unsigned)).optional().map_err(storage_error)?;
            let Some(record_id) = record_id else {
                return Ok(None);
            };
            if let Some(selection) = selection {
                workspace_dispatch::verify_preflight_selection(&connection, record_id, selection)?;
            }
            let revision =
                workspace_dispatch::selected_revision(&connection, record_id, RecordType::Task)?;
            let profile = load_agent_profile(&connection, RecordType::Task, revision)?;
            let attempt: u32 = connection.query_row("SELECT COALESCE(MAX(attempt_number), 0) + 1 FROM agent_assignments WHERE record_id = ?1", [record_id.cast_signed()], |row| row.get::<_, i64>(0)).map_err(storage_error)?.cast_unsigned().try_into().expect("attempt number fits u32");
            let claimed_at_millis = now_millis()?;
            connection.execute("INSERT INTO agent_assignments (id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state, state_changed_at_millis, safe_dispatch_generation) VALUES (?1, ?2, ?3, 'task', ?6, ?4, ?5, 'provisioning', ?5, 1)", params![assignment_id, record_id.cast_signed(), project_slug, attempt, claimed_at_millis, revision]).map_err(|error| if matches!(error.sqlite_error_code(), Some(ErrorCode::ConstraintViolation)) { RegistrationError::new("assignment-conflict", "Agent Assignment already exists or Task is assigned") } else { storage_error(error) })?;
            Ok(Some(AgentAssignment {
                id: assignment_id.into(),
                record_id,
                project_slug: project_slug.into(),
                profile,
                attempt_number: attempt,
                claimed_at_millis: claimed_at_millis.cast_unsigned(),
                state: AgentAssignmentState::Provisioning,
                state_changed_at_millis: claimed_at_millis.cast_unsigned(),
                next_retry_at_millis: 0,
                retry_count: 0,
                closed_at_millis: None,
                closure_reason: None,
            }))
        })();
        match result {
            Ok(assignment) => {
                connection.execute_batch("COMMIT").map_err(storage_error)?;
                Ok(assignment)
            }
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    fn compare_and_set_agent_assignment_state(
        &self,
        assignment_id: &str,
        expected: AgentAssignmentState,
        next: AgentAssignmentState,
    ) -> Result<Option<AgentAssignment>, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let changed = transaction
            .execute(
                "UPDATE agent_assignments SET state = ?1, state_changed_at_millis = ?2, next_retry_at_millis = 0, retry_count = 0 WHERE id = ?3 AND state = ?4 AND closed_at_millis IS NULL",
                params![assignment_state_name(next), now_millis()?, assignment_id, assignment_state_name(expected)],
            )
            .map_err(storage_error)?;
        let assignment = if changed == 1 {
            Some(transaction.query_row("SELECT id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state, closed_at_millis, closure_reason, state_changed_at_millis, next_retry_at_millis, retry_count FROM agent_assignments WHERE id = ?1", [assignment_id], |row| agent_assignment_from_row(row, &transaction)).map_err(storage_error)?)
        } else {
            None
        };
        transaction.commit().map_err(storage_error)?;
        Ok(assignment)
    }

    fn acknowledge_agent_assignment(
        &self,
        assignment_id: &str,
    ) -> Result<AgentAssignment, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let changed = transaction.execute("UPDATE agent_assignments SET state = CASE WHEN acknowledged_at_millis IS NULL THEN 'working' ELSE state END, state_changed_at_millis = CASE WHEN acknowledged_at_millis IS NULL THEN ?2 ELSE state_changed_at_millis END, acknowledged_at_millis = COALESCE(acknowledged_at_millis, ?2), next_retry_at_millis = 0, retry_count = 0 WHERE id = ?1 AND closed_at_millis IS NULL AND (state = 'provisioning' OR acknowledged_at_millis IS NOT NULL)", params![assignment_id, now_millis()?]).map_err(storage_error)?;
        if changed != 1 {
            return Err(RegistrationError::new(
                "assignment-acknowledgement-denied",
                "Assignment is closed or not provisioning",
            ));
        }
        let assignment = transaction.query_row("SELECT id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state, closed_at_millis, closure_reason, state_changed_at_millis, next_retry_at_millis, retry_count FROM agent_assignments WHERE id = ?1", [assignment_id], |row| agent_assignment_from_row(row, &transaction)).map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(assignment)
    }

    fn close_agent_assignment(
        &self,
        assignment_id: &str,
        reason: AgentAssignmentClosureReason,
    ) -> Result<AgentAssignment, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let closed_at_millis = now_millis()?;
        let reason_name = closure_reason_name(reason);
        if transaction.execute("UPDATE agent_assignments SET closed_at_millis = ?1, closure_reason = ?2 WHERE id = ?3 AND closed_at_millis IS NULL", params![closed_at_millis, reason_name, assignment_id]).map_err(storage_error)? != 1 {
            return Err(RegistrationError::new("assignment-not-open", "Agent Assignment is absent or already closed"));
        }
        let assignment = transaction.query_row("SELECT id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state, closed_at_millis, closure_reason, state_changed_at_millis, next_retry_at_millis, retry_count FROM agent_assignments WHERE id = ?1", [assignment_id], |row| agent_assignment_from_row(row, &transaction)).map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(assignment)
    }

    fn compare_and_close_agent_assignment(
        &self,
        assignment_id: &str,
        expected: AgentAssignmentState,
        reason: AgentAssignmentClosureReason,
    ) -> Result<Option<AgentAssignment>, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let changed = transaction.execute(
            "UPDATE agent_assignments SET closed_at_millis = ?1, closure_reason = ?2 WHERE id = ?3 AND state = ?4 AND closed_at_millis IS NULL",
            params![now_millis()?, closure_reason_name(reason), assignment_id, assignment_state_name(expected)],
        ).map_err(storage_error)?;
        let assignment = if changed == 1 {
            Some(transaction.query_row("SELECT id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state, closed_at_millis, closure_reason, state_changed_at_millis, next_retry_at_millis, retry_count FROM agent_assignments WHERE id = ?1", [assignment_id], |row| agent_assignment_from_row(row, &transaction)).map_err(storage_error)?)
        } else {
            None
        };
        transaction.commit().map_err(storage_error)?;
        Ok(assignment)
    }

    fn schedule_agent_assignment_retry(
        &self,
        assignment_id: &str,
        expected: AgentAssignmentState,
        next_retry_at_millis: u64,
        retry_count: u32,
    ) -> Result<bool, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let changed = connection.execute(
            "UPDATE agent_assignments SET next_retry_at_millis = ?1, retry_count = ?2 WHERE id = ?3 AND state = ?4 AND closed_at_millis IS NULL",
            params![next_retry_at_millis.cast_signed(), i64::from(retry_count), assignment_id, assignment_state_name(expected)],
        ).map_err(storage_error)?;
        Ok(changed == 1)
    }

    fn begin_agent_launch(
        &self,
        assignment_id: &str,
        now_millis: u64,
    ) -> Result<bool, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let now = i64::try_from(now_millis)
            .map_err(|_| RegistrationError::new("invalid-time", "launch time is out of range"))?;
        let changed = connection.execute("UPDATE agent_assignments SET launch_started_at_millis = ?2, acknowledgement_deadline_millis = ?2 + (SELECT acknowledgement_timeout_millis FROM agent_profiles WHERE record_type = profile_record_type AND revision = profile_revision) WHERE id = ?1 AND closed_at_millis IS NULL AND state = 'provisioning' AND launch_started_at_millis IS NULL AND ?2 <= 9223372036854775807 - (SELECT acknowledgement_timeout_millis FROM agent_profiles WHERE record_type = profile_record_type AND revision = profile_revision)", params![assignment_id, now]).map_err(storage_error)?;
        Ok(changed == 1)
    }

    fn agent_launch_intent(
        &self,
        assignment_id: &str,
    ) -> Result<Option<forge_app::AgentLaunchIntent>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let intent = connection.query_row("SELECT launch_started_at_millis, acknowledgement_deadline_millis FROM agent_assignments WHERE id = ?1", [assignment_id], |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))).map_err(storage_error)?;
        match intent {
            (Some(start), Some(deadline)) => Ok(Some(forge_app::AgentLaunchIntent {
                started_at_millis: start.cast_unsigned(),
                acknowledgement_deadline_millis: deadline.cast_unsigned(),
            })),
            (None, None) => Ok(None),
            _ => Err(RegistrationError::new(
                "storage",
                "incomplete launch intent",
            )),
        }
    }

    fn hold_uncertain_agent_assignment(
        &self,
        assignment_id: &str,
    ) -> Result<bool, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection.execute("INSERT OR IGNORE INTO agent_assignment_resources (assignment_id, resource_kind, resource_id, created_at_millis) SELECT id, 'provision-intent', 'once', ?2 FROM agent_assignments WHERE id = ?1 AND closed_at_millis IS NULL AND state = 'provisioning'", params![assignment_id, now_millis()?]).map_err(storage_error)?;
        let changed = connection.execute("INSERT OR IGNORE INTO dispatch_holds (record_id, reason) SELECT record_id, 'Automatic dispatch stopped after an uncertain provisioning attempt; investigate the open Assignment and potentially live resources.' FROM agent_assignments WHERE id = ?1 AND closed_at_millis IS NULL AND state = 'provisioning'", [assignment_id]).map_err(storage_error)?;
        Ok(changed == 1)
    }

    fn guard_agent_provisioning(
        &self,
        assignment_id: &str,
        now_millis: u64,
    ) -> Result<bool, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let now = i64::try_from(now_millis).map_err(|_| {
            RegistrationError::new("invalid-time", "reconciliation time is out of range")
        })?;
        // The conditional insert and acknowledgement serialize in SQLite. Never replace an
        // operator's existing hold, and never place one after acknowledgement has won.
        connection.execute("INSERT OR IGNORE INTO dispatch_holds (record_id, reason) SELECT record_id, CASE WHEN acknowledgement_deadline_millis IS NOT NULL THEN 'Agent acknowledgement deadline expired; investigate the open Assignment and potentially live resources before retirement or explicit resolution.' ELSE 'Uncertain legacy or interrupted provisioning attempt; investigate the open Assignment and potentially live resources before retirement or explicit resolution.' END FROM agent_assignments a WHERE id = ?1 AND closed_at_millis IS NULL AND state = 'provisioning' AND ((acknowledgement_deadline_millis IS NOT NULL AND acknowledgement_deadline_millis <= ?2) OR (acknowledgement_deadline_millis IS NULL AND (safe_dispatch_generation = 0 OR EXISTS (SELECT 1 FROM agent_assignment_resources r WHERE r.assignment_id = a.id AND r.resource_kind = 'provision-intent'))))", params![assignment_id, now]).map_err(storage_error)?;
        connection.query_row("SELECT EXISTS(SELECT 1 FROM agent_assignments a WHERE id = ?1 AND closed_at_millis IS NULL AND state = 'provisioning' AND safe_dispatch_generation = 1 AND launch_started_at_millis IS NULL AND acknowledgement_deadline_millis IS NULL AND NOT EXISTS (SELECT 1 FROM dispatch_holds h WHERE h.record_id = a.record_id) AND NOT EXISTS (SELECT 1 FROM agent_assignment_resources r WHERE r.assignment_id = a.id AND r.resource_kind = 'provision-intent'))", [assignment_id], |row| row.get(0)).map_err(storage_error)
    }

    fn begin_agent_provision(&self, assignment_id: &str) -> Result<bool, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let changed = connection.execute(
            "INSERT OR IGNORE INTO agent_assignment_resources (assignment_id, resource_kind, resource_id, created_at_millis) SELECT id, 'provision-intent', 'once', ?2 FROM agent_assignments WHERE id = ?1 AND closed_at_millis IS NULL",
            params![assignment_id, now_millis()?],
        ).map_err(storage_error)?;
        Ok(changed == 1)
    }

    fn add_agent_assignment_resource(
        &self,
        resource: &NewAgentAssignmentResource,
    ) -> Result<AgentAssignmentResource, RegistrationError> {
        if resource.resource_kind.trim().is_empty() || resource.resource_id.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-assignment-resource",
                "assignment resource kind and id must not be blank",
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_assignments WHERE id = ?1)",
                [&resource.assignment_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !exists {
            return Err(RegistrationError::new(
                "assignment-not-found",
                "Agent Assignment does not exist",
            ));
        }
        let created_at_millis = now_millis()?;
        connection.execute("INSERT OR IGNORE INTO agent_assignment_resources (assignment_id, resource_kind, resource_id, created_at_millis) VALUES (?1, ?2, ?3, ?4)", params![resource.assignment_id, resource.resource_kind, resource.resource_id, created_at_millis]).map_err(storage_error)?;
        connection.query_row("SELECT assignment_id, resource_kind, resource_id, created_at_millis, retired_at_millis FROM agent_assignment_resources WHERE assignment_id = ?1 AND resource_kind = ?2 AND resource_id = ?3", params![resource.assignment_id, resource.resource_kind, resource.resource_id], |row| Ok(AgentAssignmentResource { assignment_id: row.get(0)?, resource_kind: row.get(1)?, resource_id: row.get(2)?, created_at_millis: row.get::<_, i64>(3)?.cast_unsigned(), retired_at_millis: row.get::<_, Option<i64>>(4)?.map(i64::cast_unsigned) })).map_err(storage_error)
    }

    fn retire_agent_assignment_resource(
        &self,
        assignment_id: &str,
        resource_kind: &str,
        resource_id: &str,
    ) -> Result<AgentAssignmentResource, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection.execute("UPDATE agent_assignment_resources SET retired_at_millis = COALESCE(retired_at_millis, ?1) WHERE assignment_id = ?2 AND resource_kind = ?3 AND resource_id = ?4", params![now_millis()?, assignment_id, resource_kind, resource_id]).map_err(storage_error)?;
        connection.query_row("SELECT assignment_id, resource_kind, resource_id, created_at_millis, retired_at_millis FROM agent_assignment_resources WHERE assignment_id = ?1 AND resource_kind = ?2 AND resource_id = ?3", params![assignment_id, resource_kind, resource_id], |row| Ok(AgentAssignmentResource { assignment_id: row.get(0)?, resource_kind: row.get(1)?, resource_id: row.get(2)?, created_at_millis: row.get::<_, i64>(3)?.cast_unsigned(), retired_at_millis: row.get::<_, Option<i64>>(4)?.map(i64::cast_unsigned) })).map_err(|error| match error { rusqlite::Error::QueryReturnedNoRows => RegistrationError::new("assignment-resource-not-found", "Agent Assignment resource does not exist"), error => storage_error(error) })
    }

    fn resolve_unknown_agent_resources(
        &self,
        assignment_id: &str,
    ) -> Result<(), RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let closed = transaction
            .query_row(
                "SELECT closed_at_millis IS NOT NULL FROM agent_assignments WHERE id = ?1",
                [assignment_id],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or_else(|| {
                RegistrationError::new("assignment-not-found", "Agent Assignment does not exist")
            })?;
        if !closed {
            return Err(RegistrationError::new(
                "assignment-not-revoked",
                "Retire the Assignment to revoke its capability before resolving unknown resources",
            ));
        }
        let now = now_millis()?;
        // Only the uncertainty marker is covered by the assertion. Known identities and
        // retirement-pending must still be resolved through verified provider cleanup.
        transaction.execute(
            "INSERT OR IGNORE INTO assignment_unknown_resource_resolutions (assignment_id, confirmed_at_millis, authority, confirmation) VALUES (?1, ?2, ?3, ?4)",
            params![assignment_id, now, "trusted-operator-rpc", "Operator confirmed the old agent can no longer act and unknown resources have been removed or otherwise made safe; this assertion is not provider proof and revocation cannot undo external actions."],
        ).map_err(storage_error)?;
        transaction.execute(
            "UPDATE agent_assignment_resources SET retired_at_millis = ?2 WHERE assignment_id = ?1 AND resource_kind = 'provision-intent' AND retired_at_millis IS NULL",
            params![assignment_id, now],
        ).map_err(storage_error)?;
        transaction.commit().map_err(storage_error)
    }

    fn list_agent_assignment_resources(
        &self,
        assignment_id: &str,
    ) -> Result<Vec<AgentAssignmentResource>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection.prepare("SELECT assignment_id, resource_kind, resource_id, created_at_millis, retired_at_millis FROM agent_assignment_resources WHERE assignment_id = ?1 ORDER BY created_at_millis, resource_kind, resource_id").map_err(storage_error)?.query_map([assignment_id], |row| Ok(AgentAssignmentResource { assignment_id: row.get(0)?, resource_kind: row.get(1)?, resource_id: row.get(2)?, created_at_millis: row.get::<_, i64>(3)?.cast_unsigned(), retired_at_millis: row.get::<_, Option<i64>>(4)?.map(i64::cast_unsigned) })).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)
    }

    fn add_assignment_authorization_audit(
        &self,
        audit: &AssignmentAuthorizationAudit,
    ) -> Result<(), RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let created_at_millis = if audit.created_at_millis == 0 {
            now_millis()?
        } else {
            audit.created_at_millis.cast_signed()
        };
        let operation = match audit.operation {
            AssignmentAuthorizationOperation::ReadContext => "read-context",
            AssignmentAuthorizationOperation::AddComment => "add-comment",
            AssignmentAuthorizationOperation::CompleteTask => "complete-task",
            AssignmentAuthorizationOperation::StartTask => "start-task",
            AssignmentAuthorizationOperation::Acknowledge => "acknowledge",
        };
        let outcome = match audit.outcome {
            AssignmentAuthorizationOutcome::Allowed => "allowed",
            AssignmentAuthorizationOutcome::Denied => "denied",
        };
        connection.execute("INSERT INTO assignment_authorization_audits (assignment_id, capability_id, created_at_millis, operation, target_record_id, outcome, reason, correlation_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", params![audit.assignment_id, audit.capability_id, created_at_millis, operation, audit.target_record_id.cast_signed(), outcome, audit.reason, audit.correlation_id]).map_err(storage_error)?;
        Ok(())
    }

    fn list_assignment_authorization_audits(
        &self,
        assignment_id: &str,
    ) -> Result<Vec<AssignmentAuthorizationAudit>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection.prepare("SELECT assignment_id, capability_id, created_at_millis, operation, target_record_id, outcome, reason, correlation_id FROM assignment_authorization_audits WHERE assignment_id = ?1 ORDER BY id").map_err(storage_error)?
            .query_map([assignment_id], |row| {
                let operation = match row.get::<_, String>(3)?.as_str() { "read-context" => AssignmentAuthorizationOperation::ReadContext, "add-comment" => AssignmentAuthorizationOperation::AddComment, "complete-task" => AssignmentAuthorizationOperation::CompleteTask, "start-task" => AssignmentAuthorizationOperation::StartTask, "acknowledge" => AssignmentAuthorizationOperation::Acknowledge, _ => return Err(rusqlite::Error::InvalidQuery) };
                let outcome = match row.get::<_, String>(5)?.as_str() { "allowed" => AssignmentAuthorizationOutcome::Allowed, "denied" => AssignmentAuthorizationOutcome::Denied, _ => return Err(rusqlite::Error::InvalidQuery) };
                Ok(AssignmentAuthorizationAudit { assignment_id: row.get(0)?, capability_id: row.get(1)?, created_at_millis: row.get::<_, i64>(2)?.cast_unsigned(), operation, target_record_id: row.get::<_, i64>(4)?.cast_unsigned(), outcome, reason: row.get(6)?, correlation_id: row.get(7)? })
            }).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)
    }

    fn assignment_execution_directory(
        &self,
        assignment_id: &str,
    ) -> Result<Option<PathBuf>, RegistrationError> {
        let assignment = self.agent_assignment(assignment_id)?;
        let current = self.agent_workspace(assignment.record_id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "store lock poisoned"))?;
        if let Some(path) = current {
            let path = path.to_str().ok_or_else(|| {
                RegistrationError::new(
                    "assignment-execution-unavailable",
                    "execution directory cannot be represented",
                )
            })?;
            connection.execute("INSERT OR IGNORE INTO assignment_execution_bindings (assignment_id, directory) VALUES (?1, ?2)", params![assignment_id, path]).map_err(storage_error)?;
        }
        connection
            .query_row(
                "SELECT directory FROM assignment_execution_bindings WHERE assignment_id = ?1",
                [assignment_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map(|path| path.map(PathBuf::from))
            .map_err(storage_error)
    }

    fn mark_assignment_context_read(&self, assignment_id: &str) -> Result<(), RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "store lock poisoned"))?;
        connection
            .execute(
                "INSERT OR IGNORE INTO assignment_context_bootstrap (assignment_id) VALUES (?1)",
                [assignment_id],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    fn assignment_context_read(&self, assignment_id: &str) -> Result<bool, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "store lock poisoned"))?;
        connection.query_row("SELECT EXISTS(SELECT 1 FROM assignment_context_bootstrap WHERE assignment_id = ?1)", [assignment_id], |row| row.get(0)).map_err(storage_error)
    }

    fn agent_workspace(&self, record_id: u64) -> Result<Option<PathBuf>, RegistrationError> {
        self.record_workspace(record_id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection
            .query_row(
                "SELECT w.root FROM records r JOIN workspaces w ON w.id = r.workspace_id
                 WHERE r.id = ?1 AND r.record_type = 'grilling'
                 UNION ALL
                 SELECT c.path FROM worktree_claims c JOIN records r ON r.id = c.record_id
                 WHERE c.record_id = ?1 AND c.state = 'active' AND r.record_type = 'task'",
                [record_id.cast_signed()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage_error)
            .map(|path| path.map(PathBuf::from))
    }

    fn assignment_dispatch_workspace(
        &self,
        assignment_id: &str,
    ) -> Result<Option<DispatchWorkspace>, RegistrationError> {
        let connection = self.connection.lock().map_err(|_| {
            RegistrationError::new("storage", "Assignment storage lock is poisoned")
        })?;
        let identity: Option<String> = connection
            .query_row(
                "SELECT dispatch_workspace_id FROM agent_assignments WHERE id = ?1",
                [assignment_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        Ok(identity.map(|id| DispatchWorkspace { id }))
    }

    fn dispatch_workspace(&self) -> Result<Option<DispatchWorkspace>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection
            .query_row(
                "SELECT dispatch_workspace_id FROM agent_dispatch_configuration WHERE singleton = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(storage_error)
            .map(|workspace| workspace.flatten().map(|id| DispatchWorkspace { id }))
    }

    fn retain_dispatch_workspace(
        &self,
        workspace: &DispatchWorkspace,
    ) -> Result<DispatchWorkspace, RegistrationError> {
        if workspace.id.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-dispatch-workspace",
                "dispatch workspace requires an ID",
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection.execute(
            "INSERT INTO agent_dispatch_configuration (singleton, dispatch_workspace_id) VALUES (1, ?1) ON CONFLICT(singleton) DO UPDATE SET dispatch_workspace_id = excluded.dispatch_workspace_id WHERE agent_dispatch_configuration.dispatch_workspace_id IS NULL",
            params![workspace.id],
        ).map_err(storage_error)?;
        let retained = connection.query_row(
            "SELECT dispatch_workspace_id FROM agent_dispatch_configuration WHERE singleton = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ).map_err(storage_error)?;
        match retained {
            Some(id) if id == workspace.id => Ok(workspace.clone()),
            Some(_) => Err(RegistrationError::new(
                "dispatch-workspace-conflict",
                "a different Forge Dispatch Workspace is already retained",
            )),
            _ => Err(RegistrationError::new(
                "storage",
                "stored dispatch workspace is incomplete",
            )),
        }
    }

    fn select_dispatch_workspace(
        &self,
        workspace: &DispatchWorkspace,
    ) -> Result<DispatchWorkspace, RegistrationError> {
        if workspace.id.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-dispatch-workspace",
                "workspace requires an ID",
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let changed = connection.execute(
            "INSERT INTO agent_dispatch_configuration (singleton, dispatch_workspace_id)
             SELECT 1, ?1 WHERE NOT EXISTS (SELECT 1 FROM agent_assignments WHERE closed_at_millis IS NULL)
               AND NOT EXISTS (SELECT 1 FROM agent_assignment_resources WHERE retired_at_millis IS NULL)
             ON CONFLICT(singleton) DO UPDATE SET dispatch_workspace_id = excluded.dispatch_workspace_id,
               dispatch_workspace_root = NULL
             WHERE NOT EXISTS (SELECT 1 FROM agent_assignments WHERE closed_at_millis IS NULL)
               AND NOT EXISTS (SELECT 1 FROM agent_assignment_resources WHERE retired_at_millis IS NULL)",
            params![workspace.id],
        ).map_err(storage_error)?;
        if changed == 0 {
            let current: Option<Option<String>> = connection.query_row(
                "SELECT dispatch_workspace_id FROM agent_dispatch_configuration WHERE singleton = 1", [],
                |row| row.get(0),
            ).optional().map_err(storage_error)?;
            if current.flatten().as_deref() != Some(&workspace.id) {
                return Err(RegistrationError::new(
                    "dispatch-workspace-busy",
                    "unresolved Assignments or retained resources prevent changing workspace; inspect dispatcher status and assignment-info, then safely retire/resolve Assignments before retrying",
                ));
            }
        }
        Ok(workspace.clone())
    }

    fn unset_dispatch_workspace(&self) -> Result<(), RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let changed = connection.execute(
            "UPDATE agent_dispatch_configuration SET dispatch_workspace_id = NULL, dispatch_workspace_root = NULL
             WHERE singleton = 1
               AND NOT EXISTS (SELECT 1 FROM agent_assignments WHERE closed_at_millis IS NULL)
               AND NOT EXISTS (SELECT 1 FROM agent_assignment_resources WHERE retired_at_millis IS NULL)",
            [],
        ).map_err(storage_error)?;
        if changed == 0 {
            let current: Option<Option<String>> = connection.query_row(
                "SELECT dispatch_workspace_id FROM agent_dispatch_configuration WHERE singleton = 1", [],
                |row| row.get(0),
            ).optional().map_err(storage_error)?;
            if current.flatten().is_some() {
                return Err(RegistrationError::new(
                    "dispatch-workspace-busy",
                    "unresolved Assignments or retained resources prevent unsetting workspace; inspect dispatcher status and assignment-info, then safely retire/resolve Assignments before retrying",
                ));
            }
        }
        Ok(())
    }

    fn set_herdr_socket_path(&self, path: Option<&Path>) -> Result<(), RegistrationError> {
        if path.is_some_and(|path| !path.is_absolute()) {
            return Err(RegistrationError::new(
                "invalid-socket-path",
                "Herdr socket path must be absolute",
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let value = path.map(Path::to_string_lossy);
        let changed = connection.execute(
            "INSERT INTO agent_dispatch_configuration (singleton, herdr_socket_path)
             SELECT 1, ?1
             WHERE NOT EXISTS (SELECT 1 FROM agent_assignments WHERE closed_at_millis IS NULL)
               AND NOT EXISTS (SELECT 1 FROM agent_assignment_resources WHERE retired_at_millis IS NULL)
             ON CONFLICT(singleton) DO UPDATE SET herdr_socket_path = excluded.herdr_socket_path
             WHERE NOT EXISTS (SELECT 1 FROM agent_assignments WHERE closed_at_millis IS NULL)
               AND NOT EXISTS (SELECT 1 FROM agent_assignment_resources WHERE retired_at_millis IS NULL)",
            [value.as_deref()],
        ).map_err(storage_error)?;
        if changed == 0 {
            let current: Option<Option<String>> = connection.query_row(
                "SELECT herdr_socket_path FROM agent_dispatch_configuration WHERE singleton = 1", [],
                |row| row.get(0),
            ).optional().map_err(storage_error)?;
            if current.flatten().as_deref() != value.as_deref() {
                return Err(RegistrationError::new(
                    "dispatch-endpoint-busy",
                    "unresolved Assignments or retained resources prevent changing herdr-socket; inspect dispatcher status and assignment-info, then safely retire/resolve Assignments before retrying",
                ));
            }
        }
        Ok(())
    }

    fn herdr_socket_path(&self) -> Result<Option<std::path::PathBuf>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection
            .query_row(
                "SELECT herdr_socket_path FROM agent_dispatch_configuration WHERE singleton = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(storage_error)
            .map(|path| path.flatten().map(Into::into))
    }

    fn set_project_task_concurrency_limit(
        &self,
        project_slug: &str,
        limit: Option<u16>,
    ) -> Result<(), RegistrationError> {
        if limit.is_some_and(|limit| !(1..=1024).contains(&limit)) {
            return Err(RegistrationError::new(
                "invalid-concurrency-limit",
                "Project Task concurrency limit must be between 1 and 1024",
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE slug = ?1)",
                [project_slug],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !exists {
            return Err(RegistrationError::new(
                "project-not-found",
                format!("Project `{project_slug}` is not registered"),
            ));
        }
        if let Some(limit) = limit {
            connection.execute("INSERT INTO project_agent_dispatch_configuration (project_slug, task_concurrency_limit) VALUES (?1, ?2) ON CONFLICT(project_slug) DO UPDATE SET task_concurrency_limit = excluded.task_concurrency_limit", params![project_slug, limit]).map_err(storage_error)?;
        } else {
            connection
                .execute(
                    "DELETE FROM project_agent_dispatch_configuration WHERE project_slug = ?1",
                    [project_slug],
                )
                .map_err(storage_error)?;
        }
        Ok(())
    }

    fn effective_project_task_concurrency_limit(
        &self,
        project_slug: &str,
    ) -> Result<u16, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE slug = ?1)",
                [project_slug],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !exists {
            return Err(RegistrationError::new(
                "project-not-found",
                format!("Project `{project_slug}` is not registered"),
            ));
        }
        connection.query_row("SELECT task_concurrency_limit FROM project_agent_dispatch_configuration WHERE project_slug = ?1", [project_slug], |row| row.get::<_, i64>(0).map(i64::cast_unsigned)).optional().map_err(storage_error).map(|limit| limit.unwrap_or(1).try_into().expect("database constraint bounds concurrency limit"))
    }

    fn project_task_concurrency_limit(
        &self,
        project_slug: &str,
    ) -> Result<Option<u16>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE slug = ?1)",
                [project_slug],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !exists {
            return Err(RegistrationError::new(
                "project-not-found",
                format!("Project `{project_slug}` is not registered"),
            ));
        }
        connection
            .query_row(
                "SELECT task_concurrency_limit FROM project_agent_dispatch_configuration WHERE project_slug = ?1",
                [project_slug],
                |row| row.get::<_, i64>(0).map(i64::cast_unsigned),
            )
            .optional()
            .map_err(storage_error)
            .map(|limit| limit.map(|value| value.try_into().expect("database constraint bounds concurrency limit")))
    }
}

fn relationship_kind_name(kind: RecordRelationshipKind) -> &'static str {
    match kind {
        RecordRelationshipKind::ParentChild => "parent-child",
        RecordRelationshipKind::DependsOn => "depends-on",
        RecordRelationshipKind::GeneratedBy => "generated-by",
        RecordRelationshipKind::Initiative => "initiative",
    }
}
fn parse_relationship_kind(kind: &str) -> rusqlite::Result<RecordRelationshipKind> {
    match kind {
        "parent-child" => Ok(RecordRelationshipKind::ParentChild),
        "depends-on" => Ok(RecordRelationshipKind::DependsOn),
        "generated-by" => Ok(RecordRelationshipKind::GeneratedBy),
        "initiative" => Ok(RecordRelationshipKind::Initiative),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn status_name(status: RecordStatus) -> &'static str {
    match status {
        RecordStatus::Open => "open",
        RecordStatus::InProgress => "in_progress",
        RecordStatus::Done => "done",
    }
}

#[allow(clippy::needless_pass_by_value)]
fn insert_relationship(
    transaction: &rusqlite::Transaction<'_>,
    child: &Record,
    parent: &Record,
    kind: RecordRelationshipKind,
) -> Result<(), RegistrationError> {
    match kind {
        RecordRelationshipKind::ParentChild => {
            if child.record_type != RecordType::Generic
                || !matches!(parent.record_type, RecordType::Generic | RecordType::Task)
            {
                return Err(RegistrationError::new(
                    "invalid-relationship",
                    "only a Generic Record may be attached to a Generic or Task parent after opening",
                ));
            }
            if child.project_slugs != parent.project_slugs {
                return Err(RegistrationError::new(
                    "scope-mismatch",
                    "parent and child Records must have identical Project Scopes",
                ));
            }
            let cycle: bool = transaction.query_row(
                "WITH RECURSIVE descendants(id) AS (SELECT child_id FROM record_relationships WHERE kind = 'parent-child' AND parent_id = ?1 UNION SELECT relationship.child_id FROM record_relationships relationship JOIN descendants ON relationship.parent_id = descendants.id WHERE relationship.kind = 'parent-child') SELECT EXISTS(SELECT 1 FROM descendants WHERE id = ?2)",
                params![child.id.cast_signed(), parent.id.cast_signed()], |row| row.get(0)
            ).map_err(storage_error)?;
            if cycle {
                return Err(RegistrationError::new(
                    "relationship-cycle",
                    "parent/child Relationships cannot form a cycle",
                ));
            }
            transaction.execute("INSERT INTO record_relationships (from_id, to_id, parent_id, child_id, kind) VALUES (?1, ?2, ?2, ?1, 'parent-child')", params![child.id.cast_signed(), parent.id.cast_signed()]).map_err(|error| if matches!(error.sqlite_error_code(), Some(ErrorCode::ConstraintViolation)) { RegistrationError::new("child-already-related", "a child Record can have only one parent") } else { storage_error(error) })?;
        }
        RecordRelationshipKind::DependsOn => {
            if child.status == RecordStatus::Done {
                return Err(RegistrationError::new(
                    "blocked-record-not-open",
                    "a dependency may change only while its blocked Record is Open",
                ));
            }
            if child.status == RecordStatus::InProgress {
                let agent_owned: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM agent_assignments WHERE record_id = ?1 AND closed_at_millis IS NULL)",
                    [child.id.cast_signed()],
                    |row| row.get(0),
                ).map_err(storage_error)?;
                if agent_owned {
                    return Err(RegistrationError::new(
                        "agent-owned-task",
                        "a dependency cannot be added to an agent-owned In Progress Task",
                    ));
                }
            }
            let valid_dependency = child.record_type == parent.record_type
                && matches!(child.record_type, RecordType::Task | RecordType::Spec)
                || child.record_type == RecordType::Task
                    && parent.record_type == RecordType::Grilling
                    && (child.project_slugs.is_empty() && parent.project_slugs.is_empty()
                        || child.project_slugs.len() == 1
                            && parent.project_slugs.contains(&child.project_slugs[0]));
            if !valid_dependency {
                return Err(RegistrationError::new(
                    "invalid-relationship",
                    "depends-on requires same-type Task or Spec Records, or a Task scoped within a Grilling",
                ));
            }
            let cycle: bool = transaction.query_row(
                "WITH RECURSIVE prerequisites(id) AS (SELECT to_id FROM record_relationships WHERE kind = 'depends-on' AND from_id = ?1 UNION SELECT relationship.to_id FROM record_relationships relationship JOIN prerequisites ON relationship.from_id = prerequisites.id WHERE relationship.kind = 'depends-on') SELECT EXISTS(SELECT 1 FROM prerequisites WHERE id = ?2)",
                params![parent.id.cast_signed(), child.id.cast_signed()], |row| row.get(0)
            ).map_err(storage_error)?;
            if cycle {
                return Err(RegistrationError::new(
                    "relationship-cycle",
                    "dependency Relationships cannot form a cycle",
                ));
            }
            transaction.execute("INSERT INTO record_relationships (from_id, to_id, kind) VALUES (?1, ?2, 'depends-on')", params![child.id.cast_signed(), parent.id.cast_signed()]).map_err(|error| if matches!(error.sqlite_error_code(), Some(ErrorCode::ConstraintViolation)) { RegistrationError::new("duplicate-relationship", "Relationship already exists") } else { storage_error(error) })?;
        }
        RecordRelationshipKind::Initiative => {
            record_workspaces::attach_initiative(transaction, child, parent)?;
        }
        RecordRelationshipKind::GeneratedBy => {
            if child.status != RecordStatus::Open {
                return Err(RegistrationError::new(
                    "generated-record-not-open",
                    "provenance may change only while its generated Record is Open",
                ));
            }
            let cycle: bool = transaction.query_row("WITH RECURSIVE generators(id) AS (SELECT to_id FROM record_relationships WHERE kind = 'generated-by' AND from_id = ?1 UNION SELECT relationship.to_id FROM record_relationships relationship JOIN generators ON relationship.from_id = generators.id WHERE relationship.kind = 'generated-by') SELECT EXISTS(SELECT 1 FROM generators WHERE id = ?2)", params![parent.id.cast_signed(), child.id.cast_signed()], |row| row.get(0)).map_err(storage_error)?;
            if cycle {
                return Err(RegistrationError::new(
                    "relationship-cycle",
                    "generated-by Relationships cannot form a cycle",
                ));
            }
            transaction.execute("INSERT INTO record_relationships (from_id, to_id, kind) VALUES (?1, ?2, 'generated-by')", params![child.id.cast_signed(), parent.id.cast_signed()]).map_err(|error| if matches!(error.sqlite_error_code(), Some(ErrorCode::ConstraintViolation)) { RegistrationError::new("duplicate-relationship", "Relationship already exists") } else { storage_error(error) })?;
        }
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn insert_update(
    connection: &Connection,
    record_id: u64,
    kind: RecordUpdateKind,
) -> Result<(), RegistrationError> {
    let (kind, from_status, to_status, related_record_id, comment_id, relationship_kind) =
        match kind {
            RecordUpdateKind::FindingActivity { activity_id } => {
                connection.execute("INSERT INTO record_updates (record_id, created_at_millis, kind, finding_activity_id) VALUES (?1, ?2, 'finding-activity', ?3)", params![record_id.cast_signed(), now_millis()?, activity_id.cast_signed()]).map_err(storage_error)?;
                return Ok(());
            }
            RecordUpdateKind::CheckInvalidated { assessment_id } => {
                connection.execute("INSERT INTO record_updates (record_id, created_at_millis, kind, assessment_id) VALUES (?1, ?2, 'check-invalidated', ?3)", params![record_id.cast_signed(), now_millis()?, assessment_id.cast_signed()]).map_err(storage_error)?;
                return Ok(());
            }
            RecordUpdateKind::CheckAssessed { assessment_id } => {
                connection.execute("INSERT INTO record_updates (record_id, created_at_millis, kind, assessment_id) VALUES (?1, ?2, 'check-assessed', ?3)", params![record_id.cast_signed(), now_millis()?, assessment_id.cast_signed()]).map_err(storage_error)?;
                return Ok(());
            }
            RecordUpdateKind::Created => ("created", None, None, None, None, None),
            RecordUpdateKind::BodyEdited => ("body-edited", None, None, None, None, None),
            RecordUpdateKind::StatusTransition { from, to } => (
                "status-transition",
                Some(status_name(from)),
                Some(status_name(to)),
                None,
                None,
                None,
            ),
            RecordUpdateKind::RelationshipAdded {
                related_record_id,
                relationship_kind,
            } => (
                "relationship-added",
                None,
                None,
                Some(related_record_id.cast_signed()),
                None,
                relationship_kind.map(relationship_kind_name),
            ),
            RecordUpdateKind::RelationshipRemoved {
                related_record_id,
                relationship_kind,
            } => (
                "relationship-removed",
                None,
                None,
                Some(related_record_id.cast_signed()),
                None,
                relationship_kind.map(relationship_kind_name),
            ),
            RecordUpdateKind::CommentAdded { comment_id } => (
                "comment-added",
                None,
                None,
                None,
                Some(comment_id.0.cast_signed()),
                None,
            ),
            RecordUpdateKind::CommentDeleted { comment_id } => (
                "comment-deleted",
                None,
                None,
                None,
                Some(comment_id.0.cast_signed()),
                None,
            ),
        };
    let created_at_millis = now_millis()?;
    connection
        .execute(
            "INSERT INTO record_updates (record_id, created_at_millis, kind, from_status, to_status, related_record_id, comment_id, relationship_kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![record_id.cast_signed(), created_at_millis, kind, from_status, to_status, related_record_id, comment_id, relationship_kind],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn update_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordUpdate> {
    update_from_row_at(row, 0, 1)
}

fn update_from_row_at(
    row: &rusqlite::Row<'_>,
    id_index: usize,
    created_at_index: usize,
) -> rusqlite::Result<RecordUpdate> {
    let kind: String = row.get(created_at_index + 1)?;
    let from_status: Option<String> = row.get(created_at_index + 2)?;
    let to_status: Option<String> = row.get(created_at_index + 3)?;
    let related_record_id: Option<i64> = row.get(created_at_index + 4)?;
    let comment_id: Option<i64> = row.get(created_at_index + 5)?;
    let relationship_kind: Option<String> = row.get(created_at_index + 6)?;
    let status = |value: String| match value.as_str() {
        "open" => Ok(RecordStatus::Open),
        "in_progress" => Ok(RecordStatus::InProgress),
        "done" => Ok(RecordStatus::Done),
        _ => Err(rusqlite::Error::InvalidQuery),
    };
    let kind = match kind.as_str() {
        "finding-activity" => RecordUpdateKind::FindingActivity {
            activity_id: row.get::<_, i64>(created_at_index + 8)?.cast_unsigned(),
        },
        "check-invalidated" => RecordUpdateKind::CheckInvalidated {
            assessment_id: row.get::<_, i64>(created_at_index + 7)?.cast_unsigned(),
        },
        "check-assessed" => RecordUpdateKind::CheckAssessed {
            assessment_id: row.get::<_, i64>(created_at_index + 7)?.cast_unsigned(),
        },
        "created" => RecordUpdateKind::Created,
        "body-edited" => RecordUpdateKind::BodyEdited,
        "status-transition" => RecordUpdateKind::StatusTransition {
            from: status(from_status.ok_or(rusqlite::Error::InvalidQuery)?)?,
            to: status(to_status.ok_or(rusqlite::Error::InvalidQuery)?)?,
        },
        "relationship-added" => RecordUpdateKind::RelationshipAdded {
            related_record_id: related_record_id
                .ok_or(rusqlite::Error::InvalidQuery)?
                .cast_unsigned(),
            relationship_kind: relationship_kind
                .as_deref()
                .map(parse_relationship_kind)
                .transpose()?,
        },
        "relationship-removed" => RecordUpdateKind::RelationshipRemoved {
            related_record_id: related_record_id
                .ok_or(rusqlite::Error::InvalidQuery)?
                .cast_unsigned(),
            relationship_kind: relationship_kind
                .as_deref()
                .map(parse_relationship_kind)
                .transpose()?,
        },
        "comment-added" => RecordUpdateKind::CommentAdded {
            comment_id: RecordCommentId(
                comment_id
                    .ok_or(rusqlite::Error::InvalidQuery)?
                    .cast_unsigned(),
            ),
        },
        "comment-deleted" => RecordUpdateKind::CommentDeleted {
            comment_id: RecordCommentId(
                comment_id
                    .ok_or(rusqlite::Error::InvalidQuery)?
                    .cast_unsigned(),
            ),
        },
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(RecordUpdate {
        id: RecordUpdateId(row.get::<_, i64>(id_index)?.cast_unsigned()),
        created_at_millis: row.get::<_, i64>(created_at_index)?.cast_unsigned(),
        kind,
    })
}

fn unfinished_prerequisite_ids(
    connection: &Connection,
    id: u64,
) -> Result<Vec<u64>, RegistrationError> {
    connection
        .prepare(
            "SELECT prerequisite.id FROM record_relationships dependency JOIN records prerequisite ON prerequisite.id = dependency.to_id WHERE dependency.from_id = ?1 AND dependency.kind = 'depends-on' AND prerequisite.status != 'done' ORDER BY prerequisite.id",
        )
        .map_err(storage_error)?
        .query_map([id.cast_signed()], |row| row.get::<_, i64>(0).map(i64::cast_unsigned))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)
}

fn dependencies_not_done_error(ids: &[u64]) -> RegistrationError {
    RegistrationError::new(
        "dependencies-not-done",
        format!(
            "direct prerequisite Records are not Done: {}",
            ids.iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    )
}

fn record_or_transition_error(
    connection: &Connection,
    id: u64,
) -> Result<Record, RegistrationError> {
    match load_record(connection, id) {
        Ok(_) => Err(RegistrationError::new(
            "invalid-status-transition",
            "record status can only advance from open to in-progress to done",
        )),
        Err(error) => Err(error),
    }
}

fn validate_projects(
    connection: &Connection,
    project_slugs: &[String],
) -> Result<(), RegistrationError> {
    let mut workspace = None;
    for slug in project_slugs {
        let owner: String = connection
            .query_row(
                "SELECT workspace_id FROM projects WHERE slug = ?1",
                [slug],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or_else(|| {
                RegistrationError::new(
                    "project-not-found",
                    format!("Project `{slug}` is not registered"),
                )
            })?;
        if workspace
            .as_ref()
            .is_some_and(|workspace| workspace != &owner)
        {
            return Err(RegistrationError::new(
                "cross-workspace-record",
                "all Projects in a Record's scope must belong to one Workspace",
            ));
        }
        workspace = Some(owner);
    }
    Ok(())
}

fn add_generated_by_relationships(
    connection: &Connection,
    generated_id: u64,
    generator_ids: &[u64],
) -> Result<(), RegistrationError> {
    let mut generators = generator_ids.to_vec();
    generators.sort_unstable();
    if generators.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(RegistrationError::new(
            "duplicate-relationship",
            "generated-by Relationships must not be duplicated",
        ));
    }
    for generator_id in generators {
        if generator_id == generated_id {
            return Err(RegistrationError::new(
                "invalid-relationship",
                "a Record cannot be generated by itself",
            ));
        }
        load_record(connection, generator_id)?;
        record_workspaces::same_workspace(connection, generated_id, generator_id)?;
        connection
            .execute(
                "INSERT INTO record_relationships (from_id, to_id, kind) VALUES (?1, ?2, 'generated-by')",
                params![generated_id.cast_signed(), generator_id.cast_signed()],
            )
            .map_err(storage_error)?;
        insert_update(
            connection,
            generated_id,
            RecordUpdateKind::RelationshipAdded {
                related_record_id: generator_id,
                relationship_kind: Some(RecordRelationshipKind::GeneratedBy),
            },
        )?;
        insert_update(
            connection,
            generator_id,
            RecordUpdateKind::RelationshipAdded {
                related_record_id: generated_id,
                relationship_kind: Some(RecordRelationshipKind::GeneratedBy),
            },
        )?;
    }
    Ok(())
}

fn validate_typed_record(record: &NewTypedRecord) -> Result<(), RegistrationError> {
    if record.record_type != RecordType::Task && record.task_kind.is_some() {
        return Err(RegistrationError::new(
            "invalid-task-kind",
            "Task Kind may be selected only for a Task",
        ));
    }
    match record.record_type {
        RecordType::Generic => Err(RegistrationError::new(
            "invalid-record-type",
            "typed Record opening requires a non-Generic type",
        )),
        RecordType::Spec if record.project_slugs.len() != 1 => Err(RegistrationError::new(
            "invalid-project-scope",
            "a Spec must have exactly one Project in its scope",
        )),
        RecordType::Task if record.project_slugs.len() > 1 => Err(RegistrationError::new(
            "invalid-project-scope",
            "a standalone Task may have at most one Project in its scope",
        )),
        RecordType::Spec
        | RecordType::Task
        | RecordType::Wayfinder
        | RecordType::Grilling
        | RecordType::Initiative => Ok(()),
    }
}

fn record_type_name(record_type: RecordType) -> &'static str {
    match record_type {
        RecordType::Generic => "generic",
        RecordType::Spec => "spec",
        RecordType::Task => "task",
        RecordType::Wayfinder => "wayfinder",
        RecordType::Grilling => "grilling",
        RecordType::Initiative => "initiative",
    }
}

fn task_kind_name(task_kind: Option<TaskKind>) -> Option<&'static str> {
    task_kind.map(|task_kind| match task_kind {
        TaskKind::Research => "research",
        TaskKind::Prototype => "prototype",
        TaskKind::Review => "review",
    })
}

fn task_kind(value: Option<String>) -> Result<Option<TaskKind>, RegistrationError> {
    value
        .map(|value| match value.as_str() {
            "research" => Ok(TaskKind::Research),
            "prototype" => Ok(TaskKind::Prototype),
            "review" => Ok(TaskKind::Review),
            _ => Err(RegistrationError::new(
                "storage",
                "stored Task has an invalid kind",
            )),
        })
        .transpose()
}

fn record_type(value: &str) -> Result<RecordType, RegistrationError> {
    match value {
        "generic" => Ok(RecordType::Generic),
        "spec" => Ok(RecordType::Spec),
        "task" => Ok(RecordType::Task),
        "wayfinder" => Ok(RecordType::Wayfinder),
        "grilling" => Ok(RecordType::Grilling),
        "initiative" => Ok(RecordType::Initiative),
        _ => Err(RegistrationError::new(
            "storage",
            "stored Record has an invalid type",
        )),
    }
}

fn load_record(connection: &Connection, id: u64) -> Result<Record, RegistrationError> {
    let (stored_record_type, stored_task_kind, body, status, completion_resolution) = connection
        .query_row(
            "SELECT record_type, task_kind, body, status, completion_resolution FROM records WHERE id = ?1",
            [id.cast_signed()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => {
                RegistrationError::new("record-not-found", format!("Record {id} does not exist"))
            }
            error => storage_error(error),
        })?;
    let project_slugs = connection
        .prepare(
            "SELECT project_slug FROM record_projects WHERE record_id = ?1 ORDER BY project_slug",
        )
        .map_err(storage_error)?
        .query_map([id.cast_signed()], |row| row.get(0))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let status = match status.as_str() {
        "open" => RecordStatus::Open,
        "in_progress" => RecordStatus::InProgress,
        "done" => RecordStatus::Done,
        _ => {
            return Err(RegistrationError::new(
                "storage",
                "stored Record has an invalid status",
            ));
        }
    };
    let relationships = connection
        .prepare("SELECT from_id, to_id, parent_id, child_id, kind FROM record_relationships WHERE from_id = ?1 OR to_id = ?1 ORDER BY from_id, to_id, kind")
        .map_err(storage_error)?
        .query_map([id.cast_signed()], |row| {
            let kind: String = row.get(4)?;
            Ok((row.get::<_, i64>(0)?.cast_unsigned(), row.get::<_, i64>(1)?.cast_unsigned(), row.get::<_, Option<i64>>(2)?.map(i64::cast_unsigned), row.get::<_, Option<i64>>(3)?.map(i64::cast_unsigned), kind))
        })
        .map_err(storage_error)?
        .map(|row| match row.map_err(storage_error)? {
            (from_id, to_id, Some(parent_id), Some(child_id), kind) if kind == "parent-child" => Ok(RecordRelationship { kind: RecordRelationshipKind::ParentChild, from_id, to_id, parent_id, child_id }),
            (from_id, to_id, None, None, kind) if kind == "depends-on" => Ok(RecordRelationship { kind: RecordRelationshipKind::DependsOn, from_id, to_id, parent_id: 0, child_id: 0 }),
            (from_id, to_id, None, None, kind) if kind == "generated-by" => Ok(RecordRelationship { kind: RecordRelationshipKind::GeneratedBy, from_id, to_id, parent_id: 0, child_id: 0 }),
            (from_id, to_id, None, None, kind) if kind == "initiative" => Ok(RecordRelationship { kind: RecordRelationshipKind::Initiative, from_id, to_id, parent_id: 0, child_id: 0 }),
            _ => Err(RegistrationError::new("storage", "stored Record has an invalid Relationship kind")),
        })
        .collect::<Result<Vec<_>, RegistrationError>>()?;
    Ok(Record {
        workspace_id: record_workspaces::owner(connection, id)?,
        id,
        record_type: record_type(&stored_record_type)?,
        task_kind: task_kind(stored_task_kind)?,
        body,
        project_slugs,
        status,
        completion_resolution,
        relationships,
    })
}

fn now_millis() -> Result<i64, RegistrationError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| RegistrationError::new("storage", "system clock is before Unix epoch"))?
        .as_millis()
        .try_into()
        .map_err(|_| RegistrationError::new("storage", "system clock is out of range"))
}

fn migrate_record_type_schema(connection: &Connection) -> Result<(), RegistrationError> {
    let columns = connection
        .prepare("PRAGMA table_info(records)")
        .map_err(storage_error)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns.iter().any(|column| column == "record_type") {
        connection
            .execute_batch(
                "ALTER TABLE records ADD COLUMN record_type TEXT NOT NULL DEFAULT 'generic' CHECK (record_type IN ('generic', 'spec', 'task', 'wayfinder', 'grilling', 'initiative'));",
            )
            .map_err(storage_error)?;
        return Ok(());
    }

    let schema: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'records'",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if schema.contains("'initiative'") {
        return Ok(());
    }

    if !columns.iter().any(|column| column == "workspace_id") {
        connection
            .execute_batch(
                "ALTER TABLE records ADD COLUMN workspace_id TEXT REFERENCES workspaces(id);",
            )
            .map_err(storage_error)?;
    }
    if !columns.iter().any(|column| column == "task_kind") {
        connection
            .execute_batch("ALTER TABLE records ADD COLUMN task_kind TEXT;")
            .map_err(storage_error)?;
    }
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(storage_error)?;
    let migration = (|| {
        let transaction = connection.unchecked_transaction().map_err(storage_error)?;
        transaction
            .execute_batch(
                "DROP TRIGGER IF EXISTS record_project_workspace;
                 DROP TRIGGER IF EXISTS record_relationship_workspace;
                 CREATE TABLE records_new (
                    id INTEGER PRIMARY KEY NOT NULL,
                    record_type TEXT NOT NULL CHECK (record_type IN ('generic', 'spec', 'task', 'wayfinder', 'grilling', 'initiative')),
                    workspace_id TEXT REFERENCES workspaces(id) ON DELETE RESTRICT,
                    task_kind TEXT CHECK (task_kind IN ('research', 'prototype', 'review')),
                    body TEXT NOT NULL,
                    status TEXT NOT NULL CHECK (status IN ('open', 'in_progress', 'done')),
                    completion_resolution TEXT
                );
                INSERT INTO records_new (id, record_type, body, status, completion_resolution, workspace_id, task_kind)
                    SELECT id, record_type, body, status, completion_resolution, workspace_id, task_kind FROM records;
                DROP TABLE records;
                ALTER TABLE records_new RENAME TO records;",
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)
    })();
    let foreign_keys = connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(storage_error);
    migration.and(foreign_keys)
}

fn migrate_task_kind_schema(connection: &Connection) -> Result<(), RegistrationError> {
    let columns = connection
        .prepare("PRAGMA table_info(records)")
        .map_err(storage_error)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns.iter().any(|column| column == "task_kind") {
        connection
            .execute_batch(
                "ALTER TABLE records ADD COLUMN task_kind TEXT CHECK (task_kind IN ('research', 'prototype', 'review'));",
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

fn migrate_record_relationship_schema(connection: &Connection) -> Result<(), RegistrationError> {
    let schema: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'record_relationships'",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if schema.contains("from_id") && schema.contains("'initiative'") {
        return Ok(());
    }
    if schema.contains("from_id") {
        connection.execute_batch("PRAGMA foreign_keys = OFF; ALTER TABLE record_relationships RENAME TO record_relationships_old; CREATE TABLE record_relationships (from_id INTEGER NOT NULL REFERENCES records(id), to_id INTEGER NOT NULL REFERENCES records(id), parent_id INTEGER REFERENCES records(id), child_id INTEGER REFERENCES records(id) UNIQUE, kind TEXT NOT NULL CHECK (kind IN ('parent-child', 'depends-on', 'generated-by', 'initiative')), PRIMARY KEY (from_id, to_id, kind)); INSERT INTO record_relationships SELECT from_id, to_id, parent_id, child_id, kind FROM record_relationships_old; DROP TABLE record_relationships_old; PRAGMA foreign_keys = ON;").map_err(storage_error)?;
        return Ok(());
    }
    connection.execute_batch("PRAGMA foreign_keys = OFF;
        ALTER TABLE record_relationships RENAME TO record_relationships_old;
        CREATE TABLE record_relationships (
            from_id INTEGER NOT NULL REFERENCES records(id), to_id INTEGER NOT NULL REFERENCES records(id),
            parent_id INTEGER REFERENCES records(id), child_id INTEGER REFERENCES records(id) UNIQUE,
            kind TEXT NOT NULL CHECK (kind IN ('parent-child', 'depends-on', 'generated-by', 'initiative')), PRIMARY KEY (from_id, to_id, kind)
        );
        INSERT INTO record_relationships (from_id, to_id, parent_id, child_id, kind)
            SELECT child_id, parent_id, parent_id, child_id, kind FROM record_relationships_old;
        DROP TABLE record_relationships_old;
        PRAGMA foreign_keys = ON;").map_err(storage_error)
}

fn migrate_record_updates_schema(connection: &Connection) -> Result<(), RegistrationError> {
    let schema: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'record_updates'",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if !schema.contains("comment-added") {
        connection.execute_batch("PRAGMA foreign_keys = OFF; CREATE TABLE record_updates_new (id INTEGER PRIMARY KEY NOT NULL, record_id INTEGER NOT NULL REFERENCES records(id), created_at_millis INTEGER NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('created', 'body-edited', 'status-transition', 'relationship-added', 'relationship-removed', 'comment-added', 'comment-deleted')), from_status TEXT, to_status TEXT, related_record_id INTEGER REFERENCES records(id), comment_id INTEGER); INSERT INTO record_updates_new (id, record_id, created_at_millis, kind, from_status, to_status, related_record_id) SELECT id, record_id, created_at_millis, kind, from_status, to_status, related_record_id FROM record_updates; DROP TABLE record_updates; ALTER TABLE record_updates_new RENAME TO record_updates; PRAGMA foreign_keys = ON;").map_err(storage_error)?;
    }
    let columns = connection
        .prepare("PRAGMA table_info(record_updates)")
        .map_err(storage_error)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns.iter().any(|column| column == "relationship_kind") {
        connection.execute_batch("ALTER TABLE record_updates ADD COLUMN relationship_kind TEXT CHECK (relationship_kind IN ('parent-child', 'depends-on', 'generated-by'));").map_err(storage_error)?;
    }
    Ok(())
}

fn migrate_record_status_schema(connection: &Connection) -> Result<(), RegistrationError> {
    let schema: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'records'",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if schema.contains("status = 'open'") {
        connection
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                CREATE TABLE records_new (
                    id INTEGER PRIMARY KEY NOT NULL,
                    body TEXT NOT NULL,
                    status TEXT NOT NULL CHECK (status IN ('open', 'in_progress', 'done')),
                    completion_resolution TEXT
                );
                INSERT INTO records_new (id, body, status) SELECT id, body, status FROM records;
                DROP TABLE records;
                ALTER TABLE records_new RENAME TO records;
                PRAGMA foreign_keys = ON;",
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn storage_error(error: rusqlite::Error) -> RegistrationError {
    RegistrationError::new("storage", format!("could not persist Forge data: {error}"))
}

#[cfg(test)]
mod tests {
    mod finding_tests {
        include!("finding_tests.rs");
    }
    mod check_tests {
        include!("check_tests.rs");
    }
    mod check_template_tests {
        include!("check_template_tests.rs");
    }
    mod check_invalidation_tests {
        include!("check_invalidation_tests.rs");
    }
    mod check_completion_tests {
        include!("check_completion_tests.rs");
    }
    include!("subscription_removal_tests.rs");
    use super::SqliteProjectRepository as Repository;
    use forge_app::{RegistrationError, WorkspaceRepository};

    // Fixture factories return the real repository; every test Project has an
    // explicit owner. Short persistence identities keep unrelated record tests readable.
    struct SqliteProjectRepository;
    impl SqliteProjectRepository {
        fn in_memory() -> Result<Repository, RegistrationError> {
            Self::seed(&Repository::in_memory()?)
        }
        fn open(path: &std::path::Path) -> Result<Repository, RegistrationError> {
            Self::seed(&Repository::open(path)?)
        }
        fn seed(repository: &Repository) -> Result<Repository, RegistrationError> {
            if repository.workspace("test").is_err() {
                repository.create_workspace("test", "Test", std::path::Path::new("/"))?;
                for kind in [forge_app::RecordType::Task, forge_app::RecordType::Grilling] {
                    forge_app::WorkspaceDispatchRepository::select_workspace_profile(
                        repository,
                        "test",
                        kind,
                        Some(1),
                    )?;
                }
                forge_app::WorkspaceDispatchRepository::retain_verified_workspace_binding(
                    repository,
                    "test",
                    Some(&forge_app::DispatchWorkspace {
                        id: "test-provider".into(),
                    }),
                    0,
                )?;
            }
            repository.in_record_workspace("test")
        }
    }
    use forge_app::{
        AgentDispatchRepository, AssignmentAuthorizationOperation, ManagedWorktreeRepository,
        NewAgentAssignment, NewAgentAssignmentResource, NewRecord, NewTypedRecord, Project,
        ProjectRepository, Record, RecordListFilter, RecordRelationshipKind, RecordRepository,
        RecordStatus, RecordType, RecordUpdateKind, TaskKind, WorkspaceDispatchRepository,
        assignment_acknowledge, assignment_add_comment, assignment_complete_task,
        assignment_context, assignment_start_task,
    };
    use std::{
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };
    fn project(slug: &str, folder: &str) -> Project {
        let root = std::env::temp_dir()
            .join(format!("forge-store-projects-{}", std::process::id()))
            .join(folder.trim_start_matches('/'));
        std::fs::create_dir_all(&root).unwrap();
        Project {
            reference: slug.into(),
            workspace_id: "test".into(),
            slug: slug.into(),
            project_root: root.canonicalize().unwrap(),
            origin_url: "https://example.test/repo.git".into(),
        }
    }

    #[test]
    fn task_start_waits_for_competing_resume_without_losing_the_transition() {
        use std::{sync::mpsc, time::Duration};

        let path = std::env::temp_dir().join(format!(
            "forge-start-resume-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let starter = SqliteProjectRepository::open(&path).unwrap();
        starter.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = starter
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "start while another connection clears the hold".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        starter
            .hold_dispatch(task.id, "acknowledgement timeout")
            .unwrap();
        let resumer = SqliteProjectRepository::open(&path).unwrap();
        let (reserved_tx, reserved_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        // Test-only scheduling: stop the real DELETE after it obtains a writer
        // reservation but before commit. No production operation is mocked/retried.
        resumer
            .connection
            .lock()
            .unwrap()
            .update_hook(Some(move |action, _: &str, table: &str, _: i64| {
                if action == rusqlite::hooks::Action::SQLITE_DELETE && table == "dispatch_holds" {
                    reserved_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                }
            }))
            .unwrap();
        let result = std::thread::scope(|scope| {
            let resume = scope.spawn(|| resumer.resume_dispatch(task.id));
            reserved_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let (started_tx, started_rx) = mpsc::channel();
            let starter = &starter;
            let start = scope.spawn(move || {
                started_tx.send(starter.start_record(task.id)).unwrap();
            });
            // A deferred read-to-write upgrade fails immediately here, despite
            // SQLite's busy timeout. A safe start waits for the competing writer.
            let early = started_rx.recv_timeout(Duration::from_millis(100));
            release_tx.send(()).unwrap();
            resume.join().unwrap().unwrap();
            let result = match early {
                Ok(result) => result,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    started_rx.recv_timeout(Duration::from_secs(5)).unwrap()
                }
                Err(error) => panic!("Task start disconnected: {error}"),
            };
            start.join().unwrap();
            result
        });
        drop(resumer);
        let reopened = SqliteProjectRepository::open(&path).unwrap();
        let status = reopened.show_record(task.id).unwrap().status;
        let hold = reopened.dispatch_hold(task.id).unwrap();
        let transitions = reopened
            .list_record_updates(task.id)
            .unwrap()
            .iter()
            .filter(|update| {
                matches!(
                    update.kind,
                    RecordUpdateKind::StatusTransition {
                        to: RecordStatus::InProgress,
                        ..
                    }
                )
            })
            .count();
        drop(reopened);
        drop(starter);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(result.unwrap().status, RecordStatus::InProgress);
        assert_eq!(status, RecordStatus::InProgress);
        assert!(hold.is_none());
        assert_eq!(transitions, 1);
    }

    #[test]
    fn task_start_exhausts_its_busy_timeout_without_partial_lifecycle_writes() {
        use std::time::Duration;

        let path = std::env::temp_dir().join(format!(
            "forge-start-timeout-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteProjectRepository::open(&path).unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "bounded contention".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        // Shorten only the test connection's wait, not the production policy.
        store
            .connection
            .lock()
            .unwrap()
            .busy_timeout(Duration::from_millis(30))
            .unwrap();
        let mut competing = rusqlite::Connection::open(&path).unwrap();
        let reserved = competing
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        let before = std::time::Instant::now();
        let error = store.start_record(task.id).unwrap_err();
        assert_eq!(error.code, "storage");
        assert!(error.message.contains("database is locked"));
        assert!(before.elapsed() >= Duration::from_millis(20));
        assert_eq!(
            store.show_record(task.id).unwrap().status,
            RecordStatus::Open
        );
        assert_eq!(store.list_record_updates(task.id).unwrap().len(), 1);
        reserved.rollback().unwrap();
        assert_eq!(
            store.start_record(task.id).unwrap().status,
            RecordStatus::InProgress
        );
        assert_eq!(store.list_record_updates(task.id).unwrap().len(), 2);
        drop(competing);
        drop(store);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn atomically_opens_and_corrects_generated_by_provenance() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let generator = store
            .open_record(&NewRecord {
                body: "generator".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        let generated = store
            .open_record(&NewRecord {
                body: "generated".into(),
                project_slugs: vec![],
                generated_by_ids: vec![generator.id],
            })
            .unwrap();
        assert!(generated.relationships.iter().any(|relationship| {
            relationship.kind == RecordRelationshipKind::GeneratedBy
                && relationship.from_id == generated.id
                && relationship.to_id == generator.id
                && relationship.parent_id == 0
                && relationship.child_id == 0
        }));
        assert!(matches!(
            store.list_record_updates(generated.id).unwrap()[0].kind,
            RecordUpdateKind::RelationshipAdded {
                related_record_id,
                relationship_kind: Some(RecordRelationshipKind::GeneratedBy),
            } if related_record_id == generator.id
        ));
        assert_eq!(
            store
                .relate_records(
                    generator.id,
                    generated.id,
                    RecordRelationshipKind::GeneratedBy
                )
                .unwrap_err()
                .code,
            "relationship-cycle"
        );
        store.start_record(generated.id).unwrap();
        assert_eq!(
            store
                .unrelate_records(
                    generated.id,
                    generator.id,
                    RecordRelationshipKind::GeneratedBy
                )
                .unwrap_err()
                .code,
            "generated-record-not-open"
        );
    }

    #[test]
    fn persists_directional_task_dependencies_and_notifies_both_endpoints() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = |body: &str| NewTypedRecord {
            record_type: RecordType::Task,
            task_kind: None,
            body: body.into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        };
        let blocked = store.open_typed_record(&task("blocked")).unwrap();
        let prerequisite = store.open_typed_record(&task("prerequisite")).unwrap();
        let other = store.open_typed_record(&task("other")).unwrap();
        let later = store.open_typed_record(&task("later")).unwrap();

        let related = store
            .relate_records(
                blocked.id,
                prerequisite.id,
                RecordRelationshipKind::DependsOn,
            )
            .unwrap();
        assert!(related.relationships.iter().any(|relationship| matches!(
            relationship.kind,
            RecordRelationshipKind::DependsOn
        ) && relationship.from_id
            == blocked.id
            && relationship.to_id == prerequisite.id));
        assert!(
            matches!(store.list_record_updates(prerequisite.id).unwrap()[0].kind, RecordUpdateKind::RelationshipAdded { related_record_id, .. } if related_record_id == blocked.id)
        );
        assert_eq!(
            store
                .relate_records(
                    blocked.id,
                    prerequisite.id,
                    RecordRelationshipKind::DependsOn
                )
                .unwrap_err()
                .code,
            "duplicate-relationship"
        );
        assert_eq!(
            store
                .relate_records(
                    prerequisite.id,
                    blocked.id,
                    RecordRelationshipKind::DependsOn
                )
                .unwrap_err()
                .code,
            "relationship-cycle"
        );
        store.start_record(prerequisite.id).unwrap();
        check_tests::assess(&store, prerequisite.id, forge_app::checks::Verdict::Pass);
        store.done_record(prerequisite.id, None).unwrap();
        store.start_record(blocked.id).unwrap();
        assert_eq!(
            store
                .unrelate_records(
                    blocked.id,
                    prerequisite.id,
                    RecordRelationshipKind::DependsOn
                )
                .unwrap_err()
                .code,
            "blocked-record-not-open"
        );
        store
            .relate_records(blocked.id, other.id, RecordRelationshipKind::DependsOn)
            .unwrap();
        let assigned = store
            .create_agent_assignment(&NewAgentAssignment {
                id: "assigned".into(),
                record_id: blocked.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        assert_eq!(
            store
                .relate_records(blocked.id, later.id, RecordRelationshipKind::DependsOn)
                .unwrap_err()
                .code,
            "agent-owned-task"
        );
        assert_eq!(assigned.record_id, blocked.id);
    }

    #[test]
    fn dependencies_gate_starts_until_each_direct_prerequisite_is_done() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let task = |body: &str| NewTypedRecord {
            record_type: RecordType::Task,
            task_kind: None,
            body: body.into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        };
        let blocked = store.open_typed_record(&task("blocked")).unwrap();
        let first = store.open_typed_record(&task("first")).unwrap();
        let second = store.open_typed_record(&task("second")).unwrap();
        store
            .relate_records(blocked.id, second.id, RecordRelationshipKind::DependsOn)
            .unwrap();
        store
            .relate_records(blocked.id, first.id, RecordRelationshipKind::DependsOn)
            .unwrap();

        let error = store.start_record(blocked.id).unwrap_err();
        assert_eq!(error.code, "dependencies-not-done");
        assert_eq!(
            error.message,
            format!(
                "direct prerequisite Records are not Done: {}, {}",
                first.id, second.id
            )
        );
        store.start_record(first.id).unwrap();
        check_tests::assess(&store, first.id, forge_app::checks::Verdict::Pass);
        store.done_record(first.id, None).unwrap();
        assert_eq!(
            store.start_record(blocked.id).unwrap_err().code,
            "dependencies-not-done"
        );
        store.start_record(second.id).unwrap();
        check_tests::assess(&store, second.id, forge_app::checks::Verdict::Pass);
        store.done_record(second.id, None).unwrap();
        assert_eq!(
            store.start_record(blocked.id).unwrap().status,
            RecordStatus::InProgress
        );
    }

    #[test]
    fn finds_a_project_from_its_project_root_or_descendant() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let alpha = project("alpha", "/repos/alpha");
        store.register(&alpha).unwrap();
        assert_eq!(store.find_containing(&alpha.project_root).unwrap(), alpha);
        assert_eq!(
            store
                .find_containing(&alpha.project_root.join("src/module"))
                .unwrap()
                .slug,
            "alpha"
        );
        assert_eq!(
            store
                .find_containing(Path::new("/repos/outside"))
                .unwrap_err()
                .code,
            "project-not-found"
        );
    }
    #[test]
    fn stores_idempotently_and_rejects_identity_conflicts() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let alpha = project("alpha", "/repos/alpha");
        assert_eq!(store.register(&alpha).unwrap(), alpha);
        assert_eq!(store.register(&alpha).unwrap(), alpha);
        assert_eq!(
            store
                .register(&project("alpha", "/repos/other"))
                .unwrap_err()
                .code,
            "slug-conflict"
        );
        assert_eq!(
            store
                .register(&project("other", "/repos/alpha"))
                .unwrap_err()
                .code,
            "folder-conflict"
        );
        let nested = project("nested", "/repos/alpha/tools");
        assert_eq!(store.register(&nested).unwrap(), nested);
    }
    #[test]
    fn lists_records_by_scope_membership_status_and_descending_id() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        store.register(&project("beta", "/repos/beta")).unwrap();
        let alpha = store
            .open_record(&NewRecord {
                body: "alpha".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let both = store
            .open_record(&NewRecord {
                body: "both".into(),
                project_slugs: vec!["alpha".into(), "beta".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let unscoped = store
            .open_record(&NewRecord {
                body: "unscoped".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        store.start_record(both.id).unwrap();

        assert_eq!(
            store
                .list_records(&RecordListFilter {
                    project_slug: Some("alpha".into()),
                    status: None,
                    record_type: None,
                    task_kind: None,
                })
                .unwrap()
                .into_iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec![both.id, alpha.id]
        );
        assert_eq!(
            store
                .list_records(&RecordListFilter {
                    project_slug: None,
                    status: Some(RecordStatus::Open),
                    record_type: None,
                    task_kind: None,
                })
                .unwrap()
                .into_iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec![unscoped.id, alpha.id]
        );
    }

    #[test]
    fn persists_content_changes_and_strict_lifecycle() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let record = store
            .open_record(&NewRecord {
                body: "original".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        assert_eq!(
            store.edit_record(record.id, "revised").unwrap().body,
            "revised"
        );
        assert_eq!(
            store.done_record(record.id, None).unwrap_err().code,
            "invalid-status-transition"
        );
        assert_eq!(
            store.start_record(record.id).unwrap().status,
            RecordStatus::InProgress
        );
        let done = store.done_record(record.id, Some("finished")).unwrap();
        assert_eq!(done.completion_resolution.as_deref(), Some("finished"));
        assert_eq!(store.show_record(record.id).unwrap(), done);
        assert_eq!(
            store.edit_record(record.id, "later").unwrap_err().code,
            "invalid-status-transition"
        );
    }

    #[test]
    fn persists_hierarchies_and_enforces_their_rules() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let parent = store
            .open_record(&NewRecord {
                body: "parent".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        let child = store
            .open_record(&NewRecord {
                body: "child".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        let grandchild = store
            .open_record(&NewRecord {
                body: "grandchild".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        assert_eq!(
            store
                .relate_records(child.id, parent.id, RecordRelationshipKind::ParentChild)
                .unwrap()
                .relationships
                .len(),
            1
        );
        store
            .relate_records(grandchild.id, child.id, RecordRelationshipKind::ParentChild)
            .unwrap();
        assert_eq!(
            store
                .relate_records(
                    parent.id,
                    grandchild.id,
                    RecordRelationshipKind::ParentChild
                )
                .unwrap_err()
                .code,
            "relationship-cycle"
        );
        store.start_record(parent.id).unwrap();
        assert_eq!(
            store.done_record(parent.id, None).unwrap_err().code,
            "children-not-done"
        );
        for record_id in [grandchild.id, child.id] {
            store.start_record(record_id).unwrap();
            store.done_record(record_id, None).unwrap();
        }
        assert_eq!(
            store.done_record(parent.id, None).unwrap().status,
            RecordStatus::Done
        );
        assert_eq!(
            store
                .unrelate_records(child.id, parent.id, RecordRelationshipKind::ParentChild)
                .unwrap()
                .relationships
                .len(),
            1
        );
    }

    #[test]
    fn records_minimal_updates_for_each_successful_record_change() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let parent = store
            .open_record(&NewRecord {
                body: "parent".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        let child = store
            .open_record(&NewRecord {
                body: "child".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        assert_eq!(
            store.done_record(child.id, None).unwrap_err().code,
            "invalid-status-transition"
        );
        assert_eq!(store.list_record_updates(child.id).unwrap().len(), 1);
        store.edit_record(child.id, "edited").unwrap();
        store.start_record(child.id).unwrap();
        store.done_record(child.id, None).unwrap();
        store
            .relate_records(child.id, parent.id, RecordRelationshipKind::ParentChild)
            .unwrap();
        store
            .unrelate_records(child.id, parent.id, RecordRelationshipKind::ParentChild)
            .unwrap();

        let updates = store.list_record_updates(child.id).unwrap();
        assert_eq!(updates.len(), 6);
        assert!(
            updates.windows(2).all(|pair| pair[0].id > pair[1].id
                || pair[0].created_at_millis > pair[1].created_at_millis)
        );
        assert!(
            matches!(updates[0].kind, RecordUpdateKind::RelationshipRemoved { related_record_id, .. } if related_record_id == parent.id)
        );
        assert!(
            matches!(updates[1].kind, RecordUpdateKind::RelationshipAdded { related_record_id, .. } if related_record_id == parent.id)
        );
        assert!(matches!(
            updates[2].kind,
            RecordUpdateKind::StatusTransition {
                from: RecordStatus::InProgress,
                to: RecordStatus::Done
            }
        ));
        assert!(matches!(
            updates[3].kind,
            RecordUpdateKind::StatusTransition {
                from: RecordStatus::Open,
                to: RecordStatus::InProgress
            }
        ));
        assert!(matches!(updates[4].kind, RecordUpdateKind::BodyEdited));
        assert!(matches!(updates[5].kind, RecordUpdateKind::Created));
        assert_eq!(
            store.list_record_updates(999).unwrap_err().code,
            "record-not-found"
        );
    }

    #[test]
    fn comments_are_immutable_newest_first_and_record_minimal_updates() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let record = store
            .open_record(&NewRecord {
                body: "record".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        let first = store.add_record_comment(record.id, "first", None).unwrap();
        let second = store
            .add_record_comment(record.id, "second", Some("agent"))
            .unwrap();
        assert_eq!(
            store.list_record_comments(record.id).unwrap(),
            vec![second.clone(), first.clone()]
        );
        store.start_record(record.id).unwrap();
        store.done_record(record.id, None).unwrap();
        let after_done = store.add_record_comment(record.id, "after", None).unwrap();
        store.delete_record_comment(record.id, second.id).unwrap();
        assert_eq!(
            store.list_record_comments(record.id).unwrap(),
            vec![after_done.clone(), first]
        );
        assert_eq!(
            store
                .delete_record_comment(record.id, second.id)
                .unwrap_err()
                .code,
            "comment-not-found"
        );
        let updates = store.list_record_updates(record.id).unwrap();
        assert!(
            matches!(updates[0].kind, RecordUpdateKind::CommentDeleted { comment_id } if comment_id == second.id)
        );
        assert!(
            matches!(updates[1].kind, RecordUpdateKind::CommentAdded { comment_id } if comment_id == after_done.id)
        );
        assert!(
            matches!(updates[4].kind, RecordUpdateKind::CommentAdded { comment_id } if comment_id == second.id)
        );
        assert!(
            matches!(updates[5].kind, RecordUpdateKind::CommentAdded { comment_id } if comment_id.0 == 1)
        );
    }

    #[test]
    fn persists_scoped_and_unscoped_open_records() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        assert_eq!(
            store
                .open_record(&NewRecord {
                    body: "unscoped".into(),
                    project_slugs: vec![],
                    generated_by_ids: vec![],
                })
                .unwrap()
                .status,
            RecordStatus::Open
        );
        let record = store
            .open_record(&NewRecord {
                body: "scoped".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        assert_eq!(record.project_slugs, ["alpha"]);
        assert_eq!(
            store
                .open_record(&NewRecord {
                    body: "bad".into(),
                    project_slugs: vec!["missing".into()],
                    generated_by_ids: vec![],
                })
                .unwrap_err()
                .code,
            "project-not-found"
        );
    }

    #[test]
    fn persists_typed_records_filters_them_and_opens_tasks_atomically() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let spec = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Spec,
                task_kind: None,
                body: "spec".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let task = store
            .open_task_under_spec("task", spec.id, Some(TaskKind::Research), vec![])
            .unwrap();
        assert_eq!(task.record_type, RecordType::Task);
        assert_eq!(task.task_kind, Some(TaskKind::Research));
        assert_eq!(task.project_slugs, ["alpha"]);
        assert_eq!(task.relationships[0].parent_id, spec.id);
        assert_eq!(
            store
                .list_records(&RecordListFilter {
                    project_slug: None,
                    status: None,
                    record_type: Some(RecordType::Task),
                    task_kind: None,
                })
                .unwrap(),
            vec![task.clone()]
        );
        assert_eq!(
            store
                .list_records(&RecordListFilter {
                    project_slug: None,
                    status: None,
                    record_type: None,
                    task_kind: Some(TaskKind::Research),
                })
                .unwrap(),
            vec![task.clone()]
        );
        assert_eq!(
            store
                .open_task_under_spec("nope", task.id, None, vec![])
                .unwrap_err()
                .code,
            "invalid-parent-type"
        );
        assert_eq!(
            store
                .list_records(&RecordListFilter {
                    project_slug: None,
                    status: None,
                    record_type: None,
                    task_kind: None,
                })
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            store
                .relate_records(task.id, spec.id, RecordRelationshipKind::ParentChild)
                .unwrap_err()
                .code,
            "invalid-relationship"
        );
        assert_eq!(
            store
                .unrelate_records(task.id, spec.id, RecordRelationshipKind::ParentChild)
                .unwrap_err()
                .code,
            "invalid-relationship"
        );
    }

    #[test]
    fn opens_scoped_grillings_and_gates_tasks_on_them() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let (wayfinder, full_scope) = opens_scoped_grillings(&store);
        let alpha_task = store
            .open_task_under_wayfinder(
                "alpha task",
                wayfinder.id,
                Some("alpha".into()),
                None,
                vec![],
            )
            .unwrap();
        store
            .relate_records(
                alpha_task.id,
                full_scope.id,
                RecordRelationshipKind::DependsOn,
            )
            .unwrap();
        assert_eq!(
            store.start_record(alpha_task.id).unwrap_err().code,
            "dependencies-not-done"
        );
        let gamma_task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "gamma task".into(),
                project_slugs: vec!["gamma".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        assert_eq!(
            store
                .relate_records(
                    gamma_task.id,
                    full_scope.id,
                    RecordRelationshipKind::DependsOn,
                )
                .unwrap_err()
                .code,
            "invalid-relationship"
        );
        store.start_record(full_scope.id).unwrap();
        store.done_record(full_scope.id, None).unwrap();
        assert_eq!(
            store.start_record(alpha_task.id).unwrap().status,
            RecordStatus::InProgress
        );
        store.start_record(wayfinder.id).unwrap();
        assert_eq!(
            store.done_record(wayfinder.id, None).unwrap_err().code,
            "children-not-done"
        );
    }

    fn opens_scoped_grillings(store: &Repository) -> (Record, Record) {
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        store.register(&project("beta", "/repos/beta")).unwrap();
        store.register(&project("gamma", "/repos/gamma")).unwrap();
        let wayfinder = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Wayfinder,
                task_kind: None,
                body: "wayfinder".into(),
                project_slugs: vec!["alpha".into(), "beta".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let full_scope = store
            .open_grilling_under_wayfinder("full", wayfinder.id, None, vec![])
            .unwrap();
        assert_eq!(full_scope.record_type, RecordType::Grilling);
        assert_eq!(full_scope.project_slugs, ["alpha", "beta"]);
        assert_eq!(
            store
                .open_grilling_under_wayfinder(
                    "subset",
                    wayfinder.id,
                    Some(vec!["beta".into()]),
                    vec![]
                )
                .unwrap()
                .project_slugs,
            ["beta"]
        );
        assert_eq!(
            store
                .open_grilling_under_wayfinder("empty", wayfinder.id, Some(vec![]), vec![])
                .unwrap_err()
                .code,
            "empty-project-scope"
        );
        assert_eq!(
            store
                .open_grilling_under_wayfinder(
                    "outside",
                    wayfinder.id,
                    Some(vec!["gamma".into()]),
                    vec![]
                )
                .unwrap_err()
                .code,
            "scope-mismatch"
        );
        assert_eq!(
            store
                .list_records(&RecordListFilter {
                    project_slug: None,
                    status: None,
                    record_type: Some(RecordType::Grilling),
                    task_kind: None,
                })
                .unwrap()
                .len(),
            2
        );
        (wayfinder, full_scope)
    }

    #[test]
    fn rejects_pre_workspace_typed_records_without_adopting_them() {
        let path = std::env::temp_dir().join(format!(
            "forge-store-record-type-migration-{}.sqlite",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("PRAGMA foreign_keys = ON;
            CREATE TABLE workspaces (id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL, root TEXT UNIQUE NOT NULL);
            INSERT INTO workspaces VALUES ('test', 'Test', '/');
            CREATE TABLE projects (slug TEXT PRIMARY KEY NOT NULL, registered_folder TEXT UNIQUE NOT NULL, origin_url TEXT NOT NULL, workspace_id TEXT NOT NULL REFERENCES workspaces(id), local_slug TEXT NOT NULL);
            CREATE TABLE records (
                id INTEGER PRIMARY KEY NOT NULL,
                record_type TEXT NOT NULL CHECK (record_type IN ('generic', 'spec', 'task')),
                body TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('open', 'in_progress', 'done')),
                completion_resolution TEXT
            );
            CREATE TABLE record_projects (record_id INTEGER NOT NULL REFERENCES records(id), project_slug TEXT NOT NULL REFERENCES projects(slug), PRIMARY KEY (record_id, project_slug));
            CREATE TABLE record_relationships (
                from_id INTEGER NOT NULL REFERENCES records(id), to_id INTEGER NOT NULL REFERENCES records(id),
                parent_id INTEGER REFERENCES records(id), child_id INTEGER REFERENCES records(id) UNIQUE,
                kind TEXT NOT NULL CHECK (kind IN ('parent-child', 'depends-on', 'generated-by')), PRIMARY KEY (from_id, to_id, kind)
            );
            INSERT INTO projects VALUES ('alpha', '/repos/alpha', 'https://example.test/repo.git', 'test', 'alpha');
            INSERT INTO records VALUES (1, 'generic', 'generic', 'open', NULL);
            INSERT INTO records VALUES (2, 'spec', 'spec', 'open', NULL);
            INSERT INTO records VALUES (3, 'task', 'task', 'done', 'complete');
            INSERT INTO record_projects VALUES (1, 'alpha');
            INSERT INTO record_projects VALUES (2, 'alpha');
            INSERT INTO record_projects VALUES (3, 'alpha');
            INSERT INTO record_relationships VALUES (3, 2, 2, 3, 'parent-child');").unwrap();
        drop(connection);

        let error = SqliteProjectRepository::open(&path).err().unwrap();
        assert_eq!(error.code, "record-workspace-required");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_pre_workspace_generic_records_without_adopting_them() {
        let path = std::env::temp_dir().join(format!(
            "forge-store-migration-{}.sqlite",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("PRAGMA foreign_keys = ON;
            CREATE TABLE workspaces (id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL, root TEXT UNIQUE NOT NULL);
            INSERT INTO workspaces VALUES ('test', 'Test', '/');
            CREATE TABLE projects (slug TEXT PRIMARY KEY NOT NULL, registered_folder TEXT UNIQUE NOT NULL, origin_url TEXT NOT NULL, workspace_id TEXT NOT NULL REFERENCES workspaces(id), local_slug TEXT NOT NULL);
            CREATE TABLE records (id INTEGER PRIMARY KEY NOT NULL, body TEXT NOT NULL, status TEXT NOT NULL CHECK (status IN ('open', 'in_progress', 'done')), completion_resolution TEXT);
            CREATE TABLE record_projects (record_id INTEGER NOT NULL REFERENCES records(id), project_slug TEXT NOT NULL REFERENCES projects(slug), PRIMARY KEY (record_id, project_slug));
            CREATE TABLE record_relationships (parent_id INTEGER NOT NULL REFERENCES records(id), child_id INTEGER NOT NULL REFERENCES records(id) UNIQUE, kind TEXT NOT NULL CHECK (kind IN ('parent-child')), PRIMARY KEY (parent_id, child_id));
            CREATE TABLE record_updates (id INTEGER PRIMARY KEY NOT NULL, record_id INTEGER NOT NULL REFERENCES records(id), created_at_millis INTEGER NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('created', 'body-edited', 'status-transition', 'relationship-added', 'relationship-removed')), from_status TEXT, to_status TEXT, related_record_id INTEGER REFERENCES records(id), comment_id INTEGER);
            CREATE TABLE record_comments (id INTEGER PRIMARY KEY NOT NULL, record_id INTEGER NOT NULL REFERENCES records(id), created_at_millis INTEGER NOT NULL, author TEXT, body TEXT NOT NULL);
            INSERT INTO projects VALUES ('alpha', '/repos/alpha', 'https://example.test/repo.git', 'test', 'alpha');
            INSERT INTO records VALUES (6, 'parent', 'open', NULL);
            INSERT INTO records VALUES (7, 'legacy', 'done', 'complete');
            INSERT INTO record_projects VALUES (6, 'alpha');
            INSERT INTO record_projects VALUES (7, 'alpha');
            INSERT INTO record_relationships VALUES (6, 7, 'parent-child');
            INSERT INTO record_updates VALUES (9, 7, 1, 'created', NULL, NULL, NULL, NULL);
            INSERT INTO record_comments VALUES (11, 7, 2, 'agent', 'comment');").unwrap();
        drop(connection);

        let error = SqliteProjectRepository::open(&path).err().unwrap();
        assert_eq!(error.code, "record-workspace-required");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn assignment_context_includes_only_direct_provenance() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        for (slug, folder) in [
            ("alpha", "/repos/alpha"),
            ("beta", "/repos/beta"),
            ("gamma", "/repos/gamma"),
        ] {
            store.register(&project(slug, folder)).unwrap();
        }
        let task = |body: &str, project_slug: &str| NewTypedRecord {
            record_type: RecordType::Task,
            task_kind: None,
            body: body.into(),
            project_slugs: vec![project_slug.into()],
            generated_by_ids: vec![],
        };
        let assigned = store.open_typed_record(&task("assigned", "alpha")).unwrap();
        let prerequisite = store
            .open_typed_record(&task("prerequisite", "beta"))
            .unwrap();
        let dependent = store
            .open_typed_record(&task("dependent", "gamma"))
            .unwrap();
        let transitive = store
            .open_typed_record(&task("transitive", "beta"))
            .unwrap();
        let generator = store
            .open_typed_record(&task("generator", "gamma"))
            .unwrap();
        let transitive_generator = store
            .open_typed_record(&task("transitive generator", "beta"))
            .unwrap();
        store
            .relate_records(
                assigned.id,
                prerequisite.id,
                RecordRelationshipKind::DependsOn,
            )
            .unwrap();
        store
            .relate_records(dependent.id, assigned.id, RecordRelationshipKind::DependsOn)
            .unwrap();
        store
            .relate_records(
                prerequisite.id,
                transitive.id,
                RecordRelationshipKind::DependsOn,
            )
            .unwrap();
        store
            .relate_records(
                assigned.id,
                generator.id,
                RecordRelationshipKind::GeneratedBy,
            )
            .unwrap();
        store
            .relate_records(
                generator.id,
                transitive_generator.id,
                RecordRelationshipKind::GeneratedBy,
            )
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "owner".into(),
                record_id: assigned.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();

        let context = assignment_context(&store, "owner", "cap-owner", "context").unwrap();
        assert_eq!(context.task.id, assigned.id);
        let mut related_ids = context
            .directly_related_records
            .iter()
            .map(|record| record.id)
            .collect::<Vec<_>>();
        related_ids.sort_unstable();
        assert_eq!(
            related_ids,
            vec![prerequisite.id, dependent.id, generator.id]
        );
        assert_eq!(
            context
                .directly_related_records
                .iter()
                .find(|record| record.id == generator.id)
                .unwrap()
                .project_slugs,
            ["gamma"]
        );
        assert!(
            !context
                .directly_related_records
                .iter()
                .any(|record| record.id == transitive_generator.id)
        );
    }

    #[test]
    fn persists_interactive_grilling_assignments_and_dispatch_holds() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let grilling = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Grilling,
                task_kind: None,
                body: "review this".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();

        assert_eq!(
            store
                .agent_profile(RecordType::Grilling, 1)
                .unwrap()
                .prompt_template,
            "agent-dispatch/grilling/v1"
        );
        let assignment = store
            .create_agent_assignment(&NewAgentAssignment {
                id: "grilling-attempt".into(),
                record_id: grilling.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        assert_eq!(assignment.profile.record_type, RecordType::Grilling);
        store
            .hold_dispatch(grilling.id, "waiting for operator")
            .unwrap();
        assert_eq!(
            store.dispatch_hold(grilling.id).unwrap().unwrap().reason,
            "waiting for operator"
        );
    }

    #[test]
    fn persists_dispatch_profiles_assignments_resources_and_project_configuration() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "task".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store.set_task_base_commit(task.id, "base-commit").unwrap();
        assert_eq!(
            store.task_base_commit(task.id).unwrap().as_deref(),
            Some("base-commit")
        );
        assert_eq!(
            store
                .set_task_base_commit(task.id, "replacement")
                .unwrap_err()
                .code,
            "base-commit-exists"
        );

        let profile = store.agent_profile(RecordType::Task, 1).unwrap();
        assert_eq!(profile.provider, "herdr");
        assert_eq!(profile.agent_kind, "pi");
        assert_eq!(profile.model, "gpt-5.6-terra");
        assert_eq!(profile.acknowledgement_timeout_millis, 300_000);
        assert_eq!(profile.startup_timeout_millis, 30_000);
        assert_eq!(store.agent_profile(RecordType::Task, 1).unwrap(), profile);

        asserts_dispatch_workspace_persists(&store);

        // Provider configuration must be established before an Assignment reserves it.
        asserts_project_configuration_persists(&store);
        let assignment = store
            .create_agent_assignment(&NewAgentAssignment {
                id: "attempt-1".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        assert_eq!(assignment.record_id, task.id);
        assert_eq!(
            store.list_agent_assignments(task.id).unwrap(),
            vec![assignment.clone()]
        );
        assert_eq!(
            store
                .create_agent_assignment(&NewAgentAssignment {
                    id: "attempt-2".into(),
                    record_id: task.id,
                    project_slug: "alpha".into(),
                    profile_revision: 1,
                })
                .unwrap_err()
                .code,
            "open-assignment-exists"
        );
        let resource = store
            .add_agent_assignment_resource(&NewAgentAssignmentResource {
                assignment_id: assignment.id.clone(),
                resource_kind: "session".into(),
                resource_id: "external-1".into(),
            })
            .unwrap();
        assert_eq!(
            store
                .list_agent_assignment_resources(&assignment.id)
                .unwrap(),
            vec![resource.clone()]
        );
        assert_eq!(
            store
                .add_agent_assignment_resource(&NewAgentAssignmentResource {
                    assignment_id: assignment.id.clone(),
                    resource_kind: "session".into(),
                    resource_id: "external-1".into(),
                })
                .unwrap(),
            resource,
            "recovery can retain an already-persisted resource safely"
        );
        let retired = store
            .retire_agent_assignment_resource(&assignment.id, "session", "external-1")
            .unwrap();
        assert!(retired.retired_at_millis.is_some());
        assert_eq!(
            store
                .retire_agent_assignment_resource(&assignment.id, "session", "external-1")
                .unwrap(),
            retired,
            "retirement evidence is idempotent"
        );
    }

    fn asserts_dispatch_workspace_persists(store: &Repository) {
        let workspace = forge_app::DispatchWorkspace {
            id: "shared-workspace".into(),
        };
        assert_eq!(store.dispatch_workspace().unwrap(), None);
        assert_eq!(
            store.retain_dispatch_workspace(&workspace).unwrap(),
            workspace
        );
        assert_eq!(store.dispatch_workspace().unwrap(), Some(workspace.clone()));
        assert_eq!(
            store
                .retain_dispatch_workspace(&forge_app::DispatchWorkspace {
                    id: "other-workspace".into(),
                })
                .unwrap_err()
                .code,
            "dispatch-workspace-conflict"
        );
    }

    fn asserts_project_configuration_persists(store: &Repository) {
        assert_eq!(
            store
                .effective_project_task_concurrency_limit("alpha")
                .unwrap(),
            1
        );
        store
            .set_project_task_concurrency_limit("alpha", Some(4))
            .unwrap();
        assert_eq!(
            store
                .effective_project_task_concurrency_limit("alpha")
                .unwrap(),
            4
        );
        assert_eq!(
            store
                .set_project_task_concurrency_limit("alpha", Some(0))
                .unwrap_err()
                .code,
            "invalid-concurrency-limit"
        );
        store
            .set_herdr_socket_path(Some(Path::new("/tmp/herdr.sock")))
            .unwrap();
        assert_eq!(
            store.herdr_socket_path().unwrap(),
            Some(std::path::PathBuf::from("/tmp/herdr.sock"))
        );
        assert_eq!(
            store
                .set_herdr_socket_path(Some(Path::new("relative.sock")))
                .unwrap_err()
                .code,
            "invalid-socket-path"
        );
    }

    #[test]
    fn provisioning_intent_can_only_be_claimed_once() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "work".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "intent".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        assert!(store.begin_agent_provision("intent").unwrap());
        assert!(!store.begin_agent_provision("intent").unwrap());
    }

    #[test]
    fn closed_assignment_with_retained_resources_reserves_project_capacity() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "first".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "old".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        store
            .add_agent_assignment_resource(&forge_app::NewAgentAssignmentResource {
                assignment_id: "old".into(),
                resource_kind: "tab".into(),
                resource_id: "tab".into(),
            })
            .unwrap();
        store
            .close_agent_assignment("old", forge_app::AgentAssignmentClosureReason::Crashed)
            .unwrap();
        assert!(
            store
                .claim_next_agent_assignment("alpha", "new")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn dispatch_workspace_reads_legacy_and_id_only_configuration() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        {
            let connection = store.connection.lock().unwrap();
            connection.execute(
                "INSERT INTO agent_dispatch_configuration (singleton, dispatch_workspace_id, dispatch_workspace_root) VALUES (1, 'w1', '/legacy')",
                [],
            ).unwrap();
        }
        let workspace = forge_app::DispatchWorkspace { id: "w1".into() };
        assert_eq!(store.dispatch_workspace().unwrap(), Some(workspace.clone()));
        store.select_dispatch_workspace(&workspace).unwrap();
        assert_eq!(store.dispatch_workspace().unwrap(), Some(workspace));
        let connection = store.connection.lock().unwrap();
        let root: Option<String> = connection.query_row(
            "SELECT dispatch_workspace_root FROM agent_dispatch_configuration WHERE singleton = 1",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(root, None);
    }

    #[test]
    fn socket_changes_and_unsets_preserve_configuration_while_assignment_is_open() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store
            .set_herdr_socket_path(Some(Path::new("/tmp/first.sock")))
            .unwrap();
        store
            .select_dispatch_workspace(&forge_app::DispatchWorkspace {
                id: "selected".into(),
            })
            .unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "work".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "session".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        assert!(
            store
                .set_herdr_socket_path(Some(Path::new("/tmp/second.sock")))
                .is_err()
        );
        assert!(store.set_herdr_socket_path(None).is_err());
        assert!(store.unset_dispatch_workspace().is_err());
        assert_eq!(
            store.herdr_socket_path().unwrap(),
            Some(PathBuf::from("/tmp/first.sock"))
        );
        assert_eq!(store.dispatch_workspace().unwrap().unwrap().id, "selected");
        // Reapplying identical values is not a provider configuration change.
        store
            .set_herdr_socket_path(Some(Path::new("/tmp/first.sock")))
            .unwrap();
        store
            .add_agent_assignment_resource(&NewAgentAssignmentResource {
                assignment_id: "session".into(),
                resource_kind: "agent-name".into(),
                resource_id: "agent-1".into(),
            })
            .unwrap();
        store
            .close_agent_assignment("session", forge_app::AgentAssignmentClosureReason::Crashed)
            .unwrap();
        // Closed does not mean that provider resources are safe to abandon.
        assert!(store.set_herdr_socket_path(None).is_err());
        assert!(
            store
                .set_herdr_socket_path(Some(Path::new("/tmp/second.sock")))
                .is_err()
        );
        assert!(store.unset_dispatch_workspace().is_err());
        assert!(
            store
                .select_dispatch_workspace(&forge_app::DispatchWorkspace { id: "other".into() })
                .is_err()
        );
        let info = store.agent_assignment_info("session").unwrap();
        assert!(info.capacity_reserved());
        assert_eq!(info.resources[0].resource_id, "agent-1");
        assert!(info.resources[0].retired_at_millis.is_none());
        store
            .retire_agent_assignment_resource("session", "agent-name", "agent-1")
            .unwrap();
        store.unset_dispatch_workspace().unwrap();
        store.set_herdr_socket_path(None).unwrap();
        assert_eq!(store.dispatch_workspace().unwrap(), None);
        assert_eq!(store.herdr_socket_path().unwrap(), None);
        // Unsetting is repeat-safe, including before any configuration row exists.
        store.unset_dispatch_workspace().unwrap();
        let fresh = SqliteProjectRepository::in_memory().unwrap();
        fresh.unset_dispatch_workspace().unwrap();
        fresh.set_herdr_socket_path(None).unwrap();
    }

    #[test]
    fn workspace_reselection_is_blocked_by_assignments_and_retained_resources() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let first = forge_app::DispatchWorkspace { id: "first".into() };
        let second = forge_app::DispatchWorkspace {
            id: "second".into(),
        };
        store.select_dispatch_workspace(&first).unwrap();
        store.select_dispatch_workspace(&second).unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "work".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "session".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        assert!(store.select_dispatch_workspace(&first).is_err());
        assert_eq!(store.dispatch_workspace().unwrap(), Some(second));
    }

    #[test]
    fn retain_dispatch_workspace_fills_existing_configuration_without_overwriting_identity() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store
            .set_herdr_socket_path(Some(Path::new("/tmp/herdr.sock")))
            .unwrap();
        let workspace = forge_app::DispatchWorkspace { id: "w1".into() };
        assert_eq!(
            store.retain_dispatch_workspace(&workspace).unwrap(),
            workspace
        );
        assert_eq!(
            store.retain_dispatch_workspace(&workspace).unwrap(),
            workspace
        );
        let different = forge_app::DispatchWorkspace { id: "w2".into() };
        assert_eq!(
            store
                .retain_dispatch_workspace(&different)
                .unwrap_err()
                .code,
            "dispatch-workspace-conflict"
        );
        assert_eq!(store.dispatch_workspace().unwrap(), Some(workspace));
        assert_eq!(
            store.herdr_socket_path().unwrap(),
            Some(PathBuf::from("/tmp/herdr.sock"))
        );
    }

    #[test]
    fn recovery_requires_hold_and_exact_path_and_retains_hold() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "task".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let path = PathBuf::from(format!("/.forge/worktrees/alpha/{}", task.id));
        assert_eq!(
            store
                .recover_worktree_claim(task.id, &path)
                .unwrap_err()
                .code,
            "recovery-blocked"
        );
        store.hold_dispatch(task.id, "operator recovery").unwrap();
        assert_eq!(
            store
                .recover_worktree_claim(task.id, Path::new("/wrong"))
                .unwrap_err()
                .code,
            "recovery-path-changed"
        );
        assert_eq!(
            store.recover_worktree_claim(task.id, &path).unwrap().state,
            forge_app::WorktreeClaimState::Active
        );
        assert_eq!(
            store
                .recover_worktree_claim(task.id, &path)
                .unwrap_err()
                .code,
            "worktree-claim-exists"
        );
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
    }

    #[test]
    fn persists_project_hooks_and_workspace_worktree_claim_lifecycle() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        store
            .set_project_setup_hook("alpha", Some("cargo test"))
            .unwrap();
        assert_eq!(
            store.project_setup_hook("alpha").unwrap().as_deref(),
            Some("cargo test")
        );

        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "task".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let reserved = store.reserve_worktree_claim(task.id).unwrap();
        assert_eq!(
            reserved.path,
            PathBuf::from(format!("/.forge/worktrees/alpha/{}", task.id))
        );
        assert_eq!(reserved.state, forge_app::WorktreeClaimState::Reserving);
        assert_eq!(
            store.reserve_worktree_claim(task.id).unwrap(),
            reserved,
            "reservation is recovery-safe"
        );
        let active = store.activate_worktree_claim(task.id).unwrap();
        assert_eq!(active.state, forge_app::WorktreeClaimState::Active);
        let cleanup = store.require_worktree_claim_cleanup(task.id).unwrap();
        assert_eq!(
            cleanup.state,
            forge_app::WorktreeClaimState::CleanupRequired
        );
        assert_eq!(
            store.worktree_claims_requiring_cleanup().unwrap(),
            vec![cleanup.clone()]
        );
        store.start_record(task.id).unwrap();
        check_tests::assess(&store, task.id, forge_app::checks::Verdict::Pass);
        store.done_record(task.id, None).unwrap();
        assert_eq!(
            store.worktree_claim(task.id).unwrap(),
            Some(cleanup),
            "completed Task claims are retained"
        );
        store.delete_worktree_claim(task.id).unwrap();
        assert_eq!(store.worktree_claim(task.id).unwrap(), None);
        assert_eq!(
            store.delete_worktree_claim(task.id).unwrap_err().code,
            "worktree-claim-not-found"
        );
    }

    #[derive(Clone)]
    struct FakeAdapter {
        provision_fails: bool,
    }

    impl forge_app::AgentDispatchAdapter for FakeAdapter {
        fn verify_workspace(
            &self,
            id: &str,
        ) -> Result<forge_app::DispatchWorkspace, forge_app::AgentAdapterError> {
            Ok(forge_app::DispatchWorkspace { id: id.into() })
        }
        fn probe(&self) -> Result<(), forge_app::AgentAdapterError> {
            Ok(())
        }

        fn provision(
            &self,
            _: &forge_app::AgentDispatchContext,
            _: &dyn forge_app::AgentDispatchRepository,
        ) -> Result<(), forge_app::AgentAdapterError> {
            if self.provision_fails {
                Err(forge_app::AgentAdapterError::Transient("offline".into()))
            } else {
                Ok(())
            }
        }

        fn observe(
            &self,
            _: &forge_app::AgentDispatchContext,
        ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
            Ok(forge_app::AgentObservation::Working)
        }

        fn recover(
            &self,
            _: &forge_app::AgentDispatchContext,
        ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
            Ok(forge_app::AgentObservation::Absent)
        }

        fn retire(
            &self,
            _: &forge_app::AgentDispatchContext,
            _: &dyn forge_app::AgentDispatchRepository,
        ) -> Result<(), forge_app::AgentAdapterError> {
            Ok(())
        }
    }

    impl forge_app::AgentDispatchAdapterFactory for FakeAdapter {
        fn adapter_for(
            &self,
            _: &forge_app::AgentProfile,
        ) -> Result<Box<dyn forge_app::AgentDispatchAdapter>, forge_app::AgentAdapterError>
        {
            Ok(Box::new(self.clone()))
        }
    }

    #[test]
    fn acknowledgement_wins_against_an_in_flight_launch_error() {
        struct RacingAdapter;
        impl forge_app::AgentDispatchAdapter for RacingAdapter {
            fn probe(&self) -> Result<(), forge_app::AgentAdapterError> {
                Ok(())
            }
            fn provision(
                &self,
                context: &forge_app::AgentDispatchContext,
                repo: &dyn AgentDispatchRepository,
            ) -> Result<(), forge_app::AgentAdapterError> {
                assert!(
                    repo.begin_agent_launch(&context.assignment.id, 1_000)
                        .unwrap()
                );
                repo.acknowledge_agent_assignment(&context.assignment.id)
                    .unwrap();
                Err(forge_app::AgentAdapterError::Transient(
                    "lost response".into(),
                ))
            }
            fn observe(
                &self,
                _: &forge_app::AgentDispatchContext,
            ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
                unreachable!()
            }
            fn recover(
                &self,
                _: &forge_app::AgentDispatchContext,
            ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
                unreachable!()
            }
            fn retire(
                &self,
                _: &forge_app::AgentDispatchContext,
                _: &dyn AgentDispatchRepository,
            ) -> Result<(), forge_app::AgentAdapterError> {
                unreachable!()
            }
        }
        for kind in [RecordType::Task, RecordType::Grilling] {
            let store = SqliteProjectRepository::in_memory().unwrap();
            store.register(&project("alpha", "/repos/alpha")).unwrap();
            let task = store
                .open_typed_record(&NewTypedRecord {
                    record_type: kind,
                    task_kind: None,
                    body: "task".into(),
                    project_slugs: vec!["alpha".into()],
                    generated_by_ids: vec![],
                })
                .unwrap();
            let assignment = store
                .create_agent_assignment(&NewAgentAssignment {
                    id: "race".into(),
                    record_id: task.id,
                    project_slug: "alpha".into(),
                    profile_revision: 1,
                })
                .unwrap();
            assert!(matches!(
                forge_app::reconcile_agent_assignment(&store, &RacingAdapter, &assignment).unwrap(),
                forge_app::AssignmentReconciliation::Stale
            ));
            assert_eq!(
                store.agent_assignment("race").unwrap().state,
                forge_app::AgentAssignmentState::Working
            );
            assert!(store.dispatch_hold(task.id).unwrap().is_none());
            assert!(!store.begin_agent_launch("race", 2_000).unwrap());
        }
    }

    #[test]
    fn launch_success_and_lost_response_wait_for_acknowledgement() {
        struct LaunchAdapter {
            fail: bool,
        }
        impl forge_app::AgentDispatchAdapter for LaunchAdapter {
            fn probe(&self) -> Result<(), forge_app::AgentAdapterError> {
                Ok(())
            }
            fn provision(
                &self,
                context: &forge_app::AgentDispatchContext,
                repo: &dyn AgentDispatchRepository,
            ) -> Result<(), forge_app::AgentAdapterError> {
                assert!(
                    repo.begin_agent_launch(&context.assignment.id, 1_000)
                        .unwrap()
                );
                if self.fail {
                    Err(forge_app::AgentAdapterError::Transient(
                        "response lost".into(),
                    ))
                } else {
                    Ok(())
                }
            }
            fn observe(
                &self,
                _: &forge_app::AgentDispatchContext,
            ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
                unreachable!()
            }
            fn recover(
                &self,
                _: &forge_app::AgentDispatchContext,
            ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
                unreachable!()
            }
            fn retire(
                &self,
                _: &forge_app::AgentDispatchContext,
                _: &dyn AgentDispatchRepository,
            ) -> Result<(), forge_app::AgentAdapterError> {
                unreachable!()
            }
        }
        for (fail, kind) in [
            (false, RecordType::Task),
            (true, RecordType::Task),
            (false, RecordType::Grilling),
            (true, RecordType::Grilling),
        ] {
            let store = SqliteProjectRepository::in_memory().unwrap();
            store.register(&project("alpha", "/repos/alpha")).unwrap();
            let task = store
                .open_typed_record(&NewTypedRecord {
                    record_type: kind,
                    task_kind: None,
                    body: "task".into(),
                    project_slugs: vec!["alpha".into()],
                    generated_by_ids: vec![],
                })
                .unwrap();
            let mut assignment = store
                .create_agent_assignment(&NewAgentAssignment {
                    id: "launch".into(),
                    record_id: task.id,
                    project_slug: "alpha".into(),
                    profile_revision: 1,
                })
                .unwrap();
            if kind == RecordType::Grilling {
                assignment.profile.launch_policy =
                    Some(forge_app::agent_profiles::ResolvedLaunchPolicy {
                        version: 1,
                        runtime: "herdr/pi".into(),
                        model: "openai/test".into(),
                        tools: vec![],
                        behavior: String::new(),
                        skills: vec![],
                        extensions: vec![],
                        mcp_servers: vec![],
                    });
            }
            assert!(matches!(
                forge_app::reconcile_agent_assignment(&store, &LaunchAdapter { fail }, &assignment)
                    .unwrap(),
                forge_app::AssignmentReconciliation::AwaitingAcknowledgement
            ));
            assert_eq!(
                store.agent_assignment("launch").unwrap().state,
                forge_app::AgentAssignmentState::Provisioning
            );
            assert!(store.dispatch_hold(task.id).unwrap().is_none());
        }
    }

    #[test]
    fn reconciliation_scan_uses_a_lease_and_durable_retry_exhaustion() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "dispatch me".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "retrying".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        let policy = forge_app::AgentDispatchReconciliationPolicy {
            scan_lease_millis: 100,
        };
        let failing = FakeAdapter {
            provision_fails: true,
        };
        for now in [0, 1_000, 2_000, 3_000, 4_000] {
            forge_app::reconcile_agent_dispatch(&store, &failing, "daemon-a", now, policy).unwrap();
        }
        let assignment = store.list_agent_assignments(task.id).unwrap().remove(0);
        assert_eq!(assignment.retry_count, 1);
        assert_eq!(
            assignment.closure_reason, None,
            "a failed provisioning attempt may have left a live agent and must reserve capacity"
        );
        assert_eq!(
            store.dispatch_hold(task.id).unwrap().unwrap().reason,
            "Automatic dispatch stopped after an uncertain provisioning attempt; investigate the open Assignment and potentially live resources."
        );
        assert_eq!(
            store.list_record_comments(task.id).unwrap().len(),
            1,
            "retry exhaustion records exactly one explanatory comment"
        );
        assert!(
            !store
                .try_acquire_agent_dispatch_scan_lease("daemon-b", 4_000, 100)
                .unwrap(),
            "a concurrent daemon cannot start another scan"
        );
        let working = FakeAdapter {
            provision_fails: false,
        };
        let result =
            forge_app::reconcile_agent_dispatch(&store, &working, "daemon-a", 5_000, policy)
                .unwrap();
        assert!(result.acquired_lease);
        assert!(result.reconciled.is_empty());
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
    }

    #[test]
    fn agent_dispatch_skips_blocked_tasks_and_claims_eligible_work() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = |body: &str| NewTypedRecord {
            record_type: RecordType::Task,
            task_kind: None,
            body: body.into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        };
        let blocked = store.open_typed_record(&task("blocked")).unwrap();
        let prerequisite = store.open_typed_record(&task("prerequisite")).unwrap();
        let eligible = store.open_typed_record(&task("eligible")).unwrap();
        store
            .relate_records(
                blocked.id,
                prerequisite.id,
                RecordRelationshipKind::DependsOn,
            )
            .unwrap();

        let assignment = store
            .claim_next_agent_assignment("alpha", "claim-1")
            .unwrap()
            .expect("eligible work must not be hidden by a blocked Task");
        assert_eq!(assignment.record_id, prerequisite.id);
        assert_eq!(
            store.show_record(blocked.id).unwrap().status,
            RecordStatus::Open
        );
        assert_eq!(
            store.show_record(eligible.id).unwrap().status,
            RecordStatus::Open
        );
    }

    #[test]
    fn explicit_dispatch_selects_requested_task_and_reuses_open_assignment() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let mut tasks = Vec::new();
        for body in ["older", "requested"] {
            tasks.push(
                store
                    .open_typed_record(&NewTypedRecord {
                        record_type: RecordType::Task,
                        task_kind: None,
                        body: body.into(),
                        project_slugs: vec!["alpha".into()],
                        generated_by_ids: vec![],
                    })
                    .unwrap(),
            );
        }
        let (assignment, already_assigned) = store
            .claim_record_agent_assignment(tasks[1].id, "explicit")
            .unwrap();
        assert_eq!(assignment.record_id, tasks[1].id);
        assert!(!already_assigned);
        let (repeated, already_assigned) = store
            .claim_record_agent_assignment(tasks[1].id, "duplicate")
            .unwrap();
        assert_eq!(repeated.id, "explicit");
        assert!(already_assigned);
        assert_eq!(store.list_open_agent_assignments().unwrap().len(), 1);
        assert_eq!(
            store
                .claim_record_agent_assignment(tasks[0].id, "full")
                .unwrap_err()
                .code,
            "project-capacity"
        );
    }

    #[test]
    fn explicit_dispatch_enforces_supported_types_readiness_and_holds() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        store.register(&project("beta", "/repos/beta")).unwrap();
        let open = |record_type, scopes: Vec<&str>| {
            store
                .open_typed_record(&NewTypedRecord {
                    record_type,
                    task_kind: None,
                    body: "record".into(),
                    project_slugs: scopes.into_iter().map(str::to_owned).collect(),
                    generated_by_ids: vec![],
                })
                .unwrap()
        };
        for kind in [RecordType::Spec, RecordType::Wayfinder] {
            let record = open(kind, vec!["alpha"]);
            assert_eq!(
                store
                    .claim_record_agent_assignment(record.id, "unsupported")
                    .unwrap_err()
                    .code,
                "unsupported-record-type"
            );
            assert!(store.list_agent_assignments(record.id).unwrap().is_empty());
        }
        let generic = store
            .open_record(&NewRecord {
                body: "generic".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        assert_eq!(
            store
                .claim_record_agent_assignment(generic.id, "generic")
                .unwrap_err()
                .code,
            "unsupported-record-type"
        );
        let prerequisite = open(RecordType::Task, vec!["alpha"]);
        let task = open(RecordType::Task, vec!["alpha"]);
        store
            .relate_records(task.id, prerequisite.id, RecordRelationshipKind::DependsOn)
            .unwrap();
        assert_eq!(
            store
                .claim_record_agent_assignment(task.id, "blocked")
                .unwrap_err()
                .code,
            "record-not-ready"
        );
        store.start_record(prerequisite.id).unwrap();
        check_tests::assess(&store, prerequisite.id, forge_app::checks::Verdict::Pass);
        store.done_record(prerequisite.id, Some("done")).unwrap();
        store.hold_dispatch(task.id, "investigate").unwrap();
        assert_eq!(
            store
                .claim_record_agent_assignment(task.id, "held")
                .unwrap_err()
                .code,
            "dispatch-held"
        );
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        let in_progress = open(RecordType::Task, vec!["beta"]);
        store.start_record(in_progress.id).unwrap();
        assert_eq!(
            store
                .claim_record_agent_assignment(in_progress.id, "in-progress")
                .unwrap_err()
                .code,
            "record-not-ready"
        );
    }

    #[test]
    fn explicit_dispatch_rejects_closed_assignment_resources() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let grilling = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Grilling,
                task_kind: None,
                body: "record".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let (assignment, _) = store
            .claim_record_agent_assignment(grilling.id, "interactive")
            .unwrap();
        assert_eq!(assignment.profile.record_type, RecordType::Grilling);
        store
            .add_agent_assignment_resource(&NewAgentAssignmentResource {
                assignment_id: assignment.id.clone(),
                resource_kind: "agent-name".into(),
                resource_id: "retained-agent".into(),
            })
            .unwrap();
        store
            .close_agent_assignment(
                &assignment.id,
                forge_app::AgentAssignmentClosureReason::Lost,
            )
            .unwrap();
        assert_eq!(
            store
                .claim_record_agent_assignment(grilling.id, "replacement")
                .unwrap_err()
                .code,
            "assignment-recovery-blocked"
        );
        assert_eq!(
            store
                .list_agent_assignment_resources(&assignment.id)
                .unwrap()[0]
                .resource_id,
            "retained-agent"
        );
        assert_eq!(store.list_agent_assignments(grilling.id).unwrap().len(), 1);
    }

    #[test]
    fn concurrent_explicit_dispatch_reuses_one_assignment_across_connections() {
        let path = std::env::temp_dir().join(format!(
            "forge-explicit-{}-{}.sqlite",
            std::process::id(),
            super::now_millis().unwrap()
        ));
        let store = SqliteProjectRepository::open(&path).unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "task".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let repositories: Vec<_> = (0..4)
            .map(|_| SqliteProjectRepository::open(&path).unwrap())
            .collect();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let threads: Vec<_> = repositories
            .into_iter()
            .enumerate()
            .map(|(index, repository)| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    repository
                        .claim_record_agent_assignment(task.id, &format!("request-{index}"))
                        .unwrap()
                })
            })
            .collect();
        let results: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|(_, existing)| !existing).count(), 1);
        assert!(
            results
                .iter()
                .all(|(assignment, _)| assignment.id == results[0].0.id)
        );
        assert_eq!(store.list_open_agent_assignments().unwrap().len(), 1);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn automatic_dispatch_ignores_grillings_and_keeps_oldest_task_order() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let mut ids = Vec::new();
        for record_type in [RecordType::Grilling, RecordType::Task, RecordType::Task] {
            ids.push(
                store
                    .open_typed_record(&NewTypedRecord {
                        record_type,
                        task_kind: None,
                        body: "work".into(),
                        project_slugs: vec!["alpha".into()],
                        generated_by_ids: vec![],
                    })
                    .unwrap()
                    .id,
            );
        }
        assert_eq!(
            store
                .claim_next_agent_assignment("alpha", "automatic")
                .unwrap()
                .unwrap()
                .record_id,
            ids[1]
        );
        assert!(store.list_agent_assignments(ids[0]).unwrap().is_empty());
    }

    #[test]
    fn atomically_claims_eligible_tasks_and_retains_capacity() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let first = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "first".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "second".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();

        let assignment = store
            .claim_next_agent_assignment("alpha", "claim-1")
            .unwrap()
            .expect("the first task is eligible");
        assert_eq!(assignment.record_id, first.id);
        assert_eq!(
            assignment.state,
            forge_app::AgentAssignmentState::Provisioning
        );
        assert_eq!(
            store.show_record(first.id).unwrap().status,
            RecordStatus::Open
        );
        assert_eq!(
            store
                .compare_and_set_agent_assignment_state(
                    "claim-1",
                    forge_app::AgentAssignmentState::Provisioning,
                    forge_app::AgentAssignmentState::Working,
                )
                .unwrap()
                .expect("the fresh observer wins")
                .state,
            forge_app::AgentAssignmentState::Working
        );
        assert!(
            store
                .compare_and_set_agent_assignment_state(
                    "claim-1",
                    forge_app::AgentAssignmentState::Provisioning,
                    forge_app::AgentAssignmentState::Idle,
                )
                .unwrap()
                .is_none(),
            "a stale observer must not overwrite the fresh observation"
        );
        assert!(
            store
                .claim_next_agent_assignment("alpha", "claim-2")
                .unwrap()
                .is_none()
        );
        store
            .close_agent_assignment("claim-1", forge_app::AgentAssignmentClosureReason::Lost)
            .unwrap();
        let retry = store
            .claim_next_agent_assignment("alpha", "claim-3")
            .unwrap()
            .expect("a closed attempt permits a replacement");
        assert_eq!(retry.record_id, first.id);
        assert_eq!(retry.attempt_number, 2);
    }

    #[test]
    fn legacy_profile_and_assignment_migrate_without_inventing_launch_intent() {
        let path = std::env::temp_dir().join(format!(
            "forge-launch-migration-{}.sqlite",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteProjectRepository::open(&path).unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "legacy".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "legacy".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        drop(store);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("ALTER TABLE agent_profiles DROP COLUMN acknowledgement_timeout_millis; ALTER TABLE agent_assignments DROP COLUMN acknowledged_at_millis; ALTER TABLE agent_assignments DROP COLUMN launch_started_at_millis; ALTER TABLE agent_assignments DROP COLUMN acknowledgement_deadline_millis; ALTER TABLE agent_assignments DROP COLUMN safe_dispatch_generation;").unwrap();
        drop(db);
        let store = SqliteProjectRepository::open(&path).unwrap();
        assert_eq!(
            store
                .agent_profile(RecordType::Task, 1)
                .unwrap()
                .acknowledgement_timeout_millis,
            300_000
        );
        assert_eq!(store.agent_launch_intent("legacy").unwrap(), None);
        assert!(!store.guard_agent_provisioning("legacy", 1).unwrap());
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        store.resume_dispatch(task.id).unwrap();
        assert!(!store.guard_agent_provisioning("legacy", 2).unwrap());
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        assert_eq!(
            store.agent_assignment("legacy").unwrap().state,
            forge_app::AgentAssignmentState::Provisioning
        );
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn acknowledgement_timeout_configuration_creates_a_new_immutable_profile_revision() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let original = store.active_task_agent_profile().unwrap();
        let updated = store
            .configure_workspace_acknowledgement_timeout("test", Some(42_000))
            .unwrap();
        assert_eq!(updated.revision, original.revision + 1);
        assert_eq!(updated.acknowledgement_timeout_millis, 42_000);
        assert_eq!(
            updated.startup_timeout_millis,
            original.startup_timeout_millis
        );
        assert_eq!(
            store
                .agent_profile(RecordType::Task, original.revision)
                .unwrap(),
            original
        );
        assert_eq!(store.active_task_agent_profile().unwrap(), updated);
        assert!(
            store
                .configure_task_acknowledgement_timeout(Some(0))
                .is_err()
        );
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "configured".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let assignment = store
            .claim_next_agent_assignment("alpha", "configured")
            .unwrap()
            .unwrap();
        assert_eq!(assignment.profile, updated);
        assert!(store.begin_agent_launch("configured", 1000).unwrap());
        assert_eq!(
            store
                .agent_launch_intent("configured")
                .unwrap()
                .unwrap()
                .acknowledgement_deadline_millis,
            43_000
        );
    }

    #[test]
    fn launch_intent_is_durable_and_cannot_be_replaced() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "task".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "launch".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        assert_eq!(store.agent_launch_intent("launch").unwrap(), None);
        assert!(store.begin_agent_launch("launch", 1000).unwrap());
        assert!(!store.begin_agent_launch("launch", 2000).unwrap());
        let intent = store.agent_launch_intent("launch").unwrap().unwrap();
        assert_eq!(intent.started_at_millis, 1000);
        assert_eq!(intent.acknowledgement_deadline_millis, 301_000);
    }

    #[test]
    fn provisioning_deadline_holds_until_operator_resolution_and_late_ack_keeps_hold() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "deadline".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "deadline".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        assert!(store.guard_agent_provisioning("deadline", 999).unwrap());
        assert!(store.begin_agent_launch("deadline", 1000).unwrap());
        assert!(!store.guard_agent_provisioning("deadline", 300_999).unwrap());
        assert!(store.dispatch_hold(task.id).unwrap().is_none());
        assert!(!store.guard_agent_provisioning("deadline", 301_000).unwrap());
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        store.resume_dispatch(task.id).unwrap();
        assert!(!store.guard_agent_provisioning("deadline", 301_001).unwrap());
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        store.acknowledge_agent_assignment("deadline").unwrap();
        assert!(!store.guard_agent_provisioning("deadline", 301_002).unwrap());
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        assert_eq!(
            store.agent_assignment("deadline").unwrap().state,
            forge_app::AgentAssignmentState::Working
        );
    }

    #[test]
    fn scan_after_restart_retains_launch_and_holds_at_deadline() {
        let path = std::env::temp_dir().join(format!(
            "forge-reconcile-restart-{}.sqlite",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteProjectRepository::open(&path).unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "restart".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "restart".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        store.begin_agent_launch("restart", 1000).unwrap();
        drop(store);
        let store = SqliteProjectRepository::open(&path).unwrap();
        let factory = FakeAdapter {
            provision_fails: false,
        };
        let policy = forge_app::AgentDispatchReconciliationPolicy {
            scan_lease_millis: 10,
        };
        forge_app::reconcile_agent_dispatch(&store, &factory, "daemon", 300_999, policy).unwrap();
        assert!(store.dispatch_hold(task.id).unwrap().is_none());
        forge_app::reconcile_agent_dispatch(&store, &factory, "daemon", 301_010, policy).unwrap();
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        store.resume_dispatch(task.id).unwrap();
        forge_app::reconcile_agent_dispatch(&store, &factory, "daemon", 301_020, policy).unwrap();
        assert!(store.dispatch_hold(task.id).unwrap().is_some());
        assert_eq!(
            store
                .agent_launch_intent("restart")
                .unwrap()
                .unwrap()
                .started_at_millis,
            1000
        );
        assert_eq!(store.list_open_agent_assignments().unwrap().len(), 1);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn acknowledgement_winning_before_timeout_does_not_create_a_hold() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "ack".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "ack-before-timeout".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        store
            .begin_agent_launch("ack-before-timeout", 1000)
            .unwrap();
        store
            .acknowledge_agent_assignment("ack-before-timeout")
            .unwrap();
        assert!(
            !store
                .guard_agent_provisioning("ack-before-timeout", 301_000)
                .unwrap()
        );
        assert!(
            !store
                .hold_uncertain_agent_assignment("ack-before-timeout")
                .unwrap()
        );
        assert!(store.dispatch_hold(task.id).unwrap().is_none());
    }

    #[test]
    fn acknowledgement_is_idempotent_does_not_start_task_and_rejects_closed_assignment() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "task".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "ack-attempt".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        let first = assignment_acknowledge(&store, "ack-attempt", "cap", "first").unwrap();
        assert_eq!(first.state, forge_app::AgentAssignmentState::Working);
        assert_eq!(
            store.show_record(task.id).unwrap().status,
            RecordStatus::Open
        );
        assert_eq!(
            assignment_acknowledge(&store, "ack-attempt", "cap", "retry").unwrap(),
            first
        );
        assert!(
            store
                .compare_and_set_agent_assignment_state(
                    "ack-attempt",
                    forge_app::AgentAssignmentState::Provisioning,
                    forge_app::AgentAssignmentState::Unknown
                )
                .unwrap()
                .is_none()
        );
        assert!(
            !store
                .schedule_agent_assignment_retry(
                    "ack-attempt",
                    forge_app::AgentAssignmentState::Provisioning,
                    1,
                    1
                )
                .unwrap()
        );
        store
            .close_agent_assignment(
                "ack-attempt",
                forge_app::AgentAssignmentClosureReason::Replaced,
            )
            .unwrap();
        assert!(assignment_acknowledge(&store, "ack-attempt", "cap", "closed").is_err());
    }

    #[test]
    fn bound_assignment_capability_starts_its_open_task_idempotently() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let spec = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Spec,
                task_kind: None,
                body: "spec".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let task = store
            .open_task_under_spec("task", spec.id, None, vec![])
            .unwrap();
        let sibling = store
            .open_task_under_spec("sibling", spec.id, None, vec![])
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "attempt".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();

        assert_eq!(
            assignment_start_task(&store, "attempt", "cap", "first", task.id)
                .unwrap()
                .status,
            RecordStatus::InProgress
        );
        assert_eq!(
            assignment_start_task(&store, "attempt", "cap", "retry", task.id)
                .unwrap()
                .status,
            RecordStatus::InProgress
        );
        assert_eq!(
            assignment_start_task(&store, "attempt", "cap", "wrong", sibling.id)
                .unwrap_err()
                .code,
            "assignment-capability-denied"
        );
        assert_eq!(
            store
                .list_assignment_authorization_audits("attempt")
                .unwrap()
                .iter()
                .filter(|audit| matches!(
                    audit.operation,
                    AssignmentAuthorizationOperation::StartTask
                ))
                .count(),
            3
        );
    }

    #[test]
    fn capability_operations_are_bounded_and_audited_without_secrets_or_comment_bodies() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let spec = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Spec,
                task_kind: None,
                body: "spec".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let task = store
            .open_task_under_spec("task", spec.id, None, vec![])
            .unwrap();
        let sibling = store
            .open_task_under_spec("sibling", spec.id, None, vec![])
            .unwrap();
        store.start_record(task.id).unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "attempt".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();

        let context = assignment_context(&store, "attempt", "cap-1", "read-1").unwrap();
        assert_eq!(context.task.id, task.id);
        assert_eq!(
            context.directly_related_records,
            vec![store.show_record(spec.id).unwrap()]
        );
        assert_eq!(
            assignment_add_comment(
                &store,
                "attempt",
                "cap-1",
                "comment-1",
                sibling.id,
                "hidden"
            )
            .unwrap_err()
            .code,
            "assignment-capability-denied"
        );
        assignment_add_comment(&store, "attempt", "cap-1", "comment-2", task.id, "finished")
            .unwrap();
        check_tests::assess(&store, task.id, forge_app::checks::Verdict::Pass);
        assignment_complete_task(
            &store,
            "attempt",
            "cap-1",
            "done-1",
            task.id,
            Some("complete"),
        )
        .unwrap();
        let audit = store
            .list_assignment_authorization_audits("attempt")
            .unwrap();
        assert_eq!(audit.len(), 4);
        assert!(
            audit
                .iter()
                .all(|entry| entry.capability_id == "attempt" && !entry.reason.contains("hidden"))
        );
        assert_eq!(
            audit[1].outcome,
            forge_app::AssignmentAuthorizationOutcome::Denied
        );
    }
}
