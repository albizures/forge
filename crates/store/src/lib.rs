//! SQLite implementation of the application persistence boundary.

use std::{path::Path, sync::Mutex};

use forge_app::{
    NewRecord, Project, ProjectRepository, Record, RecordRepository, RecordStatus,
    RegistrationError,
};
use rusqlite::{Connection, ErrorCode, params};

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
                status TEXT NOT NULL CHECK (status = 'open')
            );
            CREATE TABLE IF NOT EXISTS record_projects (
                record_id INTEGER NOT NULL REFERENCES records(id),
                project_slug TEXT NOT NULL REFERENCES projects(slug),
                PRIMARY KEY (record_id, project_slug)
            );",
            )
            .map_err(storage_error)?;
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
        })
    }
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
