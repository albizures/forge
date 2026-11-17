//! Workspace-local Project identity and filesystem registration boundaries.
use crate::{Application, Project, RegistrationError, Workspace};
use std::path::{Path, PathBuf};

/// Project policy for selecting an explicitly configured root's starting branch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BaseBranchPolicy {
    #[default]
    Ask,
    PlanningBranch,
    DefaultBranch,
}

impl BaseBranchPolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::PlanningBranch => "planning-branch",
            Self::DefaultBranch => "default-branch",
        }
    }
}

impl std::str::FromStr for BaseBranchPolicy {
    type Err = RegistrationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ask" => Ok(Self::Ask),
            "planning-branch" => Ok(Self::PlanningBranch),
            "default-branch" => Ok(Self::DefaultBranch),
            _ => Err(RegistrationError::new(
                "invalid-base-branch-policy",
                "Base Branch Policy must be ask, planning-branch, or default-branch",
            )),
        }
    }
}

#[cfg(test)]
mod policy_tests {
    use super::BaseBranchPolicy;

    #[test]
    fn policy_values_are_exact_and_round_trip() {
        assert_eq!(BaseBranchPolicy::default(), BaseBranchPolicy::Ask);
        for value in ["ask", "planning-branch", "default-branch"] {
            assert_eq!(value.parse::<BaseBranchPolicy>().unwrap().as_str(), value);
        }
        for value in ["", "ASK", "default", " ask", "ask "] {
            assert!(value.parse::<BaseBranchPolicy>().is_err());
        }
    }
}

/// An unambiguous Project reference, rendered as `<workspace>/<slug>`.
/// Persist this reference (not the local slug) in Project-owned configuration and scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectReference {
    pub workspace_id: String,
    pub slug: String,
}
impl ProjectReference {
    /// Constructs a qualified immutable identity.
    /// # Errors
    /// Returns invalid identity components, including path separators.
    pub fn new(workspace_id: &str, slug: &str) -> Result<Self, RegistrationError> {
        if workspace_id.is_empty()
            || !workspace_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
            || slug.is_empty()
            || slug == "."
            || slug == ".."
            || !slug
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        {
            return Err(RegistrationError::new(
                "invalid-project-reference",
                "Project reference must be <workspace>/<slug>; use letters, digits, hyphens or underscores (and dots within Project slugs)",
            ));
        }
        Ok(Self {
            workspace_id: workspace_id.into(),
            slug: slug.into(),
        })
    }

    /// Parses a reference without inferring a Workspace from mutable process state.
    /// # Errors
    /// Returns an invalid or unqualified reference error.
    pub fn parse(value: &str) -> Result<Self, RegistrationError> {
        let (workspace, slug) = value.split_once('/').ok_or_else(|| {
            RegistrationError::new(
                "qualified-project-required",
                "select a Workspace or supply <workspace>/<project>",
            )
        })?;
        Self::new(workspace, slug)
    }

    #[must_use]
    pub fn qualified(&self) -> String {
        format!("{}/{}", self.workspace_id, self.slug)
    }
}

/// Validates canonical containment and the Workspace-relative reserved subtree.
/// Both the apparent reserved location and aliases to its canonical location are reserved.
/// # Errors
/// Returns unresolvable, external, or reserved Project-root errors.
pub fn canonical_project_root(
    workspace: &Workspace,
    folder: &Path,
) -> Result<PathBuf, RegistrationError> {
    let root = folder.canonicalize().map_err(|_| {
        RegistrationError::new(
            "invalid-project-root",
            "Project root must be an existing, resolvable directory",
        )
    })?;
    if !root.is_dir() || !root.starts_with(&workspace.root) {
        return Err(RegistrationError::new(
            "project-outside-workspace",
            "Project root must be within its selected Workspace",
        ));
    }
    let reserved = workspace.root.join(".forge/worktrees");
    let resolved_reserved = canonical_existing_ancestor(&reserved)?;
    if root.starts_with(&reserved) || root.starts_with(&resolved_reserved) {
        return Err(RegistrationError::new(
            "reserved-project-root",
            "Project roots cannot be within the Workspace's .forge/worktrees subtree",
        ));
    }
    Ok(root)
}

