//! SQLite implementation of the application persistence boundary.

use std::{path::Path, sync::Mutex};

use forge_app::{Project, ProjectRepository, RegistrationError};
use rusqlite::{Connection, ErrorCode, params};

/// SQLite-backed Project storage.
pub struct SqliteProjectRepository {
    connection: Mutex<Connection>,
}

impl SqliteProjectRepository {
    /// Opens a database and applies the Project schema.
    ///
    /// # Errors
    ///
    /// Returns a storage error when SQLite cannot open or initialize the database.
    pub fn open(path: &Path) -> Result<Self, RegistrationError> {
        let connection = Connection::open(path).map_err(storage_error)?;
        Self::from_connection(connection)
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
                "CREATE TABLE IF NOT EXISTS projects (
                slug TEXT PRIMARY KEY NOT NULL,
                registered_folder TEXT UNIQUE NOT NULL,
                origin_url TEXT NOT NULL
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
                let existing = connection.query_row(
                    "SELECT slug, registered_folder, origin_url FROM projects WHERE slug = ?1 OR registered_folder = ?2",
                    params![project.slug, folder],
                    |row| Ok(Project {
                        slug: row.get(0)?, registered_folder: std::path::PathBuf::from(row.get::<_, String>(1)?), origin_url: row.get(2)?,
                    }),
                ).map_err(storage_error)?;
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

fn folders_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

#[allow(clippy::needless_pass_by_value)]
fn storage_error(error: rusqlite::Error) -> RegistrationError {
    RegistrationError::new(
        "storage",
        format!("could not persist Project registration: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::SqliteProjectRepository;
    use forge_app::{Project, ProjectRepository};
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
}
