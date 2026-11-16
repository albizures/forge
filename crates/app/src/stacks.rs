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

/// Independently observed, unchanged repository output at a particular generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnchangedResult {
    pub evidence: Evidence,
    pub generation: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckoutObservation {
    pub commit: String,
    pub clean: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskStack {
    pub record_id: u64,
    pub predecessor: Option<u64>,
    pub destination: StackDestination,
    pub base_branch: String,
    pub base_commit: Option<String>,
    pub publication: Option<Publication>,
    #[serde(default)]
    pub unchanged_result: Option<UnchangedResult>,
}

impl TaskStack {
    /// Resolve retained completion evidence against the current work generation.
    /// # Errors
    /// Rejects missing, conflicting, stale or contradictory repository results.
    pub fn verified_commit<'a>(
        &'a self,
        work: &crate::outcomes::WorkState,
    ) -> Result<&'a str, RegistrationError> {
        let commit = match (&self.publication, &self.unchanged_result) {
            (Some(publication), None) if publication.generation == work.generation => {
                &publication.head_commit
            }
            (None, Some(result))
                if result.generation == work.generation
                    && work.output.as_ref() == Some(&result.evidence) =>
            {
                let Evidence::Repository { commit, .. } = &result.evidence else {
                    return Err(error("unchanged result is not repository evidence"));
                };
                if self.base_commit.as_ref() != Some(commit) {
                    return Err(error("unchanged result differs from prepared input"));
                }
                commit
            }
            _ => {
                return Err(error(
                    "verified predecessor result is missing, conflicting or stale",
                ));
            }
        };
        if !matches!(&work.output, Some(Evidence::Repository { commit: output, .. }) if output == commit)
        {
            return Err(error(
                "verified predecessor result differs from current output",
            ));
        }
        Ok(commit)
    }
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
    /// Observe a Manual Task's registered Project checkout without modifying it.
    /// # Errors
    /// Rejects unavailable or invalid repositories. Unsupported observers fail closed.
    fn checkout(&self, _root: &Path) -> Result<CheckoutObservation, RegistrationError> {
        Err(error("checkout observation is unavailable"))
    }
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
    validate_stack_task(task)?;
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

