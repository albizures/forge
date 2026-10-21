//! SQLite implementation of the application persistence boundary.

use std::{path::Path, sync::Mutex};

use forge_app::{
    NewRecord, Project, ProjectRepository, Record, RecordRelationship, RecordRelationshipKind,
    RecordRepository, RecordStatus, RegistrationError,
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
            );",
            )
            .map_err(storage_error)?;
        migrate_record_status_schema(&connection)?;
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
        transaction.commit().map_err(storage_error)?;
        Ok(Record {
            id,
            body: record.body.clone(),
            project_slugs: record.project_slugs.clone(),
            status: RecordStatus::Open,
            completion_resolution: None,
            relationships: vec![],
        })
    }

    fn show_record(&self, id: u64) -> Result<Record, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        load_record(&connection, id)
    }

    fn edit_record(&self, id: u64, body: &str) -> Result<Record, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let updated = connection
            .execute(
                "UPDATE records SET body = ?1 WHERE id = ?2 AND status != 'done'",
                params![body, id.cast_signed()],
            )
            .map_err(storage_error)?;
        if updated == 0 {
            return record_or_transition_error(&connection, id);
        }
        load_record(&connection, id)
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
        let record = load_record(&transaction, child_id)?;
        transaction.commit().map_err(storage_error)?;
        Ok(record)
    }

    fn unrelate_records(&self, child_id: u64, parent_id: u64) -> Result<Record, RegistrationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
        let deleted = connection
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
        load_record(&connection, child_id)
    }
}

fn transition_record(
    connection: &Mutex<Connection>,
    id: u64,
    from: &str,
    to: &str,
    completion_resolution: Option<&str>,
) -> Result<Record, RegistrationError> {
    let connection = connection
        .lock()
        .map_err(|_| RegistrationError::new("storage", "Project storage lock is poisoned"))?;
    let updated = connection
        .execute(
            "UPDATE records SET status = ?1, completion_resolution = ?2 WHERE id = ?3 AND status = ?4",
            params![to, completion_resolution, id.cast_signed(), from],
        )
        .map_err(storage_error)?;
    if updated == 0 {
        return record_or_transition_error(&connection, id);
    }
    load_record(&connection, id)
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
        body,
        project_slugs,
        status,
        completion_resolution,
        relationships,
    })
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
    use forge_app::{NewRecord, Project, ProjectRepository, RecordRepository, RecordStatus};
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
