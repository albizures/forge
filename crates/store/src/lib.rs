//! SQLite implementation of the application persistence boundary.

use std::{
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use forge_app::{
    NewRecord, NewTypedRecord, Project, ProjectRepository, Record, RecordComment, RecordCommentId,
    RecordListFilter, RecordRelationship, RecordRelationshipKind, RecordRepository, RecordStatus,
    RecordType, RecordUpdate, RecordUpdateId, RecordUpdateKind, RegistrationError,
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
            );",
            )
            .map_err(storage_error)?;
        migrate_record_status_schema(&connection)?;
        migrate_record_updates_schema(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}

impl ProjectRepository for SqliteProjectRepository {
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
                "INSERT INTO records (body, status) VALUES (?1, 'open')",
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

    fn open_typed_record(&self, _: &NewTypedRecord) -> Result<Record, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported-record-type",
            "typed Record persistence has not been migrated",
        ))
    }

    fn open_task_under_spec(&self, _: &str, _: u64) -> Result<Record, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported-record-type",
            "typed Record persistence has not been migrated",
        ))
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
        ids.into_iter()
            .map(|id| load_record(&connection, id.cast_unsigned()))
            .collect()
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

fn update_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordUpdate> {
    let kind: String = row.get(2)?;
    let from_status: Option<String> = row.get(3)?;
    let to_status: Option<String> = row.get(4)?;
    let related_record_id: Option<i64> = row.get(5)?;
    let comment_id: Option<i64> = row.get(6)?;
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
        id: RecordUpdateId(row.get::<_, i64>(0)?.cast_unsigned()),
        created_at_millis: row.get::<_, i64>(1)?.cast_unsigned(),
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

fn load_record(connection: &Connection, id: u64) -> Result<Record, RegistrationError> {
    let (body, status, completion_resolution) = connection
        .query_row(
            "SELECT body, status, completion_resolution FROM records WHERE id = ?1",
            [id.cast_signed()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get(2)?,
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
        record_type: RecordType::Generic,
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
        NewRecord, Project, ProjectRepository, RecordListFilter, RecordRepository, RecordStatus,
        RecordUpdateKind,
    };
    use std::path::{Path, PathBuf};
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
                    status: None
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
                    status: Some(RecordStatus::Open)
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
}