/// Validate the Task boundary independently of execution completeness.
/// # Errors
/// Rejects non-Task or non-single-Project configuration.
pub fn validate_stack_task(task: &crate::Record) -> Result<(), RegistrationError> {
    if task.record_type != crate::RecordType::Task || task.project_slugs.len() != 1 {
        return Err(error("stack requires a Task scoped to exactly one Project"));
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
        || task.record_type != crate::RecordType::Task
        || task.project_slugs.len() != 1
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

    /// Configure root execution inputs before any affected Task has prepared execution.
    /// # Errors
    /// Rejects invalid destinations, predecessor declarations or prepared Tasks.
    pub fn configure_task_stack(
        &self,
        record: u64,
        source: &StackSource,
    ) -> Result<TaskStack, RegistrationError> {
        if let StackSource::Root { destination } = source {
            destination.validate()?;
        }
        let task = self.records.show_record(record)?;
        validate_stack_task(&task)?;
        if matches!(source, StackSource::Predecessor { .. }) {
            return Err(error(
                "Stack predecessor configuration is no longer supported; use record relate --kind builds-on --to <predecessor>",
            ));
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
        self.verify_predecessor_lineage(stack, root)
            .map_err(|failure| {
                RegistrationError::new(
                    failure.code,
                    format!(
                        "Task {}, Stack Predecessor {:?}, Base Branch {}, pinned commit {:?}: {}",
                        stack.record_id,
                        stack.predecessor,
                        stack.base_branch,
                        stack.base_commit,
                        failure.message
                    ),
                )
            })
    }

    fn verify_predecessor_lineage(
        &self,
        stack: &TaskStack,
        root: &Path,
    ) -> Result<(), RegistrationError> {
        if stack.predecessor.is_none() {
            return Ok(());
        }
        let mut cursor = stack.predecessor;
        let mut expected = stack.base_commit.clone();
        let mut visited = std::collections::HashSet::from([stack.record_id]);
        while let Some(predecessor) = cursor {
            if !visited.insert(predecessor) {
                return Err(error("stack cycle"));
            }
            if self.records.show_record(predecessor)?.status != RecordStatus::Done {
                return Err(error(format!(
                    "Stack Predecessor {predecessor} is not Done"
                )));
            }
            let previous = self
                .task_stack(predecessor)?
                .ok_or_else(|| error("predecessor is not stacked"))?;
            let work = self.record_work_state(predecessor)?;
            let commit = previous.verified_commit(&work).map_err(|failure| {
                error(format!(
                    "Stack Predecessor {predecessor}, generation {}, observed output {:?}: {}",
                    work.generation, work.output, failure.message
                ))
            })?;
            if expected.as_ref().is_some_and(|expected| expected != commit) {
                return Err(error(format!(
                    "Stack Predecessor {predecessor}: expected commit {}, verified {commit}",
                    expected.as_deref().unwrap_or_default()
                )));
            }
            expected = Some(commit.to_owned());
            if let Some(publication) = &previous.publication {
                let observed = self.hosting()?.pull_request(
                    root,
                    &publication.repository,
                    &publication.url,
                )?;
                verify_pr(
                    &previous.destination.repository,
                    &task_branch(predecessor),
                    commit,
                    &previous.base_branch,
                    &observed,
                )?;
                if stack.base_branch != publication.head_branch {
                    return Err(error(
                        "retained publication base contradicts predecessor lineage",
                    ));
                }
                break;
            }
            if stack.base_branch != previous.base_branch {
                return Err(error(
                    "unchanged predecessor has contradictory base lineage",
                ));
            }
            cursor = previous.predecessor;
        }
        let observed = self
            .hosting()?
            .fetch_branch(root, &stack.destination, &stack.base_branch)
            .map_err(|failure| {
                RegistrationError::new(
                    failure.code,
                    format!(
                        "expected commit {:?}; branch observation unavailable: {}",
                        expected, failure.message
                    ),
                )
            })?;
        if expected
            .as_ref()
            .is_some_and(|expected| expected != &observed)
        {
            return Err(error(format!(
                "Stack Predecessor {:?}, branch {}: expected commit {}, observed {observed}",
                stack.predecessor,
                stack.base_branch,
                expected.as_deref().unwrap_or_default()
            )));
        }
        Ok(())
    }

    /// Re-observe inherited hosting evidence at execution entry, without repinning inputs.
    /// Root branch movement after preparation does not change a running Task's input.
    pub(crate) fn verify_stack_execution(&self, record: u64) -> Result<(), RegistrationError> {
        self.records.validate_task_execution_plan(record)?;
        if let Some(stack) = self.task_stack(record)? {
            self.verify_predecessor(&stack, &self.stack_root(record)?)?;
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
        self.records.validate_task_execution_plan(record)?;
        let root = self.stack_root(record)?;
        self.verify_predecessor(&stack, &root)?;
        let commit = self
            .hosting()?
            .fetch_branch(&root, &stack.destination, &stack.base_branch)
            .map_err(|failure| {
                RegistrationError::new(
                    failure.code,
                    format!(
                        "Task {record}, Base Branch {}, pinned commit {:?}: {}",
                        stack.base_branch, stack.base_commit, failure.message
                    ),
                )
            })?;
        if let Some(expected) = &stack.base_commit {
            if expected != &commit {
                return Err(error(format!(
                    "prepared branch {}: expected commit {expected}, observed {commit}",
                    stack.base_branch
                )));
            }
            return Ok(Some(stack));
        }
        if let Some(predecessor) = stack.predecessor {
            let previous = self
                .task_stack(predecessor)?
                .ok_or_else(|| error("predecessor is not stacked"))?;
            let work = self.record_work_state(predecessor)?;
            if commit != previous.verified_commit(&work)? {
                return Err(error(format!(
                    "Stack Predecessor {predecessor} branch moved during preparation"
                )));
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

    fn unchanged_evidence(&self, record: u64) -> Result<Evidence, RegistrationError> {
        let task = self.records.show_record(record)?;
        if task.execution_mode == crate::ExecutionMode::Dispatched {
            return self.observe_task_evidence(record);
        }
        let root = self.stack_root(record)?;
        let observed = self.hosting()?.checkout(&root)?;
        if !observed.clean {
            return Err(error("unchanged publication requires a clean checkout"));
        }
        let evidence = Evidence::Repository {
            repository: root.display().to_string(),
            commit: observed.commit,
        };
        evidence.validate()?;
        Ok(evidence)
    }

    /// Independently verify unchanged output; this records evidence, not a Check verdict.
    /// The bound Publication attempt must still submit its ordinary passing outcome.
    /// # Errors
    /// Rejects dirty, changed, stale, unreviewed or unauthorized work.
    pub fn publish_unchanged_task_stack(
        &self,
        record: u64,
        assignment: Option<&str>,
    ) -> Result<TaskStack, RegistrationError> {
        let stack = self
            .task_stack(record)?
            .ok_or_else(|| error("Task is not stacked"))?;
        self.verify_predecessor(&stack, &self.stack_root(record)?)?;
        let work = self.record_work_state(record)?;
        let evidence = self.unchanged_evidence(record)?;
        if work.output.as_ref() != Some(&evidence)
            || !matches!(&evidence, Evidence::Repository { commit, .. } if Some(commit) == stack.base_commit.as_ref())
        {
            return Err(error(
                "observed output must exactly equal the prepared input commit",
            ));
        }
        self.records.record_stack_unchanged(
            &stack,
            &UnchangedResult {
                evidence,
                generation: work.generation,
            },
            assignment,
        )
    }

    /// Observe again before completion; persistence separately rechecks pinned evidence.
    pub(crate) fn verify_stack_completion(&self, record: u64) -> Result<(), RegistrationError> {
        let Some(stack) = self.task_stack(record)? else {
            return Ok(());
        };
        if let Some(result) = &stack.unchanged_result {
            self.verify_predecessor(&stack, &self.stack_root(record)?)?;
            let work = self.record_work_state(record)?;
            if result.generation != work.generation
                || work.output.as_ref() != Some(&result.evidence)
                || self.unchanged_evidence(record)? != result.evidence
            {
                return Err(error("unchanged publication evidence is stale"));
            }
            return Ok(());
        }
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
        return Err(error(format!(
            "expected an open non-draft PR in {repository} with head {head} at {commit} and base {base}; observed repository {}, head repository {}, head {} at {}, base {}, open {}, draft {}",
            observed.repository,
            observed.head_repository,
            observed.head_branch,
            observed.head_commit,
            observed.base_branch,
            observed.open,
            observed.draft
        )));
    }
    Ok(())
}
