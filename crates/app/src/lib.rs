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
    /// Returns a user-facing error when persistence or identity validation fails.
    fn register(&self, project: &Project) -> Result<Project, RegistrationError>;

    /// Finds the Project whose Registered Folder contains `folder`.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when no Project contains the folder.
    fn find_containing(&self, folder: &Path) -> Result<Project, RegistrationError>;
}

/// The application boundary for operations shared by all transports.
pub struct Application {
    projects: Box<dyn ProjectRepository>,
    inspector: Box<dyn RepositoryInspector>,
}

impl Application {
    /// Creates an application from its infrastructure boundaries.
    #[must_use]
    pub fn new(
        projects: Box<dyn ProjectRepository>,
        inspector: Box<dyn RepositoryInspector>,
    ) -> Self {
        Self {
            projects,
            inspector,
        }
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
        self.projects.register(&project)
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
        self.projects.find_containing(&folder)
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
