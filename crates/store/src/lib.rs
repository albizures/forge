//! SQLite implementation of the application persistence boundary.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use forge_app::{
    AgentAssignment, AgentAssignmentClosureReason, AgentAssignmentResource, AgentAssignmentState,
    AgentDispatchRepository, AgentProfile, AssignmentAuthorizationAudit,
    AssignmentAuthorizationOperation, AssignmentAuthorizationOutcome, ManagedWorktreeRepository,
    NewAgentAssignment, NewAgentAssignmentResource, NewRecord, NewRecordUpdateSubscription,
    NewTypedRecord, Project, ProjectRepository, Record, RecordComment, RecordCommentId,
    RecordListFilter, RecordRelationship, RecordRelationshipKind, RecordRepository, RecordStatus,
    RecordType, RecordUpdate, RecordUpdateEnvelope, RecordUpdateId, RecordUpdateKind,
    RecordUpdateKindFilter, RecordUpdateSubscription, RecordUpdateSubscriptionId,
    RecordUpdateSubscriptionRepository, RegistrationError, WorktreeClaim, WorktreeClaimState,
    is_worktree_eligible, managed_worktree_path,
};
use rusqlite::{Connection, ErrorCode, OptionalExtension, params};

/// SQLite-backed Project and Record storage.
pub struct SqliteProjectRepository {
    connection: Mutex<Connection>,
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
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS projects (
                slug TEXT PRIMARY KEY NOT NULL,
                registered_folder TEXT UNIQUE NOT NULL,
                origin_url TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS records (
                id INTEGER PRIMARY KEY NOT NULL,
                record_type TEXT NOT NULL CHECK (record_type IN ('generic', 'spec', 'task')),
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
                parent_id INTEGER NOT NULL REFERENCES records(id),
                child_id INTEGER NOT NULL REFERENCES records(id) UNIQUE,
                kind TEXT NOT NULL CHECK (kind IN ('parent-child')),
                PRIMARY KEY (parent_id, child_id)
            );
            CREATE TABLE IF NOT EXISTS record_updates (
                id INTEGER PRIMARY KEY NOT NULL,
                record_id INTEGER NOT NULL REFERENCES records(id),
                created_at_millis INTEGER NOT NULL,
                kind TEXT NOT NULL CHECK (kind IN ('created', 'body-edited', 'status-transition', 'relationship-added', 'relationship-removed')),
                from_status TEXT,
                to_status TEXT,
                related_record_id INTEGER REFERENCES records(id),
                comment_id INTEGER
            );
            CREATE TABLE IF NOT EXISTS record_comments (
                id INTEGER PRIMARY KEY NOT NULL,
                record_id INTEGER NOT NULL REFERENCES records(id),
                created_at_millis INTEGER NOT NULL,
                author TEXT,
                body TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS record_update_subscriptions (
                id INTEGER PRIMARY KEY NOT NULL,
                assignment_id TEXT NOT NULL REFERENCES agent_assignments(id),
                target_key TEXT NOT NULL,
                filter_key TEXT NOT NULL,
                start_cursor INTEGER NOT NULL,
                lease_expires_at_millis INTEGER NOT NULL,
                UNIQUE (assignment_id, target_key, filter_key)
            );
            CREATE TABLE IF NOT EXISTS record_update_subscription_targets (
                subscription_id INTEGER NOT NULL REFERENCES record_update_subscriptions(id) ON DELETE CASCADE,
                record_id INTEGER NOT NULL REFERENCES records(id),
                PRIMARY KEY (subscription_id, record_id)
            );
            CREATE TABLE IF NOT EXISTS record_update_subscription_filters (
                subscription_id INTEGER NOT NULL REFERENCES record_update_subscriptions(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                PRIMARY KEY (subscription_id, kind)
            );
            CREATE INDEX IF NOT EXISTS record_updates_global_replay
                ON record_updates(id);
            CREATE INDEX IF NOT EXISTS record_update_subscriptions_expiry
                ON record_update_subscriptions(lease_expires_at_millis);
            CREATE TABLE IF NOT EXISTS agent_profiles (
                record_type TEXT NOT NULL CHECK (record_type IN ('task')),
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
            CREATE TABLE IF NOT EXISTS assignment_authorization_audits (
                id INTEGER PRIMARY KEY NOT NULL,
                assignment_id TEXT NOT NULL REFERENCES agent_assignments(id),
                capability_id TEXT NOT NULL,
                created_at_millis INTEGER NOT NULL,
                operation TEXT NOT NULL CHECK (operation IN ('read-context', 'add-comment', 'complete-task', 'create-subscription', 'renew-subscription', 'delete-subscription')),
                target_record_id INTEGER NOT NULL REFERENCES records(id),
                outcome TEXT NOT NULL CHECK (outcome IN ('allowed', 'denied')),
                reason TEXT NOT NULL,
                correlation_id TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS managed_worktree_configuration (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                worktree_root TEXT
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
                scan_lease_owner TEXT,
                scan_lease_until_millis INTEGER
            );
            CREATE TABLE IF NOT EXISTS project_agent_dispatch_configuration (
                project_slug TEXT PRIMARY KEY NOT NULL REFERENCES projects(slug),
                task_concurrency_limit INTEGER NOT NULL CHECK (task_concurrency_limit BETWEEN 1 AND 1024)
            );",
            )
            .map_err(storage_error)?;
        migrate_record_status_schema(&connection)?;
        migrate_record_type_schema(&connection)?;
        migrate_record_updates_schema(&connection)?;
        migrate_agent_assignment_schema(&connection)?;
        migrate_assignment_authorization_audit_schema(&connection)?;
        migrate_managed_worktree_schema(&connection)?;
        seed_task_agent_profile(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}

impl ProjectRepository for SqliteProjectRepository {
    fn project(&self, slug: &str) -> Result<Project, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection
            .query_row(
                "SELECT slug, registered_folder, origin_url FROM projects WHERE slug = ?1",
                [slug],
                |row| {
                    Ok(Project {
                        slug: row.get(0)?,
                        registered_folder: PathBuf::from(row.get::<_, String>(1)?),
                        origin_url: row.get(2)?,
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

    fn register(&self, project: &Project) -> Result<Project, RegistrationError> {
        let folder = project.registered_folder.to_string_lossy();
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let folders = connection
            .prepare("SELECT registered_folder FROM projects")
            .map_err(storage_error)?
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        if folders
            .iter()
            .any(|existing| folders_overlap(Path::new(existing), &project.registered_folder))
            && !folders.iter().any(|existing| existing == &folder)
        {
            return Err(RegistrationError::new(
                "folder-overlap",
                format!("Registered Folder `{folder}` overlaps an existing Project folder"),
            ));
        }
        match connection.execute(
            "INSERT INTO projects (slug, registered_folder, origin_url) VALUES (?1, ?2, ?3)",
            params![project.slug, folder, project.origin_url],
        ) {
            Ok(_) => Ok(project.clone()),
            Err(error)
                if matches!(
                    error.sqlite_error_code(),
                    Some(ErrorCode::ConstraintViolation)
                ) =>
            {
                let existing = connection.query_row("SELECT slug, registered_folder, origin_url FROM projects WHERE slug = ?1 OR registered_folder = ?2", params![project.slug, folder], |row| Ok(Project { slug: row.get(0)?, registered_folder: std::path::PathBuf::from(row.get::<_, String>(1)?), origin_url: row.get(2)? })).map_err(storage_error)?;
                if existing == *project {
                    Ok(existing)
                } else if existing.slug == project.slug {
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
                            "Registered Folder `{folder}` is already registered as `{}`",
                            existing.slug
                        ),
                    ))
                }
            }
            Err(error) => Err(storage_error(error)),
        }
    }

    fn find_containing(&self, folder: &Path) -> Result<Project, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let projects = connection
            .prepare("SELECT slug, registered_folder, origin_url FROM projects")
            .map_err(storage_error)?
            .query_map([], |row| {
                Ok(Project {
                    slug: row.get(0)?,
                    registered_folder: std::path::PathBuf::from(row.get::<_, String>(1)?),
                    origin_url: row.get(2)?,
                })
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        projects
            .into_iter()
            .find(|project| folder.starts_with(&project.registered_folder))
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

impl ManagedWorktreeRepository for SqliteProjectRepository {
    fn set_worktree_root(&self, root: Option<&Path>) -> Result<(), RegistrationError> {
        if root.is_some_and(|root| !root.is_absolute()) {
            return Err(RegistrationError::new(
                "invalid-worktree-root",
                "Worktree Root must be an absolute path",
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection.execute(
            "INSERT INTO managed_worktree_configuration (singleton, worktree_root) VALUES (1, ?1) ON CONFLICT(singleton) DO UPDATE SET worktree_root = excluded.worktree_root",
            [root.map(|path| path.to_string_lossy().into_owned())],
        ).map_err(storage_error)?;
        Ok(())
    }

    fn worktree_root(&self) -> Result<Option<PathBuf>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection
            .query_row(
                "SELECT worktree_root FROM managed_worktree_configuration WHERE singleton = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(storage_error)
            .map(|root| root.flatten().map(PathBuf::from))
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

    fn reserve_worktree_claim(&self, record_id: u64) -> Result<WorktreeClaim, RegistrationError> {
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
                "a Managed Worktree requires an In Progress Task scoped to exactly one Project",
            ));
        }
        let root: Option<String> = transaction
            .query_row(
                "SELECT worktree_root FROM managed_worktree_configuration WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?
            .flatten();
        let root = root.ok_or_else(|| {
            RegistrationError::new(
                "worktree-root-unset",
                "configure a Worktree Root before creating a Managed Worktree",
            )
        })?;
        let project_slug = record
            .project_slugs
            .into_iter()
            .next()
            .expect("eligible Record has one Project");
        let path = managed_worktree_path(Path::new(&root), &project_slug, record_id);
        transaction.execute("INSERT INTO worktree_claims (record_id, project_slug, path, state) VALUES (?1, ?2, ?3, 'reserving')", params![record_id.cast_signed(), project_slug, path.to_string_lossy()]).map_err(|error| if matches!(error.sqlite_error_code(), Some(ErrorCode::ConstraintViolation)) { RegistrationError::new("worktree-claim-exists", "Record already has a Worktree Claim") } else { storage_error(error) })?;
        let claim =
            load_worktree_claim(&transaction, record_id)?.expect("inserted Claim is visible");
        transaction.commit().map_err(storage_error)?;
        Ok(claim)
    }

    fn activate_worktree_claim(&self, record_id: u64) -> Result<WorktreeClaim, RegistrationError> {
        transition_worktree_claim(&self.connection, record_id, "reserving", "active")
    }

    fn require_worktree_claim_cleanup(
        &self,
        record_id: u64,
    ) -> Result<WorktreeClaim, RegistrationError> {
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
        connection.prepare("SELECT record_id, project_slug, path, state FROM worktree_claims ORDER BY record_id").map_err(storage_error)?.query_map([], worktree_claim_from_row).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)
    }

    fn worktree_claims_requiring_cleanup(&self) -> Result<Vec<WorktreeClaim>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection.prepare("SELECT record_id, project_slug, path, state FROM worktree_claims WHERE state = 'cleanup-required' ORDER BY record_id").map_err(storage_error)?.query_map([], worktree_claim_from_row).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)
    }

    fn delete_worktree_claim(&self, record_id: u64) -> Result<(), RegistrationError> {
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
    fn open_record(&self, record: &NewRecord) -> Result<Record, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        for slug in &record.project_slugs {
            let exists: bool = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM projects WHERE slug = ?1)",
                    [slug],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            if !exists {
                return Err(RegistrationError::new(
                    "project-not-found",
                    format!("Project `{slug}` is not registered"),
                ));
            }
        }
        transaction
            .execute(
                "INSERT INTO records (record_type, body, status) VALUES ('generic', ?1, 'open')",
                [&record.body],
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
        transaction.commit().map_err(storage_error)?;
        Ok(Record {
            id,
            record_type: RecordType::Generic,
            body: record.body.clone(),
            project_slugs: record.project_slugs.clone(),
            status: RecordStatus::Open,
            completion_resolution: None,
            relationships: vec![],
        })
    }

    fn open_typed_record(&self, record: &NewTypedRecord) -> Result<Record, RegistrationError> {
        validate_typed_record(record)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        validate_projects(&transaction, &record.project_slugs)?;
        transaction
            .execute(
                "INSERT INTO records (record_type, body, status) VALUES (?1, ?2, 'open')",
                params![record_type_name(record.record_type), record.body],
            )
            .map_err(storage_error)?;
        let id = transaction.last_insert_rowid().cast_unsigned();
        for slug in &record.project_slugs {
            transaction
                .execute(
                    "INSERT INTO record_projects (record_id, project_slug) VALUES (?1, ?2)",
                    params![id.cast_signed(), slug],
                )
                .map_err(storage_error)?;
        }
        insert_update(&transaction, id, RecordUpdateKind::Created)?;
        let opened = load_record(&transaction, id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(opened)
    }

    fn open_task_under_spec(&self, body: &str, spec_id: u64) -> Result<Record, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let spec = load_record(&transaction, spec_id)?;
        if spec.record_type != RecordType::Spec {
            return Err(RegistrationError::new(
                "invalid-parent-type",
                "a Task may be opened under a Spec only",
            ));
        }
        transaction
            .execute(
                "INSERT INTO records (record_type, body, status) VALUES ('task', ?1, 'open')",
                [body],
            )
            .map_err(storage_error)?;
        let task_id = transaction.last_insert_rowid().cast_unsigned();
        transaction
            .execute(
                "INSERT INTO record_projects (record_id, project_slug) SELECT ?1, project_slug FROM record_projects WHERE record_id = ?2",
                params![task_id.cast_signed(), spec_id.cast_signed()],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "INSERT INTO record_relationships (parent_id, child_id, kind) VALUES (?1, ?2, 'parent-child')",
                params![spec_id.cast_signed(), task_id.cast_signed()],
            )
            .map_err(storage_error)?;
        insert_update(&transaction, task_id, RecordUpdateKind::Created)?;
        insert_update(
            &transaction,
            task_id,
            RecordUpdateKind::RelationshipAdded {
                related_record_id: spec_id,
            },
        )?;
        let task = load_record(&transaction, task_id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(task)
    }

    fn list_records(&self, filter: &RecordListFilter) -> Result<Vec<Record>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let ids = match (&filter.project_slug, filter.status) {
            (Some(project), Some(status)) => connection.prepare("SELECT record.id FROM records record JOIN record_projects scope ON scope.record_id = record.id WHERE scope.project_slug = ?1 AND record.status = ?2 ORDER BY record.id DESC").map_err(storage_error)?.query_map(params![project, status_name(status)], |row| row.get(0)).map_err(storage_error)?.collect::<Result<Vec<i64>, _>>().map_err(storage_error)?,
            (Some(project), None) => connection.prepare("SELECT record.id FROM records record JOIN record_projects scope ON scope.record_id = record.id WHERE scope.project_slug = ?1 ORDER BY record.id DESC").map_err(storage_error)?.query_map([project], |row| row.get(0)).map_err(storage_error)?.collect::<Result<Vec<i64>, _>>().map_err(storage_error)?,
            (None, Some(status)) => connection.prepare("SELECT id FROM records WHERE status = ?1 ORDER BY id DESC").map_err(storage_error)?.query_map([status_name(status)], |row| row.get(0)).map_err(storage_error)?.collect::<Result<Vec<i64>, _>>().map_err(storage_error)?,
            (None, None) => connection.prepare("SELECT id FROM records ORDER BY id DESC").map_err(storage_error)?.query_map([], |row| row.get(0)).map_err(storage_error)?.collect::<Result<Vec<i64>, _>>().map_err(storage_error)?,
        };
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
            })
            .collect())
    }

    fn show_record(&self, id: u64) -> Result<Record, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        load_record(&connection, id)
    }

    fn edit_record(&self, id: u64, body: &str) -> Result<Record, RegistrationError> {
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
        transition_record(&self.connection, id, "open", "in_progress", None)
    }

    fn done_record(
        &self,
        id: u64,
        completion_resolution: Option<&str>,
    ) -> Result<Record, RegistrationError> {
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

    fn relate_records(&self, child_id: u64, parent_id: u64) -> Result<Record, RegistrationError> {
        if child_id == parent_id {
            return Err(RegistrationError::new(
                "invalid-relationship",
                "a Record cannot be its own parent",
            ));
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let child = load_record(&transaction, child_id)?;
        let parent = load_record(&transaction, parent_id)?;
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
            "WITH RECURSIVE descendants(id) AS (SELECT child_id FROM record_relationships WHERE parent_id = ?1 UNION SELECT relationship.child_id FROM record_relationships relationship JOIN descendants ON relationship.parent_id = descendants.id) SELECT EXISTS(SELECT 1 FROM descendants WHERE id = ?2)",
            params![child_id.cast_signed(), parent_id.cast_signed()], |row| row.get(0)
        ).map_err(storage_error)?;
        if cycle {
            return Err(RegistrationError::new(
                "relationship-cycle",
                "parent/child Relationships cannot form a cycle",
            ));
        }
        match transaction.execute("INSERT INTO record_relationships (parent_id, child_id, kind) VALUES (?1, ?2, 'parent-child')", params![parent_id.cast_signed(), child_id.cast_signed()]) {
            Ok(_) => {},
            Err(error) if matches!(error.sqlite_error_code(), Some(ErrorCode::ConstraintViolation)) => return Err(RegistrationError::new("child-already-related", "a child Record can have only one parent")),
            Err(error) => return Err(storage_error(error)),
        }
        insert_update(
            &transaction,
            child_id,
            RecordUpdateKind::RelationshipAdded {
                related_record_id: parent_id,
            },
        )?;
        let record = load_record(&transaction, child_id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(record)
    }

    fn unrelate_records(&self, child_id: u64, parent_id: u64) -> Result<Record, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let child = load_record(&transaction, child_id)?;
        let parent = load_record(&transaction, parent_id)?;
        if child.record_type != RecordType::Generic || parent.record_type != RecordType::Generic {
            return Err(RegistrationError::new(
                "invalid-relationship",
                "only Generic Record Relationships may be removed",
            ));
        }
        let deleted = transaction
            .execute(
                "DELETE FROM record_relationships WHERE parent_id = ?1 AND child_id = ?2",
                params![parent_id.cast_signed(), child_id.cast_signed()],
            )
            .map_err(storage_error)?;
        if deleted == 0 {
            return Err(RegistrationError::new(
                "relationship-not-found",
                "parent/child Relationship does not exist",
            ));
        }
        insert_update(
            &transaction,
            child_id,
            RecordUpdateKind::RelationshipRemoved {
                related_record_id: parent_id,
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

    fn list_record_updates(&self, id: u64) -> Result<Vec<RecordUpdate>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        load_record(&connection, id)?;
        connection.prepare("SELECT id, created_at_millis, kind, from_status, to_status, related_record_id, comment_id FROM record_updates WHERE record_id = ?1 ORDER BY created_at_millis DESC, id DESC")
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
        "CREATE TABLE IF NOT EXISTS managed_worktree_configuration (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1), worktree_root TEXT
        );
        CREATE TABLE IF NOT EXISTS project_managed_worktree_configuration (
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
    connection.execute_batch("ALTER TABLE agent_dispatch_configuration ADD COLUMN scan_lease_owner TEXT; ALTER TABLE agent_dispatch_configuration ADD COLUMN scan_lease_until_millis INTEGER;").or_else(|error| {
        if error.to_string().contains("duplicate column name") { Ok(()) } else { Err(error) }
    }).map_err(storage_error)?;
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
    if schema.contains("create-subscription") {
        return Ok(());
    }
    connection.execute_batch(
        "ALTER TABLE assignment_authorization_audits RENAME TO assignment_authorization_audits_old;
        CREATE TABLE assignment_authorization_audits (
            id INTEGER PRIMARY KEY NOT NULL,
            assignment_id TEXT NOT NULL REFERENCES agent_assignments(id),
            capability_id TEXT NOT NULL,
            created_at_millis INTEGER NOT NULL,
            operation TEXT NOT NULL CHECK (operation IN ('read-context', 'add-comment', 'complete-task', 'create-subscription', 'renew-subscription', 'delete-subscription')),
            target_record_id INTEGER NOT NULL REFERENCES records(id),
            outcome TEXT NOT NULL CHECK (outcome IN ('allowed', 'denied')),
            reason TEXT NOT NULL,
            correlation_id TEXT NOT NULL
        );
        INSERT INTO assignment_authorization_audits SELECT * FROM assignment_authorization_audits_old;
        DROP TABLE assignment_authorization_audits_old;",
    ).map_err(storage_error)
}

fn seed_task_agent_profile(connection: &Connection) -> Result<(), RegistrationError> {
    connection.execute(
        "INSERT OR IGNORE INTO agent_profiles (record_type, revision, provider, agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis) VALUES ('task', 1, 'herdr', 'pi', 'gpt-5.6-terra', 'agent-dispatch/task/v1', 3, 1000, 30000, 60000)",
        [],
    ).map_err(storage_error)?;
    Ok(())
}

fn load_agent_profile(
    connection: &Connection,
    profile_record_type: RecordType,
    revision: u32,
) -> Result<AgentProfile, RegistrationError> {
    connection.query_row(
        "SELECT record_type, revision, provider, agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis FROM agent_profiles WHERE record_type = ?1 AND revision = ?2",
        params![record_type_name(profile_record_type), revision],
        |row| Ok(AgentProfile {
            record_type: match row.get::<_, String>(0)?.as_str() {
                "task" => RecordType::Task,
                _ => return Err(rusqlite::Error::InvalidQuery),
            },
            revision: row.get::<_, i64>(1)?.cast_unsigned().try_into().expect("profile revision fits u32"),
            provider: row.get(2)?, agent_kind: row.get(3)?, model: row.get(4)?, prompt_template: row.get(5)?,
            retry_limit: row.get::<_, i64>(6)?.cast_unsigned().try_into().expect("retry limit fits u32"), retry_backoff_millis: row.get::<_, i64>(7)?.cast_unsigned(),
            startup_timeout_millis: row.get::<_, i64>(8)?.cast_unsigned(), unknown_state_grace_millis: row.get::<_, i64>(9)?.cast_unsigned(),
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
        if record.record_type != RecordType::Task
            || record.project_slugs.as_slice() != [assignment.project_slug.as_str()]
        {
            return Err(RegistrationError::new(
                "invalid-assignment-record",
                "an Assignment requires a Task scoped to its Project",
            ));
        }
        let profile =
            load_agent_profile(&transaction, RecordType::Task, assignment.profile_revision)?;
        let attempt_number: u32 = transaction.query_row(
            "SELECT COALESCE(MAX(attempt_number), 0) + 1 FROM agent_assignments WHERE record_id = ?1",
            [assignment.record_id.cast_signed()], |row| row.get::<_, i64>(0),
        ).map_err(storage_error)?.cast_unsigned().try_into().expect("attempt number fits u32");
        let claimed_at_millis = now_millis()?;
        match transaction.execute(
            "INSERT INTO agent_assignments (id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state_changed_at_millis) VALUES (?1, ?2, ?3, 'task', ?4, ?5, ?6, ?6)",
            params![assignment.id, assignment.record_id.cast_signed(), assignment.project_slug, assignment.profile_revision, attempt_number, claimed_at_millis],
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

    fn agent_assignment(&self, assignment_id: &str) -> Result<AgentAssignment, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        connection.query_row("SELECT id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state, closed_at_millis, closure_reason, state_changed_at_millis, next_retry_at_millis, retry_count FROM agent_assignments WHERE id = ?1", [assignment_id], |row| agent_assignment_from_row(row, &connection))
            .map_err(|error| match error { rusqlite::Error::QueryReturnedNoRows => RegistrationError::new("assignment-not-found", "Agent Assignment does not exist"), _ => storage_error(error) })
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

    fn claim_next_agent_assignment(
        &self,
        project_slug: &str,
        assignment_id: &str,
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
            let used: u16 = connection.query_row("SELECT COUNT(*) FROM agent_assignments WHERE project_slug = ?1 AND closed_at_millis IS NULL", [project_slug], |row| row.get::<_, i64>(0)).map_err(storage_error)?.cast_unsigned().try_into().expect("assignment count fits u16");
            if used >= limit {
                return Ok(None);
            }
            let record_id = connection.query_row("SELECT record.id FROM records record JOIN record_projects scope ON scope.record_id = record.id WHERE record.record_type = 'task' AND (record.status = 'open' OR (record.status = 'in_progress' AND EXISTS (SELECT 1 FROM agent_assignments prior WHERE prior.record_id = record.id AND prior.closed_at_millis IS NOT NULL))) AND scope.project_slug = ?1 AND (SELECT COUNT(*) FROM record_projects only_scope WHERE only_scope.record_id = record.id) = 1 AND NOT EXISTS (SELECT 1 FROM agent_assignments assignment WHERE assignment.record_id = record.id AND assignment.closed_at_millis IS NULL) ORDER BY record.id LIMIT 1", [project_slug], |row| row.get::<_, i64>(0).map(i64::cast_unsigned)).optional().map_err(storage_error)?;
            let Some(record_id) = record_id else {
                return Ok(None);
            };
            let profile = load_agent_profile(&connection, RecordType::Task, 1)?;
            let attempt: u32 = connection.query_row("SELECT COALESCE(MAX(attempt_number), 0) + 1 FROM agent_assignments WHERE record_id = ?1", [record_id.cast_signed()], |row| row.get::<_, i64>(0)).map_err(storage_error)?.cast_unsigned().try_into().expect("attempt number fits u32");
            let claimed_at_millis = now_millis()?;
            connection.execute("INSERT INTO agent_assignments (id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state, state_changed_at_millis) VALUES (?1, ?2, ?3, 'task', 1, ?4, ?5, 'provisioning', ?5)", params![assignment_id, record_id.cast_signed(), project_slug, attempt, claimed_at_millis]).map_err(|error| if matches!(error.sqlite_error_code(), Some(ErrorCode::ConstraintViolation)) { RegistrationError::new("assignment-conflict", "Agent Assignment already exists or Task is assigned") } else { storage_error(error) })?;
            let changed = connection
                .execute(
                    "UPDATE records SET status = 'in_progress' WHERE id = ?1 AND status = 'open'",
                    [record_id.cast_signed()],
                )
                .map_err(storage_error)?;
            if changed > 1 {
                return Err(RegistrationError::new(
                    "claim-conflict",
                    "Task was no longer eligible to claim",
                ));
            }
            if changed == 1 {
                insert_update(
                    &connection,
                    record_id,
                    RecordUpdateKind::StatusTransition {
                        from: RecordStatus::Open,
                        to: RecordStatus::InProgress,
                    },
                )?;
            }
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
        transaction
            .execute(
                "DELETE FROM record_update_subscriptions WHERE assignment_id = ?1",
                [assignment_id],
            )
            .map_err(storage_error)?;
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
        if changed == 1 {
            transaction
                .execute(
                    "DELETE FROM record_update_subscriptions WHERE assignment_id = ?1",
                    [assignment_id],
                )
                .map_err(storage_error)?;
        }
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
            AssignmentAuthorizationOperation::CreateSubscription => "create-subscription",
            AssignmentAuthorizationOperation::RenewSubscription => "renew-subscription",
            AssignmentAuthorizationOperation::DeleteSubscription => "delete-subscription",
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
                let operation = match row.get::<_, String>(3)?.as_str() { "read-context" => AssignmentAuthorizationOperation::ReadContext, "add-comment" => AssignmentAuthorizationOperation::AddComment, "complete-task" => AssignmentAuthorizationOperation::CompleteTask, "create-subscription" => AssignmentAuthorizationOperation::CreateSubscription, "renew-subscription" => AssignmentAuthorizationOperation::RenewSubscription, "delete-subscription" => AssignmentAuthorizationOperation::DeleteSubscription, _ => return Err(rusqlite::Error::InvalidQuery) };
                let outcome = match row.get::<_, String>(5)?.as_str() { "allowed" => AssignmentAuthorizationOutcome::Allowed, "denied" => AssignmentAuthorizationOutcome::Denied, _ => return Err(rusqlite::Error::InvalidQuery) };
                Ok(AssignmentAuthorizationAudit { assignment_id: row.get(0)?, capability_id: row.get(1)?, created_at_millis: row.get::<_, i64>(2)?.cast_unsigned(), operation, target_record_id: row.get::<_, i64>(4)?.cast_unsigned(), outcome, reason: row.get(6)?, correlation_id: row.get(7)? })
            }).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)
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
        connection.execute("INSERT INTO agent_dispatch_configuration (singleton, herdr_socket_path) VALUES (1, ?1) ON CONFLICT(singleton) DO UPDATE SET herdr_socket_path = excluded.herdr_socket_path", [path.map(Path::to_string_lossy).as_deref()]).map_err(storage_error)?;
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

impl RecordUpdateSubscriptionRepository for SqliteProjectRepository {
    fn create_record_update_subscription(
        &self,
        subscription: &NewRecordUpdateSubscription,
    ) -> Result<RecordUpdateSubscription, RegistrationError> {
        let targets = normalized_targets(&subscription.target_record_ids)?;
        let filters = normalized_filters(&subscription.kind_filters);
        let target_key = targets
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let filter_key = filters
            .iter()
            .map(|filter| filter_name(*filter))
            .collect::<Vec<_>>()
            .join(",");
        let now = now_millis()?;
        let expires = now.checked_add(SUBSCRIPTION_LEASE_MILLIS).ok_or_else(|| {
            RegistrationError::new("storage", "subscription lease is out of range")
        })?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction
            .execute(
                "DELETE FROM record_update_subscriptions WHERE lease_expires_at_millis <= ?1",
                [now],
            )
            .map_err(storage_error)?;
        let assignment_exists: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM agent_assignments WHERE id = ?1 AND closed_at_millis IS NULL)", [&subscription.assignment_id], |row| row.get(0)).map_err(storage_error)?;
        if !assignment_exists {
            return Err(RegistrationError::new(
                "assignment-not-found",
                "Assignment does not exist or is closed",
            ));
        }
        if let Some(existing) = transaction.query_row(
            "SELECT id, start_cursor, lease_expires_at_millis FROM record_update_subscriptions WHERE assignment_id = ?1 AND target_key = ?2 AND filter_key = ?3",
            params![subscription.assignment_id, target_key, filter_key],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
        ).optional().map_err(storage_error)? {
            let result = subscription_from_parts(existing.0.cast_unsigned(), subscription.assignment_id.clone(), targets, filters, existing.1.cast_unsigned(), existing.2.cast_unsigned());
            transaction.commit().map_err(storage_error)?;
            return Ok(result);
        }
        let cursor: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(id), 0) FROM record_updates",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        transaction.execute("INSERT INTO record_update_subscriptions (assignment_id, target_key, filter_key, start_cursor, lease_expires_at_millis) VALUES (?1, ?2, ?3, ?4, ?5)", params![subscription.assignment_id, target_key, filter_key, cursor, expires]).map_err(storage_error)?;
        let id = transaction.last_insert_rowid().cast_unsigned();
        for target in &targets {
            transaction.execute("INSERT INTO record_update_subscription_targets (subscription_id, record_id) VALUES (?1, ?2)", params![id.cast_signed(), target.cast_signed()]).map_err(storage_error)?;
        }
        for filter in &filters {
            transaction.execute("INSERT INTO record_update_subscription_filters (subscription_id, kind) VALUES (?1, ?2)", params![id.cast_signed(), filter_name(*filter)]).map_err(storage_error)?;
        }
        transaction.commit().map_err(storage_error)?;
        Ok(subscription_from_parts(
            id,
            subscription.assignment_id.clone(),
            targets,
            filters,
            cursor.cast_unsigned(),
            expires.cast_unsigned(),
        ))
    }

    fn record_update_subscription(
        &self,
        id: RecordUpdateSubscriptionId,
    ) -> Result<RecordUpdateSubscription, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let now = now_millis()?;
        match load_subscription(&connection, id, now) {
            Ok(subscription) => Ok(subscription),
            Err(error) if error.code == "subscription-not-found" => {
                let expired: bool = connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM record_update_subscriptions WHERE id = ?1 AND lease_expires_at_millis <= ?2)",
                        params![id.0.cast_signed(), now],
                        |row| row.get(0),
                    )
                    .map_err(storage_error)?;
                if expired {
                    Err(RegistrationError::new(
                        "subscription-expired",
                        "Subscription lease has expired",
                    ))
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    }

    fn renew_record_update_subscription(
        &self,
        id: RecordUpdateSubscriptionId,
    ) -> Result<RecordUpdateSubscription, RegistrationError> {
        let now = now_millis()?;
        let expires = now.checked_add(SUBSCRIPTION_LEASE_MILLIS).ok_or_else(|| {
            RegistrationError::new("storage", "subscription lease is out of range")
        })?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let changed = transaction.execute("UPDATE record_update_subscriptions SET lease_expires_at_millis = ?1 WHERE id = ?2 AND lease_expires_at_millis > ?3", params![expires, id.0.cast_signed(), now]).map_err(storage_error)?;
        if changed == 0 {
            return Err(RegistrationError::new(
                "subscription-not-found",
                "Subscription does not exist or has expired",
            ));
        }
        let subscription = load_subscription(&transaction, id, now)?;
        transaction.commit().map_err(storage_error)?;
        Ok(subscription)
    }

    fn delete_record_update_subscription(
        &self,
        id: RecordUpdateSubscriptionId,
    ) -> Result<(), RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let deleted = connection
            .execute(
                "DELETE FROM record_update_subscriptions WHERE id = ?1",
                [id.0.cast_signed()],
            )
            .map_err(storage_error)?;
        if deleted == 0 {
            return Err(RegistrationError::new(
                "subscription-not-found",
                "Subscription does not exist",
            ));
        }
        Ok(())
    }

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

    fn replay_record_updates(
        &self,
        id: RecordUpdateSubscriptionId,
        after: RecordUpdateId,
    ) -> Result<Vec<RecordUpdateEnvelope>, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let subscription = load_subscription(&connection, id, now_millis()?)?;
        if after < subscription.start_cursor {
            return Err(RegistrationError::new(
                "invalid-cursor",
                "cursor predates the Subscription creation cursor",
            ));
        }
        let updates = connection.prepare("SELECT id, record_id, created_at_millis, kind, from_status, to_status, related_record_id, comment_id FROM record_updates WHERE id > ?1 ORDER BY id ASC").map_err(storage_error)?.query_map([after.0.cast_signed()], update_envelope_from_row).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
        Ok(updates
            .into_iter()
            .filter(|update| {
                subscription
                    .target_record_ids
                    .binary_search(&update.record_id)
                    .is_ok()
                    && (subscription.kind_filters.is_empty()
                        || subscription
                            .kind_filters
                            .contains(&kind_filter(&update.update.kind)))
            })
            .collect())
    }
}

const SUBSCRIPTION_LEASE_MILLIS: i64 = 30 * 60 * 1000;

fn normalized_targets(targets: &[u64]) -> Result<Vec<u64>, RegistrationError> {
    let mut normalized = targets.to_vec();
    normalized.sort_unstable();
    normalized.dedup();
    if normalized.is_empty() {
        return Err(RegistrationError::new(
            "invalid-subscription-targets",
            "Subscription must select at least one Record",
        ));
    }
    Ok(normalized)
}
fn normalized_filters(filters: &[RecordUpdateKindFilter]) -> Vec<RecordUpdateKindFilter> {
    let mut normalized = filters.to_vec();
    normalized.sort_unstable();
    normalized.dedup();
    normalized
}
fn filter_name(filter: RecordUpdateKindFilter) -> &'static str {
    match filter {
        RecordUpdateKindFilter::Created => "created",
        RecordUpdateKindFilter::BodyEdited => "body-edited",
        RecordUpdateKindFilter::StatusTransition => "status-transition",
        RecordUpdateKindFilter::RelationshipAdded => "relationship-added",
        RecordUpdateKindFilter::RelationshipRemoved => "relationship-removed",
        RecordUpdateKindFilter::CommentAdded => "comment-added",
        RecordUpdateKindFilter::CommentDeleted => "comment-deleted",
    }
}
fn kind_filter(kind: &RecordUpdateKind) -> RecordUpdateKindFilter {
    match kind {
        RecordUpdateKind::Created => RecordUpdateKindFilter::Created,
        RecordUpdateKind::BodyEdited => RecordUpdateKindFilter::BodyEdited,
        RecordUpdateKind::StatusTransition { .. } => RecordUpdateKindFilter::StatusTransition,
        RecordUpdateKind::RelationshipAdded { .. } => RecordUpdateKindFilter::RelationshipAdded,
        RecordUpdateKind::RelationshipRemoved { .. } => RecordUpdateKindFilter::RelationshipRemoved,
        RecordUpdateKind::CommentAdded { .. } => RecordUpdateKindFilter::CommentAdded,
        RecordUpdateKind::CommentDeleted { .. } => RecordUpdateKindFilter::CommentDeleted,
    }
}
fn subscription_from_parts(
    id: u64,
    assignment_id: String,
    target_record_ids: Vec<u64>,
    kind_filters: Vec<RecordUpdateKindFilter>,
    start_cursor: u64,
    lease_expires_at_millis: u64,
) -> RecordUpdateSubscription {
    RecordUpdateSubscription {
        id: RecordUpdateSubscriptionId(id),
        assignment_id,
        target_record_ids,
        kind_filters,
        start_cursor: RecordUpdateId(start_cursor),
        lease_expires_at_millis,
    }
}

fn load_subscription(
    connection: &Connection,
    id: RecordUpdateSubscriptionId,
    now: i64,
) -> Result<RecordUpdateSubscription, RegistrationError> {
    let (assignment_id, start_cursor, expires): (String, i64, i64) = connection.query_row("SELECT subscription.assignment_id, subscription.start_cursor, subscription.lease_expires_at_millis FROM record_update_subscriptions subscription JOIN agent_assignments assignment ON assignment.id = subscription.assignment_id WHERE subscription.id = ?1 AND subscription.lease_expires_at_millis > ?2 AND assignment.closed_at_millis IS NULL", params![id.0.cast_signed(), now], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional().map_err(storage_error)?.ok_or_else(|| RegistrationError::new("subscription-not-found", "Subscription does not exist or has expired"))?;
    let targets = connection.prepare("SELECT record_id FROM record_update_subscription_targets WHERE subscription_id = ?1 ORDER BY record_id").map_err(storage_error)?.query_map([id.0.cast_signed()], |row| row.get::<_, i64>(0)).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?.into_iter().map(i64::cast_unsigned).collect();
    let filters = connection.prepare("SELECT kind FROM record_update_subscription_filters WHERE subscription_id = ?1 ORDER BY kind").map_err(storage_error)?.query_map([id.0.cast_signed()], |row| row.get::<_, String>(0)).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?.into_iter().map(|kind| match kind.as_str() { "created" => Ok(RecordUpdateKindFilter::Created), "body-edited" => Ok(RecordUpdateKindFilter::BodyEdited), "status-transition" => Ok(RecordUpdateKindFilter::StatusTransition), "relationship-added" => Ok(RecordUpdateKindFilter::RelationshipAdded), "relationship-removed" => Ok(RecordUpdateKindFilter::RelationshipRemoved), "comment-added" => Ok(RecordUpdateKindFilter::CommentAdded), "comment-deleted" => Ok(RecordUpdateKindFilter::CommentDeleted), _ => Err(RegistrationError::new("storage", "Subscription has an invalid update filter")) }).collect::<Result<Vec<_>, _>>()?;
    Ok(subscription_from_parts(
        id.0,
        assignment_id,
        targets,
        filters,
        start_cursor.cast_unsigned(),
        expires.cast_unsigned(),
    ))
}

fn status_name(status: RecordStatus) -> &'static str {
    match status {
        RecordStatus::Open => "open",
        RecordStatus::InProgress => "in_progress",
        RecordStatus::Done => "done",
    }
}

#[allow(clippy::needless_pass_by_value)]
fn insert_update(
    connection: &Connection,
    record_id: u64,
    kind: RecordUpdateKind,
) -> Result<(), RegistrationError> {
    let (kind, from_status, to_status, related_record_id, comment_id) = match kind {
        RecordUpdateKind::Created => ("created", None, None, None, None),
        RecordUpdateKind::BodyEdited => ("body-edited", None, None, None, None),
        RecordUpdateKind::StatusTransition { from, to } => (
            "status-transition",
            Some(status_name(from)),
            Some(status_name(to)),
            None,
            None,
        ),
        RecordUpdateKind::RelationshipAdded { related_record_id } => (
            "relationship-added",
            None,
            None,
            Some(related_record_id.cast_signed()),
            None,
        ),
        RecordUpdateKind::RelationshipRemoved { related_record_id } => (
            "relationship-removed",
            None,
            None,
            Some(related_record_id.cast_signed()),
            None,
        ),
        RecordUpdateKind::CommentAdded { comment_id } => (
            "comment-added",
            None,
            None,
            None,
            Some(comment_id.0.cast_signed()),
        ),
        RecordUpdateKind::CommentDeleted { comment_id } => (
            "comment-deleted",
            None,
            None,
            None,
            Some(comment_id.0.cast_signed()),
        ),
    };
    let created_at_millis = now_millis()?;
    connection
        .execute(
            "INSERT INTO record_updates (record_id, created_at_millis, kind, from_status, to_status, related_record_id, comment_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![record_id.cast_signed(), created_at_millis, kind, from_status, to_status, related_record_id, comment_id],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn update_envelope_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordUpdateEnvelope> {
    Ok(RecordUpdateEnvelope {
        record_id: row.get::<_, i64>(1)?.cast_unsigned(),
        update: update_from_row_at(row, 0, 2)?,
    })
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
    let status = |value: String| match value.as_str() {
        "open" => Ok(RecordStatus::Open),
        "in_progress" => Ok(RecordStatus::InProgress),
        "done" => Ok(RecordStatus::Done),
        _ => Err(rusqlite::Error::InvalidQuery),
    };
    let kind = match kind.as_str() {
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
        },
        "relationship-removed" => RecordUpdateKind::RelationshipRemoved {
            related_record_id: related_record_id
                .ok_or(rusqlite::Error::InvalidQuery)?
                .cast_unsigned(),
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

fn transition_record(
    connection: &Mutex<Connection>,
    id: u64,
    from: &str,
    to: &str,
    completion_resolution: Option<&str>,
) -> Result<Record, RegistrationError> {
    let mut connection = connection
        .lock()
        .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
    let transaction = connection.transaction().map_err(storage_error)?;
    let updated = transaction
        .execute(
            "UPDATE records SET status = ?1, completion_resolution = ?2 WHERE id = ?3 AND status = ?4",
            params![to, completion_resolution, id.cast_signed(), from],
        )
        .map_err(storage_error)?;
    if updated == 0 {
        return record_or_transition_error(&transaction, id);
    }
    let from = if from == "open" {
        RecordStatus::Open
    } else {
        RecordStatus::InProgress
    };
    let to = if to == "in_progress" {
        RecordStatus::InProgress
    } else {
        RecordStatus::Done
    };
    insert_update(
        &transaction,
        id,
        RecordUpdateKind::StatusTransition { from, to },
    )?;
    let record = load_record(&transaction, id)?;
    transaction.commit().map_err(storage_error)?;
    Ok(record)
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
    for slug in project_slugs {
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE slug = ?1)",
                [slug],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !exists {
            return Err(RegistrationError::new(
                "project-not-found",
                format!("Project `{slug}` is not registered"),
            ));
        }
    }
    Ok(())
}

fn validate_typed_record(record: &NewTypedRecord) -> Result<(), RegistrationError> {
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
        RecordType::Spec | RecordType::Task => Ok(()),
    }
}

fn record_type_name(record_type: RecordType) -> &'static str {
    match record_type {
        RecordType::Generic => "generic",
        RecordType::Spec => "spec",
        RecordType::Task => "task",
    }
}

fn record_type(value: &str) -> Result<RecordType, RegistrationError> {
    match value {
        "generic" => Ok(RecordType::Generic),
        "spec" => Ok(RecordType::Spec),
        "task" => Ok(RecordType::Task),
        _ => Err(RegistrationError::new(
            "storage",
            "stored Record has an invalid type",
        )),
    }
}

fn load_record(connection: &Connection, id: u64) -> Result<Record, RegistrationError> {
    let (stored_record_type, body, status, completion_resolution) = connection
        .query_row(
            "SELECT record_type, body, status, completion_resolution FROM records WHERE id = ?1",
            [id.cast_signed()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get(3)?,
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
        .prepare("SELECT parent_id, child_id, kind FROM record_relationships WHERE parent_id = ?1 OR child_id = ?1 ORDER BY parent_id, child_id")
        .map_err(storage_error)?
        .query_map([id.cast_signed()], |row| {
            let kind: String = row.get(2)?;
            Ok((row.get::<_, i64>(0)?.cast_unsigned(), row.get::<_, i64>(1)?.cast_unsigned(), kind))
        })
        .map_err(storage_error)?
        .map(|row| match row.map_err(storage_error)? {
            (parent_id, child_id, kind) if kind == "parent-child" => Ok(RecordRelationship { kind: RecordRelationshipKind::ParentChild, parent_id, child_id }),
            _ => Err(RegistrationError::new("storage", "stored Record has an invalid Relationship kind")),
        })
        .collect::<Result<Vec<_>, RegistrationError>>()?;
    Ok(Record {
        id,
        record_type: record_type(&stored_record_type)?,
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
    let has_record_type: bool = connection
        .prepare("PRAGMA table_info(records)")
        .map_err(storage_error)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?
        .iter()
        .any(|column| column == "record_type");
    if !has_record_type {
        connection
            .execute_batch(
                "ALTER TABLE records ADD COLUMN record_type TEXT NOT NULL DEFAULT 'generic' CHECK (record_type IN ('generic', 'spec', 'task'));",
            )
            .map_err(storage_error)?;
    }
    Ok(())
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

fn folders_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}
#[allow(clippy::needless_pass_by_value)]
fn storage_error(error: rusqlite::Error) -> RegistrationError {
    RegistrationError::new("storage", format!("could not persist Forge data: {error}"))
}

#[cfg(test)]
mod tests {
    use super::SqliteProjectRepository;
    use forge_app::{
        AgentDispatchRepository, ManagedWorktreeRepository, NewAgentAssignment,
        NewAgentAssignmentResource, NewRecord, NewRecordUpdateSubscription, NewTypedRecord,
        Project, ProjectRepository, RecordListFilter, RecordRepository, RecordStatus, RecordType,
        RecordUpdateKind, RecordUpdateKindFilter, RecordUpdateSubscriptionRepository,
        assignment_add_comment, assignment_complete_task, assignment_context,
        assignment_create_record_update_subscription, assignment_delete_record_update_subscription,
        assignment_renew_record_update_subscription,
    };
    use std::{
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };
    fn project(slug: &str, folder: &str) -> Project {
        Project {
            slug: slug.into(),
            registered_folder: PathBuf::from(folder),
            origin_url: "https://example.test/repo.git".into(),
        }
    }

    #[test]
    fn finds_a_project_from_its_registered_folder_or_descendant() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let alpha = project("alpha", "/repos/alpha");
        store.register(&alpha).unwrap();
        assert_eq!(
            store.find_containing(Path::new("/repos/alpha")).unwrap(),
            alpha
        );
        assert_eq!(
            store
                .find_containing(Path::new("/repos/alpha/src/module"))
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
        assert_eq!(
            store
                .register(&project("nested", "/repos/alpha/tools"))
                .unwrap_err()
                .code,
            "folder-overlap"
        );
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
            })
            .unwrap();
        let both = store
            .open_record(&NewRecord {
                body: "both".into(),
                project_slugs: vec!["alpha".into(), "beta".into()],
            })
            .unwrap();
        let unscoped = store
            .open_record(&NewRecord {
                body: "unscoped".into(),
                project_slugs: vec![],
            })
            .unwrap();
        store.start_record(both.id).unwrap();

        assert_eq!(
            store
                .list_records(&RecordListFilter {
                    project_slug: Some("alpha".into()),
                    status: None,
                    record_type: None,
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
            })
            .unwrap();
        let child = store
            .open_record(&NewRecord {
                body: "child".into(),
                project_slugs: vec![],
            })
            .unwrap();
        let grandchild = store
            .open_record(&NewRecord {
                body: "grandchild".into(),
                project_slugs: vec![],
            })
            .unwrap();
        assert_eq!(
            store
                .relate_records(child.id, parent.id)
                .unwrap()
                .relationships
                .len(),
            1
        );
        store.relate_records(grandchild.id, child.id).unwrap();
        assert_eq!(
            store
                .relate_records(parent.id, grandchild.id)
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
                .unrelate_records(child.id, parent.id)
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
            })
            .unwrap();
        let child = store
            .open_record(&NewRecord {
                body: "child".into(),
                project_slugs: vec![],
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
        store.relate_records(child.id, parent.id).unwrap();
        store.unrelate_records(child.id, parent.id).unwrap();

        let updates = store.list_record_updates(child.id).unwrap();
        assert_eq!(updates.len(), 6);
        assert!(
            updates.windows(2).all(|pair| pair[0].id > pair[1].id
                || pair[0].created_at_millis > pair[1].created_at_millis)
        );
        assert!(
            matches!(updates[0].kind, RecordUpdateKind::RelationshipRemoved { related_record_id } if related_record_id == parent.id)
        );
        assert!(
            matches!(updates[1].kind, RecordUpdateKind::RelationshipAdded { related_record_id } if related_record_id == parent.id)
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
                    project_slugs: vec![]
                })
                .unwrap()
                .status,
            RecordStatus::Open
        );
        let record = store
            .open_record(&NewRecord {
                body: "scoped".into(),
                project_slugs: vec!["alpha".into()],
            })
            .unwrap();
        assert_eq!(record.project_slugs, ["alpha"]);
        assert_eq!(
            store
                .open_record(&NewRecord {
                    body: "bad".into(),
                    project_slugs: vec!["missing".into()]
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
                body: "spec".into(),
                project_slugs: vec!["alpha".into()],
            })
            .unwrap();
        let task = store.open_task_under_spec("task", spec.id).unwrap();
        assert_eq!(task.record_type, RecordType::Task);
        assert_eq!(task.project_slugs, ["alpha"]);
        assert_eq!(task.relationships[0].parent_id, spec.id);
        assert_eq!(
            store
                .list_records(&RecordListFilter {
                    project_slug: None,
                    status: None,
                    record_type: Some(RecordType::Task),
                })
                .unwrap(),
            vec![task.clone()]
        );
        assert_eq!(
            store
                .open_task_under_spec("nope", task.id)
                .unwrap_err()
                .code,
            "invalid-parent-type"
        );
        assert_eq!(
            store
                .list_records(&RecordListFilter {
                    project_slug: None,
                    status: None,
                    record_type: None
                })
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            store.relate_records(task.id, spec.id).unwrap_err().code,
            "invalid-relationship"
        );
        assert_eq!(
            store.unrelate_records(task.id, spec.id).unwrap_err().code,
            "invalid-relationship"
        );
    }

    #[test]
    fn migrates_existing_records_to_generic_without_losing_related_data() {
        let path = std::env::temp_dir().join(format!(
            "forge-store-migration-{}.sqlite",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("PRAGMA foreign_keys = ON;
            CREATE TABLE projects (slug TEXT PRIMARY KEY NOT NULL, registered_folder TEXT UNIQUE NOT NULL, origin_url TEXT NOT NULL);
            CREATE TABLE records (id INTEGER PRIMARY KEY NOT NULL, body TEXT NOT NULL, status TEXT NOT NULL CHECK (status IN ('open', 'in_progress', 'done')), completion_resolution TEXT);
            CREATE TABLE record_projects (record_id INTEGER NOT NULL REFERENCES records(id), project_slug TEXT NOT NULL REFERENCES projects(slug), PRIMARY KEY (record_id, project_slug));
            CREATE TABLE record_relationships (parent_id INTEGER NOT NULL REFERENCES records(id), child_id INTEGER NOT NULL REFERENCES records(id) UNIQUE, kind TEXT NOT NULL CHECK (kind IN ('parent-child')), PRIMARY KEY (parent_id, child_id));
            CREATE TABLE record_updates (id INTEGER PRIMARY KEY NOT NULL, record_id INTEGER NOT NULL REFERENCES records(id), created_at_millis INTEGER NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('created', 'body-edited', 'status-transition', 'relationship-added', 'relationship-removed')), from_status TEXT, to_status TEXT, related_record_id INTEGER REFERENCES records(id), comment_id INTEGER);
            CREATE TABLE record_comments (id INTEGER PRIMARY KEY NOT NULL, record_id INTEGER NOT NULL REFERENCES records(id), created_at_millis INTEGER NOT NULL, author TEXT, body TEXT NOT NULL);
            INSERT INTO projects VALUES ('alpha', '/repos/alpha', 'https://example.test/repo.git');
            INSERT INTO records VALUES (6, 'parent', 'open', NULL);
            INSERT INTO records VALUES (7, 'legacy', 'done', 'complete');
            INSERT INTO record_projects VALUES (6, 'alpha');
            INSERT INTO record_projects VALUES (7, 'alpha');
            INSERT INTO record_relationships VALUES (6, 7, 'parent-child');
            INSERT INTO record_updates VALUES (9, 7, 1, 'created', NULL, NULL, NULL, NULL);
            INSERT INTO record_comments VALUES (11, 7, 2, 'agent', 'comment');").unwrap();
        drop(connection);

        let store = SqliteProjectRepository::open(&path).unwrap();
        let connection = store.connection.lock().unwrap();
        assert!(connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'record_update_subscriptions')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap());
        drop(connection);
        let record = store.show_record(7).unwrap();
        assert_eq!(record.record_type, RecordType::Generic);
        assert_eq!(record.body, "legacy");
        assert_eq!(record.project_slugs, ["alpha"]);
        assert_eq!(record.status, RecordStatus::Done);
        assert_eq!(record.completion_resolution.as_deref(), Some("complete"));
        assert_eq!(record.relationships[0].parent_id, 6);
        assert_eq!(
            store.show_record(6).unwrap().record_type,
            RecordType::Generic
        );
        assert_eq!(store.list_record_updates(7).unwrap()[0].id.0, 9);
        assert_eq!(store.list_record_comments(7).unwrap()[0].id.0, 11);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn durable_subscriptions_are_idempotent_and_replay_selected_updates_in_global_order() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "task".into(),
                project_slugs: vec!["alpha".into()],
            })
            .unwrap();
        let target = store
            .open_record(&NewRecord {
                body: "target".into(),
                project_slugs: vec![],
            })
            .unwrap();
        let other = store
            .open_record(&NewRecord {
                body: "other".into(),
                project_slugs: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "attempt-1".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();

        let request = NewRecordUpdateSubscription {
            assignment_id: "attempt-1".into(),
            target_record_ids: vec![target.id, target.id],
            kind_filters: vec![
                RecordUpdateKindFilter::BodyEdited,
                RecordUpdateKindFilter::BodyEdited,
            ],
        };
        let subscription = store.create_record_update_subscription(&request).unwrap();
        assert_eq!(subscription.target_record_ids, vec![target.id]);
        assert_eq!(
            subscription.kind_filters,
            vec![RecordUpdateKindFilter::BodyEdited]
        );
        assert_eq!(
            store.create_record_update_subscription(&request).unwrap(),
            subscription
        );

        store.edit_record(other.id, "unselected").unwrap();
        store.start_record(target.id).unwrap();
        store.edit_record(target.id, "selected").unwrap();
        assert!(store.record_update_head().unwrap() > subscription.start_cursor);
        let replay = store
            .replay_record_updates(subscription.id, subscription.start_cursor)
            .unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].record_id, target.id);
        assert!(matches!(
            replay[0].update.kind,
            RecordUpdateKind::BodyEdited
        ));
        assert!(
            replay
                .windows(2)
                .all(|pair| pair[0].update.id < pair[1].update.id)
        );
        assert_eq!(
            store
                .replay_record_updates(subscription.id, forge_app::RecordUpdateId(0))
                .unwrap_err()
                .code,
            "invalid-cursor"
        );
        store
            .delete_record_update_subscription(subscription.id)
            .unwrap();
        assert_eq!(
            store
                .record_update_subscription(subscription.id)
                .unwrap_err()
                .code,
            "subscription-not-found"
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn subscription_management_is_direct_child_owned_and_revoked_on_retirement() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let parent = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "parent".into(),
                project_slugs: vec!["alpha".into()],
            })
            .unwrap();
        let child = store
            .open_record(&NewRecord {
                body: "child".into(),
                project_slugs: vec!["alpha".into()],
            })
            .unwrap();
        let sibling = store
            .open_record(&NewRecord {
                body: "sibling".into(),
                project_slugs: vec!["alpha".into()],
            })
            .unwrap();
        let grandchild = store
            .open_record(&NewRecord {
                body: "grandchild".into(),
                project_slugs: vec!["alpha".into()],
            })
            .unwrap();
        store.relate_records(child.id, parent.id).unwrap();
        store.relate_records(sibling.id, parent.id).unwrap();
        store.relate_records(grandchild.id, child.id).unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "owner".into(),
                record_id: parent.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        let subscription = assignment_create_record_update_subscription(
            &store,
            "owner",
            "cap-owner",
            "create",
            vec![child.id],
            vec![],
        )
        .unwrap();
        assert_eq!(subscription.target_record_ids, vec![child.id]);
        assert_eq!(
            assignment_create_record_update_subscription(
                &store,
                "owner",
                "cap-owner",
                "sibling",
                vec![sibling.id],
                vec![]
            )
            .unwrap()
            .target_record_ids,
            vec![sibling.id]
        );
        assert_eq!(
            assignment_create_record_update_subscription(
                &store,
                "owner",
                "cap-owner",
                "transitive",
                vec![grandchild.id],
                vec![]
            )
            .unwrap_err()
            .code,
            "assignment-capability-denied"
        );
        assert_eq!(
            assignment_renew_record_update_subscription(
                &store,
                "other",
                "cap-other",
                "renew",
                subscription.id
            )
            .unwrap_err()
            .code,
            "assignment-not-found"
        );
        assignment_delete_record_update_subscription(
            &store,
            "owner",
            "cap-owner",
            "delete",
            subscription.id,
        )
        .unwrap();
        let replacement = assignment_create_record_update_subscription(
            &store,
            "owner",
            "cap-owner",
            "replacement",
            vec![child.id],
            vec![],
        )
        .unwrap();
        store
            .close_agent_assignment("owner", forge_app::AgentAssignmentClosureReason::Retired)
            .unwrap();
        assert_eq!(
            store
                .record_update_subscription(replacement.id)
                .unwrap_err()
                .code,
            "subscription-not-found"
        );
    }

    #[test]
    fn persists_dispatch_profiles_assignments_resources_and_project_configuration() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "task".into(),
                project_slugs: vec!["alpha".into()],
            })
            .unwrap();

        let profile = store.agent_profile(RecordType::Task, 1).unwrap();
        assert_eq!(profile.provider, "herdr");
        assert_eq!(profile.agent_kind, "pi");
        assert_eq!(profile.model, "gpt-5.6-terra");
        assert_eq!(store.agent_profile(RecordType::Task, 1).unwrap(), profile);

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

        asserts_project_configuration_persists(&store);
    }

    fn asserts_project_configuration_persists(store: &SqliteProjectRepository) {
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
    fn persists_worktree_configuration_and_claim_lifecycle() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        assert_eq!(
            store
                .set_worktree_root(Some(Path::new("relative")))
                .unwrap_err()
                .code,
            "invalid-worktree-root"
        );
        store
            .set_worktree_root(Some(Path::new("/worktrees")))
            .unwrap();
        store
            .set_project_setup_hook("alpha", Some("cargo test"))
            .unwrap();
        assert_eq!(
            store.worktree_root().unwrap(),
            Some(PathBuf::from("/worktrees"))
        );
        assert_eq!(
            store.project_setup_hook("alpha").unwrap().as_deref(),
            Some("cargo test")
        );

        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "task".into(),
                project_slugs: vec!["alpha".into()],
            })
            .unwrap();
        assert_eq!(
            store.reserve_worktree_claim(task.id).unwrap_err().code,
            "worktree-ineligible-record"
        );
        store.start_record(task.id).unwrap();
        let reserved = store.reserve_worktree_claim(task.id).unwrap();
        assert_eq!(
            reserved.path,
            PathBuf::from(format!("/worktrees/alpha/{}", task.id))
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
    fn reconciliation_scan_uses_a_lease_and_durable_retry_exhaustion() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "dispatch me".into(),
                project_slugs: vec!["alpha".into()],
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
        assert_eq!(assignment.retry_count, 5);
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
        assert_eq!(
            store.list_agent_assignments(task.id).unwrap()[0].state,
            forge_app::AgentAssignmentState::Working
        );
    }

    #[test]
    fn atomically_claims_eligible_tasks_and_retains_capacity() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let first = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "first".into(),
                project_slugs: vec!["alpha".into()],
            })
            .unwrap();
        store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "second".into(),
                project_slugs: vec!["alpha".into()],
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
            RecordStatus::InProgress
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
    fn capability_operations_are_bounded_and_audited_without_secrets_or_comment_bodies() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let spec = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Spec,
                body: "spec".into(),
                project_slugs: vec!["alpha".into()],
            })
            .unwrap();
        let task = store.open_task_under_spec("task", spec.id).unwrap();
        let sibling = store.open_task_under_spec("sibling", spec.id).unwrap();
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
                .all(|entry| entry.capability_id == "cap-1" && !entry.reason.contains("hidden"))
        );
        assert_eq!(
            audit[1].outcome,
            forge_app::AssignmentAuthorizationOutcome::Denied
        );
    }
}
