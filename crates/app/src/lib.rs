//! Application-owned operations and persistence contracts.

use std::path::{Path, PathBuf};

/// A Project registered with Forge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    /// Immutable human-referenceable identity.
    pub slug: String,
    /// Canonical path of the Repository's main Worktree.
    pub registered_folder: PathBuf,
    /// URL configured for the Repository's `origin` remote.
    pub origin_url: String,
}

/// The initial lifecycle status of a Record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordStatus {
    Open,
}

/// A generic, project-scoped unit of work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Opaque, positive identity assigned by persistence.
    pub id: u64,
    /// Required Markdown content, separate from its structured metadata.
    pub body: String,
    /// Immutable, lexically ordered Project Scope selected at opening.
    pub project_slugs: Vec<String>,
    /// The initial status assigned at opening.
    pub status: RecordStatus,
}

/// A Record before persistence assigns its identity and initial status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewRecord {
    pub body: String,
    pub project_slugs: Vec<String>,
}

/// A domain error which can safely be shown to a user.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistrationError {
    /// Stable, transport-friendly category.
    pub code: &'static str,
    /// Actionable explanation.
    pub message: String,
}

impl RegistrationError {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Inspects a folder without coupling application policy to a Git implementation.
pub trait RepositoryInspector: Send + Sync {
    /// Returns the eligible main-worktree registration facts for `folder`.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the folder is not eligible.
    fn inspect(&self, folder: &Path) -> Result<Project, RegistrationError>;
}

/// Persistence boundary owned by the application.
pub trait ProjectRepository: Send + Sync {
    /// Stores `project`, or returns the matching existing Project idempotently.
    ///
    /// # Errors
    ///
    /// Returns a user-facing validation or storage failure.
    fn register(&self, project: &Project) -> Result<Project, RegistrationError>;
    /// Finds the Project whose Registered Folder contains `folder`.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when no Project contains the folder.
    fn find_containing(&self, folder: &Path) -> Result<Project, RegistrationError>;
}

/// Persistence operations for generic Records.
pub trait RecordRepository: ProjectRepository {
    /// Opens a Record after validating that all selected Projects exist.
    ///
    /// # Errors
    ///
    /// Returns a user-facing validation or storage failure.
    fn open_record(&self, record: &NewRecord) -> Result<Record, RegistrationError>;
}

/// The application boundary for operations shared by all transports.
pub struct Application {
    records: Box<dyn RecordRepository>,
    inspector: Box<dyn RepositoryInspector>,
}

impl Application {
    /// Creates an application from its infrastructure boundaries.
    #[must_use]
    pub fn new(
        records: Box<dyn RecordRepository>,
        inspector: Box<dyn RepositoryInspector>,
    ) -> Self {
        Self { records, inspector }
    }

    /// Registers an eligible Git main Worktree as a Project.
    ///
    /// # Errors
    ///
    /// Returns the validation or persistence failure suitable for the calling adapter.
    pub fn register_project(
        &self,
        folder: &Path,
        slug: &str,
    ) -> Result<Project, RegistrationError> {
        if slug.is_empty() {
            return Err(RegistrationError::new(
                "invalid-slug",
                "project slug must not be empty",
            ));
        }
        let mut project = self.inspector.inspect(folder)?;
        slug.clone_into(&mut project.slug);
        self.records.register(&project)
    }

    /// Finds the Project containing the supplied current folder.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the folder is outside every Project.
    pub fn project_here(&self, folder: &Path) -> Result<Project, RegistrationError> {
        let folder = std::fs::canonicalize(folder).map_err(|_| {
            RegistrationError::new(
                "invalid-folder",
                format!("`{}` is not an existing folder", folder.display()),
            )
        })?;
        self.records.find_containing(&folder)
    }

    /// Opens a generic Record with a required body and explicit Project Scope.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for blank bodies, duplicate or unknown Projects, or storage failures.
    pub fn open_record(
        &self,
        body: &str,
        project_slugs: Vec<String>,
    ) -> Result<Record, RegistrationError> {
        if body.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-body",
                "record body must not be blank",
            ));
        }
        let mut project_slugs = project_slugs;
        project_slugs.sort();
        if project_slugs.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(RegistrationError::new(
                "duplicate-project",
                "record Project Scope must not contain duplicate Project slugs",
            ));
        }
        self.records.open_record(&NewRecord {
            body: body.to_owned(),
            project_slugs,
        })
    }

    /// Returns readiness information for this application instance.
    #[must_use]
    pub fn status(&self) -> ApplicationStatus {
        ApplicationStatus {
            name: "forge",
            version: env!("CARGO_PKG_VERSION"),
            ready: true,
        }
    }
}

/// Application-owned readiness information.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationStatus {
    pub name: &'static str,
    pub version: &'static str,
    pub ready: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Repository {
        records: Mutex<Vec<Record>>,
    }
    impl ProjectRepository for Repository {
        fn register(&self, project: &Project) -> Result<Project, RegistrationError> {
            Ok(project.clone())
        }
        fn find_containing(&self, _: &Path) -> Result<Project, RegistrationError> {
            unreachable!()
        }
    }
    impl RecordRepository for Repository {
        fn open_record(&self, record: &NewRecord) -> Result<Record, RegistrationError> {
            let mut records = self.records.lock().unwrap();
            let record = Record {
                id: (records.len() + 1) as u64,
                body: record.body.clone(),
                project_slugs: record.project_slugs.clone(),
                status: RecordStatus::Open,
            };
            records.push(record.clone());
            Ok(record)
        }
    }
    struct Inspector;
    impl RepositoryInspector for Inspector {
        fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
            unreachable!()
        }
    }
    fn application() -> Application {
        Application::new(
            Box::new(Repository {
                records: Mutex::new(vec![]),
            }),
            Box::new(Inspector),
        )
    }

    #[test]
    fn opening_requires_a_non_blank_body_and_a_distinct_scope() {
        assert_eq!(
            application().open_record(" \n", vec![]).unwrap_err().code,
            "invalid-body"
        );
        assert_eq!(
            application()
                .open_record("body", vec!["alpha".into(), "alpha".into()])
                .unwrap_err()
                .code,
            "duplicate-project"
        );
    }

    #[test]
    fn opening_assigns_open_status_and_sorts_scope() {
        assert_eq!(
            application()
                .open_record("body", vec!["zeta".into(), "alpha".into()])
                .unwrap(),
            Record {
                id: 1,
                body: "body".into(),
                project_slugs: vec!["alpha".into(), "zeta".into()],
                status: RecordStatus::Open
            }
        );
    }
}