fn canonical_existing_ancestor(path: &Path) -> Result<PathBuf, RegistrationError> {
    let mut ancestor = path;
    let mut suffix = Vec::new();
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let mut resolved = ancestor.canonicalize().map_err(|_| {
                    RegistrationError::new(
                        "invalid-project-root",
                        "cannot resolve the reserved Managed Worktree subtree",
                    )
                })?;
                for component in suffix.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                suffix.push(
                    ancestor
                        .file_name()
                        .ok_or_else(|| {
                            RegistrationError::new(
                                "invalid-project-root",
                                "cannot resolve Project boundary",
                            )
                        })?
                        .to_owned(),
                );
                ancestor = ancestor.parent().ok_or_else(|| {
                    RegistrationError::new(
                        "invalid-project-root",
                        "cannot resolve Project boundary",
                    )
                })?;
            }
            Err(error) => {
                return Err(RegistrationError::new(
                    "invalid-project-root",
                    error.to_string(),
                ));
            }
        }
    }
}

impl Application {
    /// Reads the effective policy for a qualified Project reference.
    /// # Errors
    /// Returns invalid reference, missing Project, or persistence failures.
    pub fn base_branch_policy(
        &self,
        reference: &str,
    ) -> Result<BaseBranchPolicy, RegistrationError> {
        let reference = ProjectReference::parse(reference)?.qualified();
        self.records.base_branch_policy(&reference)
    }

    /// Sets or resets the policy for a qualified Project reference.
    /// # Errors
    /// Returns invalid reference, missing Project, or persistence failures.
    pub fn set_base_branch_policy(
        &self,
        reference: &str,
        policy: Option<BaseBranchPolicy>,
    ) -> Result<(), RegistrationError> {
        let reference = ProjectReference::parse(reference)?.qualified();
        self.records.set_base_branch_policy(&reference, policy)
    }

    /// Routes registration by explicit selectors first, otherwise canonical cwd.
    /// # Errors
    /// Returns selector conflicts, missing Workspace, invalid roots or registration failures.
    pub fn register_selected_project(
        &self,
        workspace: Option<&str>,
        folder: &Path,
        project: &str,
        cwd: &Path,
    ) -> Result<Project, RegistrationError> {
        let qualified = project
            .contains('/')
            .then(|| ProjectReference::parse(project))
            .transpose()?;
        let selected = self.select_workspace(
            workspace,
            qualified
                .as_ref()
                .map(|reference| reference.workspace_id.as_str()),
            cwd,
        )?;
        let slug = qualified
            .as_ref()
            .map_or(project, |reference| reference.slug.as_str());
        self.register_workspace_project(&selected.id, folder, slug)
    }

    /// Registers a main checkout under an explicit Workspace-local identity.
    /// Repository inspection retains main-checkout and origin-remote requirements.
    /// # Errors
    /// Returns invalid identity, containment, repository inspection or persistence failures.
    pub fn register_workspace_project(
        &self,
        workspace_id: &str,
        folder: &Path,
        slug: &str,
    ) -> Result<Project, RegistrationError> {
        let reference = ProjectReference::new(workspace_id, slug)?;
        let workspace = self.workspace(workspace_id)?;
        let root = canonical_project_root(&workspace, folder)?;
        let mut project = self.inspector.inspect(&root)?;
        // Inspectors must identify this exact checkout, not silently register its ancestor.
        if project.project_root != root {
            return Err(RegistrationError::new(
                "invalid-project-root",
                "Project root must be the main repository checkout root",
            ));
        }
        project.reference = reference.qualified();
        project.workspace_id = reference.workspace_id;
        project.slug = reference.slug;
        self.records.register(&project)
    }

    /// Selects a qualified Project explicitly, otherwise the deepest containing root.
    /// Explicit Project selection wins over cwd, but cannot contradict a Workspace selector.
    /// # Errors
    /// Returns conflicting selectors, missing selection, or lookup errors.
    pub fn select_project(
        &self,
        workspace: Option<&str>,
        project: Option<&str>,
        cwd: &Path,
    ) -> Result<Project, RegistrationError> {
        let qualified = project
            .filter(|project| project.contains('/'))
            .map(ProjectReference::parse)
            .transpose()?;
        let selected = self.select_workspace(
            workspace,
            qualified
                .as_ref()
                .map(|reference| reference.workspace_id.as_str()),
            cwd,
        )?;
        if let Some(project) = project {
            let reference =
                qualified.map_or_else(|| ProjectReference::new(&selected.id, project), Ok)?;
            return self
                .records
                .project_in_workspace(&reference.workspace_id, &reference.slug);
        }
        let project = self.project_here(cwd).map_err(|error| {
            if error.code == "project-not-found" {
                RegistrationError::new("project-not-found", "current directory has no Project in the selected Workspace; select --project <slug>")
            } else { error }
        })?;
        if project.workspace_id != selected.id {
            return Err(RegistrationError::new(
                "project-not-found",
                "current directory has no Project in the selected Workspace; select --project",
            ));
        }
        Ok(project)
    }
}
