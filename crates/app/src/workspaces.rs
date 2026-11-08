//! Forge-owned filesystem boundaries, independent of provider containers.
use crate::{Application, RegistrationError};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}

/// Durable Workspace management. Implementations must validate canonical roots and
/// reject overlapping registrations atomically. IDs and roots cannot be edited.
pub trait WorkspaceRepository: Send + Sync {
    /// Registers an immutable ID and canonical directory.
    /// # Errors
    /// Returns invalid identity/root, overlap, duplicate, or storage errors.
    fn create_workspace(
        &self,
        id: &str,
        name: &str,
        root: &Path,
    ) -> Result<Workspace, RegistrationError>;
    /// Inspects a registration by immutable ID.
    /// # Errors
    /// Returns not-found or storage errors.
    fn workspace(&self, id: &str) -> Result<Workspace, RegistrationError>;
    /// Lists registrations in immutable ID order.
    /// # Errors
    /// Returns storage errors.
    fn workspaces(&self) -> Result<Vec<Workspace>, RegistrationError>;
    /// Changes only the display name.
    /// # Errors
    /// Returns blank-name, not-found, or storage errors.
    fn rename_workspace(&self, id: &str, name: &str) -> Result<Workspace, RegistrationError>;
    /// Removes only an empty registration; never removes files or provider resources.
    /// Extend the durable emptiness guard as new entities acquire Workspace ownership.
    /// # Errors
    /// Returns not-found, nonempty-registration, or storage errors.
    fn remove_workspace(&self, id: &str) -> Result<(), RegistrationError>;
}

impl Application {
    #[must_use]
    pub fn with_workspace_repository(mut self, repository: Box<dyn WorkspaceRepository>) -> Self {
        self.workspaces = Some(repository);
        self
    }

    fn workspace_repository(&self) -> Result<&dyn WorkspaceRepository, RegistrationError> {
        self.workspaces.as_deref().ok_or_else(|| {
            RegistrationError::new(
                "workspace-unavailable",
                "Workspace storage is not configured",
            )
        })
    }

    /// Registers a Workspace independently of dispatch configuration.
    /// # Errors
    /// Returns unavailable storage, invalid identity/root, overlap, or persistence errors.
    pub fn create_workspace(
        &self,
        id: &str,
        name: &str,
        root: &Path,
    ) -> Result<Workspace, RegistrationError> {
        self.workspace_repository()?
            .create_workspace(id, name, root)
    }
    /// Inspects a Workspace by ID.
    /// # Errors
    /// Returns unavailable storage, not-found, or persistence errors.
    pub fn workspace(&self, id: &str) -> Result<Workspace, RegistrationError> {
        self.workspace_repository()?.workspace(id)
    }
    /// Lists all Workspace registrations without requiring selection.
    /// # Errors
    /// Returns unavailable storage or persistence errors.
    pub fn workspaces(&self) -> Result<Vec<Workspace>, RegistrationError> {
        self.workspace_repository()?.workspaces()
    }
    /// Edits a display name without changing identity or root.
    /// # Errors
    /// Returns unavailable storage, invalid name, not-found, or persistence errors.
    pub fn rename_workspace(&self, id: &str, name: &str) -> Result<Workspace, RegistrationError> {
        self.workspace_repository()?.rename_workspace(id, name)
    }
    /// Removes an empty registration, never filesystem or provider resources.
    /// # Errors
    /// Returns unavailable storage, nonempty-registration, not-found, or persistence errors.
    pub fn remove_workspace(&self, id: &str) -> Result<(), RegistrationError> {
        self.workspace_repository()?.remove_workspace(id)
    }
    /// Explicit selection wins over cwd. A qualified Project's Workspace, when
    /// supplied, must agree with the explicit Workspace selector.
    /// # Errors
    /// Returns conflicting selectors, missing selection, invalid cwd, or storage errors.
    pub fn select_workspace(
        &self,
        explicit: Option<&str>,
        project_workspace: Option<&str>,
        cwd: &Path,
    ) -> Result<Workspace, RegistrationError> {
        if let (Some(explicit), Some(project)) = (explicit, project_workspace)
            && explicit != project
        {
            return Err(RegistrationError::new(
                "workspace-conflict",
                "Workspace and qualified Project selectors conflict",
            ));
        }
        if let Some(id) = explicit.or(project_workspace) {
            return self.workspace(id);
        }
        let cwd = cwd.canonicalize().map_err(|_| {
            RegistrationError::new(
                "invalid-folder",
                "could not resolve current directory; select --workspace <ID>",
            )
        })?;
        self.workspaces()?
            .into_iter()
            .find(|workspace| cwd.starts_with(&workspace.root))
            .ok_or_else(|| {
                RegistrationError::new(
                    "workspace-required",
                    "current directory is outside all Forge Workspaces; select --workspace <ID>",
                )
            })
    }
}
