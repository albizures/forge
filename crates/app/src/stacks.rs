//! Linear Task stacks and independently verified publication evidence.
use crate::{Application, RecordStatus, RegistrationError, outcomes::Evidence};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const PUBLICATION_CHECK: &str = "Publication";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StackDestination {
    /// GitHub owner/name, not a local Repository identity.
    pub repository: String,
    pub remote: String,
    pub branch: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StackSource {
    Root { destination: StackDestination },
    Predecessor { record_id: u64 },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub repository: String,
    pub url: String,
    pub head_branch: String,
    pub head_commit: String,
    pub base_branch: String,
    pub generation: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskStack {
    pub record_id: u64,
    pub predecessor: Option<u64>,
    pub destination: StackDestination,
    pub base_branch: String,
    pub base_commit: Option<String>,
    pub publication: Option<Publication>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PullRequestObservation {
    pub repository: String,
    pub url: String,
    pub head_branch: String,
    pub head_commit: String,
    pub base_branch: String,
    pub open: bool,
    pub draft: bool,
    pub head_repository: String,
}

/// Git/hosting observations are outside persistence transactions. Implementations must
/// validate remote identity and avoid modifying the main checkout or rewriting branches.
pub trait StackHosting: Send + Sync {
    /// Fetch a branch without changing the main checkout.
    /// # Errors
    /// Rejects unsupported hosts, remote identity mismatches and unavailable branches.
    fn fetch_branch(
        &self,
        root: &Path,
        destination: &StackDestination,
        branch: &str,
    ) -> Result<String, RegistrationError>;
    /// Read authoritative hosting facts for a canonical PR URL.
    /// # Errors
    /// Rejects unavailable hosting, malformed responses and unexpected PR identities.
    fn pull_request(
        &self,
        root: &Path,
        repository: &str,
        url: &str,
    ) -> Result<PullRequestObservation, RegistrationError>;
}

pub(crate) fn error(message: impl Into<String>) -> RegistrationError {
    RegistrationError::new("stack-blocked", message)
}

#[must_use]
pub fn task_branch(record: u64) -> String {
    format!("feat/forge-{record}")
}

impl StackDestination {
    /// Reject ambiguous hosts, option-like arguments and invalid branch references.
    /// # Errors
    /// Returns a stack diagnostic for malformed destinations.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        let parts: Vec<_> = self.repository.split('/').collect();
        if parts.len() != 2
            || parts.iter().any(|p| {
                p.is_empty()
                    || !p
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
            || self.remote.is_empty()
            || self.remote.starts_with('-')
            || !self
                .remote
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || !valid_branch(&self.branch)
        {
            return Err(error(
                "expected GitHub owner/name, a remote name and a valid branch",
            ));
        }
        Ok(())
    }
}

#[must_use]
// Git ref validation deliberately treats the .lock suffix case-sensitively.
#[allow(clippy::case_sensitive_file_extension_comparisons)]
pub fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with(['-', '/'])
        && !branch.ends_with(['/', '.'])
        && !branch.contains("..")
        && !branch.contains("@{")
        && branch != "@"
        && branch
            .split('/')
            .all(|p| !p.is_empty() && !p.starts_with('.') && !p.ends_with(".lock"))
        && branch
            .bytes()
            .all(|b| b > 32 && b != 127 && !b"~^:?*[\\".contains(&b))
}

/// Validate membership policy; persistence repeats this inside its writer transaction.
/// # Errors
/// Rejects non-Task/projectless work and incompatible final Check definitions.
pub fn validate_stack_member(
    task: &crate::Record,
    last: Option<&crate::checks::RecordCheck>,
) -> Result<(), RegistrationError> {
    if task.record_type != crate::RecordType::Task || task.project_slugs.len() != 1 {
        return Err(error("stack requires a Task scoped to exactly one Project"));
    }
    let last = last.ok_or_else(|| error("Task has no Checks"))?;
    if last.name != PUBLICATION_CHECK || last.position == 0 {
        return Err(error(
            "stack membership requires a final Publication Check after Implementation",
        ));
    }
    if task.execution_mode == crate::ExecutionMode::Dispatched && last.profile_revision.is_none() {
        return Err(error("Publication Check requires a pinned profile"));
    }
    Ok(())
}

/// Validate the participants independently of graph/storage representation.
/// # Errors
/// Rejects self-links, non-Tasks and differing Project or Workspace scope.
pub fn validate_stack_predecessor(
    task: &crate::Record,
    previous: &crate::Record,
) -> Result<(), RegistrationError> {
    if task.id == previous.id
        || previous.record_type != crate::RecordType::Task
        || task.workspace_id != previous.workspace_id
        || task.project_slugs != previous.project_slugs
    {
        return Err(error(
            "Stack Predecessor must be another Task in the same Project",
        ));
    }
    Ok(())
}

impl Application {
    #[must_use]
    pub fn with_stack_hosting(mut self, hosting: Box<dyn StackHosting>) -> Self {
        self.stack_hosting = Some(hosting.into());
        self
    }

    /// Read explicit stack membership and its inherited destination.
    /// # Errors
    /// Returns Workspace scope or persistence failures.
    pub fn task_stack(&self, record: u64) -> Result<Option<TaskStack>, RegistrationError> {
        self.records.task_stack(record)
    }

    /// Configure membership before any affected Task has prepared execution.
    /// # Errors
    /// Rejects invalid destinations, incompatible Checks, cycles, forks or prepared Tasks.
    pub fn configure_task_stack(
        &self,
        record: u64,
        source: &StackSource,
    ) -> Result<TaskStack, RegistrationError> {
        if let StackSource::Root { destination } = source {
            destination.validate()?;
        }
        let task = self.records.show_record(record)?;
        let checks = self.list_record_checks(record)?;
        validate_stack_member(&task, checks.iter().max_by_key(|c| c.position))?;
        if let StackSource::Predecessor { record_id } = source {
            validate_stack_predecessor(&task, &self.records.show_record(*record_id)?)?;
        }
        self.records.configure_task_stack(record, source)
    }

    fn hosting(&self) -> Result<&dyn StackHosting, RegistrationError> {
        self.stack_hosting
            .as_deref()
            .ok_or_else(|| error("GitHub stack verification is unavailable"))
    }

    fn stack_root(&self, record: u64) -> Result<std::path::PathBuf, RegistrationError> {
        let task = self.records.show_record(record)?;
        let project = task
            .project_slugs
            .first()
            .ok_or_else(|| error("stack requires a Project"))?;
        Ok(self.records.project(project)?.project_root)
    }

    fn verify_predecessor(&self, stack: &TaskStack, root: &Path) -> Result<(), RegistrationError> {
        let Some(predecessor) = stack.predecessor else {
            return Ok(());
        };
        if self.records.show_record(predecessor)?.status != RecordStatus::Done {
            return Err(error("Stack Predecessor is not Done"));
        }
        let previous = self
            .task_stack(predecessor)?
            .ok_or_else(|| error("predecessor is not stacked"))?;
        let publication = previous
            .publication
            .ok_or_else(|| error("predecessor has no verified publication"))?;
        let observed =
            self.hosting()?
                .pull_request(root, &publication.repository, &publication.url)?;
        verify_pr(
            &previous.destination.repository,
            &publication.head_branch,
            &publication.head_commit,
            &publication.base_branch,
            &observed,
        )?;
        let commit = self
            .hosting()?
            .fetch_branch(root, &stack.destination, &stack.base_branch)?;
        if commit != publication.head_commit {
            return Err(error(
                "predecessor branch moved from its verified completed commit",
            ));
        }
        Ok(())
    }

    /// Resolve once, then atomically pin against the still-current stack and prerequisites.
    /// # Errors
    /// Returns hosting, dependency, drift, scope or concurrent-configuration failures.
    pub fn prepare_task_stack(&self, record: u64) -> Result<Option<TaskStack>, RegistrationError> {
        let Some(stack) = self.task_stack(record)? else {
            return Ok(None);
        };
        let root = self.stack_root(record)?;
        self.verify_predecessor(&stack, &root)?;
        if stack.base_commit.is_some() {
            return Ok(Some(stack));
        }
        let commit = self
            .hosting()?
            .fetch_branch(&root, &stack.destination, &stack.base_branch)?;
        if let Some(predecessor) = stack.predecessor {
            let previous = self
                .task_stack(predecessor)?
                .and_then(|s| s.publication)
                .ok_or_else(|| error("predecessor publication missing"))?;
            if commit != previous.head_commit {
                return Err(error("predecessor branch moved during preparation"));
            }
        }
        self.records.pin_task_stack(&stack, &commit).map(Some)
    }

    /// Verify an already-published non-draft PR; never trust agent-reported hosting facts.
    /// # Errors
    /// Rejects mismatched hosting facts, stale evidence, incomplete reviews or unsafe ownership.
    pub fn publish_task_stack(
        &self,
        record: u64,
        url: &str,
        assignment: Option<&str>,
    ) -> Result<TaskStack, RegistrationError> {
        let stack = self
            .task_stack(record)?
            .ok_or_else(|| error("Task is not stacked"))?;
        let root = self.stack_root(record)?;
        self.verify_predecessor(&stack, &root)?;
        let work = self.record_work_state(record)?;
        let Some(Evidence::Repository { commit, .. }) = &work.output else {
            return Err(error("publication requires repository output evidence"));
        };
        let observed = self
            .hosting()?
            .pull_request(&root, &stack.destination.repository, url)?;
        verify_pr(
            &stack.destination.repository,
            &task_branch(record),
            commit,
            &stack.base_branch,
            &observed,
        )?;
        let remote_head =
            self.hosting()?
                .fetch_branch(&root, &stack.destination, &task_branch(record))?;
        if &remote_head != commit {
            return Err(error("published branch does not match checked output"));
        }
        let publication = Publication {
            repository: observed.repository,
            url: observed.url,
            head_branch: observed.head_branch,
            head_commit: observed.head_commit,
            base_branch: observed.base_branch,
            generation: work.generation,
        };
        self.records
            .record_stack_publication(&stack, &publication, assignment)
    }

    /// Observe again before completion; persistence separately rechecks pinned evidence.
    pub(crate) fn verify_stack_completion(&self, record: u64) -> Result<(), RegistrationError> {
        let Some(stack) = self.task_stack(record)? else {
            return Ok(());
        };
        let publication = stack
            .publication
            .as_ref()
            .ok_or_else(|| error("verified publication missing"))?;
        let root = self.stack_root(record)?;
        self.verify_predecessor(&stack, &root)?;
        let observed =
            self.hosting()?
                .pull_request(&root, &publication.repository, &publication.url)?;
        verify_pr(
            &stack.destination.repository,
            &task_branch(record),
            &publication.head_commit,
            &stack.base_branch,
            &observed,
        )?;
        if self
            .hosting()?
            .fetch_branch(&root, &stack.destination, &task_branch(record))?
            != publication.head_commit
        {
            return Err(error("published branch moved after verification"));
        }
        Ok(())
    }
}

fn verify_pr(
    repository: &str,
    head: &str,
    commit: &str,
    base: &str,
    observed: &PullRequestObservation,
) -> Result<(), RegistrationError> {
    if !observed.open
        || observed.draft
        || observed.repository != repository
        || observed.head_repository != repository
        || observed.head_branch != head
        || observed.head_commit != commit
        || observed.base_branch != base
    {
        return Err(error(
            "expected an open non-draft PR with the exact repository, head branch/commit and base branch",
        ));
    }
    Ok(())
}
