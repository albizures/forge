//! Application-owned operations and persistence contracts.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub mod agent_profiles;
pub mod assignment;
pub mod check_templates;
pub mod checks;
pub mod findings;
pub mod outcomes;
mod profile_catalog;
mod task_attempts;
mod workspaces;
pub use workspaces::{Workspace, WorkspaceRepository};
mod workspace_dispatch;
pub use workspace_dispatch::{
    DispatchBlocker, ProfilePreflightBlocker, WorkspaceDispatchConfiguration,
    WorkspaceDispatchReadiness, WorkspaceDispatchRepository,
};
mod projects;
pub use projects::{ProjectReference, canonical_project_root};
mod record_planning;
pub use record_planning::RecordReadiness;

/// A Project registered with Forge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    /// Immutable persistence identity, carried by scope, configuration and Assignments.
    pub reference: String,
    /// Immutable owning Workspace.
    pub workspace_id: String,
    /// Immutable human-referenceable slug, unique within the owning Workspace.
    pub slug: String,
    /// Canonical Project root: the Repository's main checkout.
    pub project_root: PathBuf,
    /// URL configured for the Repository's `origin` remote.
    pub origin_url: String,
}

/// The initial lifecycle status of a Record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordStatus {
    Open,
    InProgress,
    Done,
}

/// The currently supported kind of typed Record Relationship.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordRelationshipKind {
    ParentChild,
    DependsOn,
    GeneratedBy,
    /// Non-hierarchical membership of a Spec or Wayfinder in an Initiative.
    Initiative,
}

/// An endpoint-neutral typed Relationship between two Records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordRelationship {
    pub kind: RecordRelationshipKind,
    /// The directional source. For `DependsOn`, this is the blocked Record; for
    /// `GeneratedBy`, this is the generated Record.
    pub from_id: u64,
    /// The directional target. For `DependsOn`, this is the prerequisite Record;
    /// for `GeneratedBy`, this is the generator Record.
    pub to_id: u64,
    /// Hierarchy compatibility field, populated only for `ParentChild`.
    pub parent_id: u64,
    /// Hierarchy compatibility field, populated only for `ParentChild`.
    pub child_id: u64,
}

/// The immutable kind selected when a Record opens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordType {
    Generic,
    Spec,
    Task,
    Wayfinder,
    Grilling,
    Initiative,
}

/// Explicit execution policy, independent of lifecycle and ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    Manual,
    Dispatched,
}

impl ExecutionMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Dispatched => "dispatched",
        }
    }
}

/// Validate Task mode and Project eligibility without authorizing unsupported dispatch.
///
/// # Errors
/// Rejects Manual Tasks and Projectless Tasks, in that order.
pub fn validate_task_dispatch_mode(record: &Record) -> Result<(), RegistrationError> {
    if record.execution_mode == ExecutionMode::Manual {
        return Err(RegistrationError::new(
            "manual-dispatch-forbidden",
            "Manual Tasks cannot be dispatched; explicitly select Dispatched at a safe boundary",
        ));
    }
    if record.project_slugs.is_empty() {
        return Err(RegistrationError::new(
            "dispatch-requires-project",
            "Task dispatch requires a Project",
        ));
    }
    Ok(())
}

/// A unit of work whose immutable type governs its scope and hierarchy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Immutable owning Forge Workspace.
    pub workspace_id: String,
    /// Opaque, positive identity assigned by persistence.
    pub id: u64,
    /// Immutable type selected at opening.
    pub record_type: RecordType,
    /// Required Markdown content, separate from its structured metadata.
    pub body: String,
    /// Immutable, lexically ordered Project Scope selected at opening.
    pub project_slugs: Vec<String>,
    /// Explicit policy; never inferred from status or absence of Assignments.
    pub execution_mode: ExecutionMode,
    /// The Record's lifecycle status.
    pub status: RecordStatus,
    /// Immutable optional explanation recorded when the Record was completed.
    pub completion_resolution: Option<String>,
    /// All Relationships incident to this Record.
    pub relationships: Vec<RecordRelationship>,
}

/// A Record before persistence assigns its identity and initial status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewRecord {
    pub body: String,
    pub project_slugs: Vec<String>,
    /// Records which caused this Record to be opened.
    pub generated_by_ids: Vec<u64>,
}

/// A non-generic Record before persistence assigns its identity and initial status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewTypedRecord {
    pub record_type: RecordType,
    pub body: String,
    pub project_slugs: Vec<String>,
    /// Records which caused this Record to be opened.
    pub generated_by_ids: Vec<u64>,
}

/// Optional discovery constraints for Records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordListFilter {
    pub project_slug: Option<String>,
    pub status: Option<RecordStatus>,
    pub record_type: Option<RecordType>,
}

/// Opaque stable identity assigned to a Forge-authored Record Update.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RecordUpdateId(pub u64);

/// The minimal fact recorded for a successful Record mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordUpdate {
    pub id: RecordUpdateId,
    /// Unix milliseconds when Forge committed this update.
    pub created_at_millis: u64,
    pub kind: RecordUpdateKind,
}

/// Opaque stable identity assigned to a Record Comment.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RecordCommentId(pub u64);

/// Immutable free-text discussion attached to a Record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordComment {
    pub id: RecordCommentId,
    pub created_at_millis: u64,
    pub author: Option<String>,
    pub body: String,
}

/// The change represented by a Record Update. Deliberately excludes content snapshots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordUpdateKind {
    Created,
    BodyEdited,
    ExecutionModeChanged {
        from: ExecutionMode,
        to: ExecutionMode,
        actor: String,
        message: String,
    },
    CheckAssessed {
        assessment_id: u64,
    },
    CheckInvalidated {
        assessment_id: u64,
    },
    FindingActivity {
        activity_id: u64,
    },
    StatusTransition {
        from: RecordStatus,
        to: RecordStatus,
    },
    RelationshipAdded {
        related_record_id: u64,
        /// Absent only for legacy persisted updates whose kind is unknowable.
        relationship_kind: Option<RecordRelationshipKind>,
    },
    RelationshipRemoved {
        related_record_id: u64,
        /// Absent only for legacy persisted updates whose kind is unknowable.
        relationship_kind: Option<RecordRelationshipKind>,
    },
    CommentAdded {
        comment_id: RecordCommentId,
    },
    CommentDeleted {
        comment_id: RecordCommentId,
    },
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
    /// Resolves the immutable Git `HEAD` commit for a registered Project folder.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when commit resolution is unsupported or fails.
    fn head_commit(&self, folder: &Path) -> Result<String, RegistrationError> {
        let _ = folder;
        Err(RegistrationError::new(
            "head-unavailable",
            "the configured repository inspector cannot resolve a Task Base Commit",
        ))
    }
}

/// Persistence boundary owned by the application.
#[allow(clippy::missing_errors_doc)]
pub trait ProjectRepository: Send + Sync {
    /// Returns a Project by its immutable persistence reference, not its local slug.
    fn project(&self, reference: &str) -> Result<Project, RegistrationError>;
    /// Resolves an operator-facing Workspace-local slug to the immutable persistence identity.
    /// # Errors
    /// Returns missing Project or storage failures.
    fn project_in_workspace(
        &self,
        workspace: &str,
        slug: &str,
    ) -> Result<Project, RegistrationError> {
        self.project(&ProjectReference::new(workspace, slug)?.qualified())
    }
    /// Stores `project`, or returns the matching existing Project idempotently.
    ///
    /// # Errors
    ///
    /// Returns a user-facing validation or storage failure.
    fn register(&self, project: &Project) -> Result<Project, RegistrationError>;
    /// Finds the Project whose deepest Project root contains `folder`.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when no Project contains the folder.
    fn find_containing(&self, folder: &Path) -> Result<Project, RegistrationError>;
}

/// Durable state of a Worktree Claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorktreeClaimState {
    Reserving,
    Active,
    CleanupRequired,
}

/// Forge's exclusive durable association of one Record with a Managed Worktree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeClaim {
    pub record_id: u64,
    pub project_slug: String,
    pub path: PathBuf,
    pub state: WorktreeClaimState,
}

/// Returns whether a Record may own a Dispatcher-created Managed Worktree.
///
/// A Provisioning Assignment owns dispatch capacity while its Task deliberately remains Open.
#[must_use]
pub fn is_worktree_eligible(record: &Record) -> bool {
    record.record_type == RecordType::Task
        && record.status == RecordStatus::Open
        && record.project_slugs.len() == 1
}

/// Derives the only path Forge may use for a Record's Managed Worktree.
#[must_use]
pub fn managed_worktree_path(root: &Path, project_slug: &str, record_id: u64) -> PathBuf {
    root.join(".forge/worktrees")
        .join(project_slug)
        .join(record_id.to_string())
}

/// Validates the fixed placement against canonical existing ancestors before Git or filesystem
/// effects. Symlink redirection (including dangling links) is never used for managed checkouts.
///
/// # Errors
/// Returns an actionable error for missing Workspace roots or redirected path components.
pub fn validate_managed_worktree_placement(path: &Path) -> Result<(), RegistrationError> {
    let invalid = || {
        RegistrationError::new(
            "managed-worktree-path-invalid",
            "Managed Worktree placement is missing or redirected by a symlink; restore the fixed Workspace path before retrying",
        )
    };
    let root = path.ancestors().nth(4).ok_or_else(invalid)?;
    if std::fs::canonicalize(root).map_err(|_| invalid())? != root {
        return Err(invalid());
    }
    for ancestor in path.ancestors().take(4) {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => {
                if std::fs::canonicalize(ancestor).map_err(|_| invalid())? != ancestor {
                    return Err(invalid());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(invalid()),
        }
    }
    Ok(())
}

/// Persistence boundary for managed-worktree configuration and claims.
#[allow(clippy::missing_errors_doc)]
pub trait ManagedWorktreeRepository: RecordRepository {
    /// Returns the fixed Workspace-rooted placement for a single-Project Task.
    fn worktree_path(&self, record_id: u64) -> Result<PathBuf, RegistrationError>;
    /// Rejects removal while an Assignment or unresolved resources retain the checkout.
    fn ensure_worktree_removable(&self, record_id: u64) -> Result<(), RegistrationError>;
    /// Sets or clears a Project Setup Hook.
    fn set_project_setup_hook(
        &self,
        project_slug: &str,
        hook: Option<&str>,
    ) -> Result<(), RegistrationError>;
    /// Returns a Project's Setup Hook.
    fn project_setup_hook(&self, project_slug: &str) -> Result<Option<String>, RegistrationError>;
    /// Atomically claim a validated existing checkout while dispatch is held.
    fn recover_worktree_claim(
        &self,
        _record_id: u64,
        _path: &Path,
    ) -> Result<WorktreeClaim, RegistrationError> {
        Err(RegistrationError::new(
            "recovery-unavailable",
            "Worktree recovery is unavailable",
        ))
    }
    /// Atomically reserves the sole Claim for an eligible Record.
    fn reserve_worktree_claim(&self, record_id: u64) -> Result<WorktreeClaim, RegistrationError>;
    /// Marks a reserved Claim active after its Managed Worktree exists.
    fn activate_worktree_claim(&self, record_id: u64) -> Result<WorktreeClaim, RegistrationError>;
    /// Marks a Claim for recovery-oriented cleanup without deleting its evidence.
    fn require_worktree_claim_cleanup(
        &self,
        record_id: u64,
    ) -> Result<WorktreeClaim, RegistrationError>;
    /// Returns one Claim, including Claims retained after Record completion.
    fn worktree_claim(&self, record_id: u64) -> Result<Option<WorktreeClaim>, RegistrationError>;
    /// Lists all Claims, including Claims retained after Record completion.
    fn worktree_claims(&self) -> Result<Vec<WorktreeClaim>, RegistrationError>;
    /// Lists Claims which need recovery work.
    fn worktree_claims_requiring_cleanup(&self) -> Result<Vec<WorktreeClaim>, RegistrationError>;
    /// Deletes a Claim only after its checkout has been safely removed.
    fn delete_worktree_claim(&self, record_id: u64) -> Result<(), RegistrationError>;
}

/// Local, offline evidence supporting a Managed Worktree removal decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeSafetyAssessment {
    pub facts: Vec<String>,
    pub blockers: Vec<String>,
    pub removable: bool,
}

/// Server-owned Git and platform-shell operations for Managed Worktrees.
#[allow(clippy::missing_errors_doc)]
pub trait ManagedWorktreeAdapter: Send + Sync {
    fn validate_recovery(
        &self,
        _repository: &Path,
        _path: &Path,
        _branch: &str,
        _base: &str,
    ) -> Result<(), RegistrationError> {
        Err(RegistrationError::new(
            "recovery-unavailable",
            "Worktree recovery validation is unavailable",
        ))
    }
    fn create(
        &self,
        repository: &Path,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> Result<(), RegistrationError>;
    fn remove(&self, path: &Path, force: bool) -> Result<(), RegistrationError>;
    fn assess(&self, path: &Path) -> WorktreeSafetyAssessment;
    fn run_setup_hook(
        &self,
        path: &Path,
        hook: &str,
        project_slug: &str,
        record_id: u64,
        branch: &str,
        base: &str,
    ) -> Result<(), RegistrationError>;
}

/// A Claim together with its current local removal assessment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedWorktreeInfo {
    pub claim: WorktreeClaim,
    pub safety: WorktreeSafetyAssessment,
}

/// Persistence operations for generic Records.
pub trait RecordRepository: ProjectRepository {
    /// Read current Project criteria and bounded attributed replacement history.
    ///
    /// # Errors
    /// Returns scope, pagination, storage or unsupported-operation errors.
    fn show_check_template(
        &self,
        _project: &str,
        _after: Option<u64>,
        _limit: u32,
    ) -> Result<check_templates::TemplatePage, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Check Templates are not supported",
        ))
    }
    /// Atomically replace the whole Project template at the observed version.
    ///
    /// # Errors
    /// Returns validation, scope, stale-version, storage or unsupported-operation errors.
    fn replace_check_template(
        &self,
        _project: &str,
        _input: &check_templates::TemplateReplacement,
    ) -> Result<check_templates::CheckTemplate, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Check Templates are not supported",
        ))
    }
    /// Raise a Finding and its immutable origin activity atomically.
    /// # Errors
    /// Returns validation, scope, origin, frozen-state or storage errors.
    fn raise_record_finding(
        &self,
        _task: u64,
        _input: &findings::FindingRaiseInput,
    ) -> Result<findings::Finding, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Findings are not supported",
        ))
    }
    /// Resolve an unresolved Finding at the observed version.
    /// # Errors
    /// Returns validation, scope, stale-version, transition or storage errors.
    fn resolve_record_finding(
        &self,
        _task: u64,
        _finding: u64,
        _input: &findings::FindingResolveInput,
    ) -> Result<findings::Finding, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Findings are not supported",
        ))
    }
    /// Append a version-checked Finding review action atomically.
    /// # Errors
    /// Returns validation, scope, stale-version, transition, frozen-state or storage errors.
    fn mutate_record_finding(
        &self,
        _task: u64,
        _finding: u64,
        _input: &findings::FindingMutationInput,
    ) -> Result<findings::Finding, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Findings are not supported",
        ))
    }
    /// Raise a Finding with authenticated Assignment provenance.
    /// # Errors
    /// Returns unsupported-operation or normal Finding validation errors.
    fn raise_assignment_finding(
        &self,
        _task: u64,
        _input: &findings::FindingRaiseInput,
        _assignment: &str,
    ) -> Result<findings::Finding, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Assignment Findings are not supported",
        ))
    }
    /// Append a review action with authenticated Assignment provenance.
    /// # Errors
    /// Returns unsupported-operation or normal Finding validation errors.
    fn mutate_assignment_finding(
        &self,
        _task: u64,
        _finding: u64,
        _input: &findings::FindingMutationInput,
        _assignment: &str,
    ) -> Result<findings::Finding, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Assignment Findings are not supported",
        ))
    }
    /// List a bounded page ordered by Finding ID.
    /// # Errors
    /// Returns scope, type, pagination or storage errors.
    fn list_record_findings(
        &self,
        _task: u64,
        _after: Option<u64>,
        _limit: u32,
    ) -> Result<findings::FindingList, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Findings are not supported",
        ))
    }
    /// Inspect a Finding and bounded activity history.
    /// # Errors
    /// Returns scope, type, ownership, pagination or storage errors.
    fn show_record_finding(
        &self,
        _task: u64,
        _finding: u64,
        _after: Option<u64>,
        _limit: u32,
    ) -> Result<findings::FindingPage, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Findings are not supported",
        ))
    }
    /// Inspect a Task's current work evidence and freshness.
    /// # Errors
    /// Returns unsupported, scope or storage errors.
    fn record_work_state(&self, _record: u64) -> Result<outcomes::WorkState, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Manual outcomes unavailable",
        ))
    }
    /// Begin an evidence-bound Manual attempt.
    /// # Errors
    /// Returns validation, ordering, ownership or storage errors.
    fn begin_record_attempt(
        &self,
        _record: u64,
        _input: &outcomes::AttemptInput,
    ) -> Result<outcomes::Attempt, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Manual outcomes unavailable",
        ))
    }
    /// Authenticated same-owner outcome acceptance; transport credentials are checked by the application.
    /// # Errors
    /// Rejects unbound, cross-owner, stale, unauthorized and conflicting outcomes atomically.
    fn submit_assignment_outcome(
        &self,
        _record: u64,
        _assignment: &str,
        _input: &outcomes::OutcomeInput,
    ) -> Result<outcomes::AcceptedOutcome, RegistrationError> {
        Err(RegistrationError::new(
            "assignment-outcome-required",
            "Check-bound Assignment required",
        ))
    }
    /// Atomically submit an attempt's outcome, with durable exact replay.
    /// # Errors
    /// Returns validation, freshness, ownership or storage errors.
    fn submit_record_outcome(
        &self,
        _record: u64,
        _input: &outcomes::OutcomeInput,
    ) -> Result<outcomes::AcceptedOutcome, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Manual outcomes unavailable",
        ))
    }
    /// Invalidate current work, retaining all immutable history.
    /// # Errors
    /// Returns validation, freshness, ownership or storage errors.
    fn invalidate_record_work(
        &self,
        _record: u64,
        _input: &outcomes::WorkInvalidationInput,
    ) -> Result<outcomes::WorkState, RegistrationError> {
        Err(RegistrationError::new(
            "unsupported",
            "Manual outcomes unavailable",
        ))
    }
    /// Inspect immutable completion criteria for a Task.
    /// # Errors
    /// Returns scope, Record type or storage errors.
    fn list_record_checks(
        &self,
        _task: u64,
    ) -> Result<Vec<checks::RecordCheck>, RegistrationError> {
        Err(RegistrationError::new(
            "checks-unavailable",
            "Task Checks are unavailable",
        ))
    }
    /// Inspect one Check and an ascending, bounded Assessment history page.
    ///
    /// # Errors
    /// Returns scope, Check ownership, pagination, or storage errors.
    fn show_record_check(
        &self,
        _task: u64,
        _check: u64,
        _after: Option<u64>,
        _limit: u32,
    ) -> Result<checks::CheckPage, RegistrationError> {
        Err(RegistrationError::new(
            "checks-unavailable",
            "Task Checks are unavailable",
        ))
    }
    /// Reject an unbound Assessment; accepted Assessments require the outcome boundary.
    ///
    /// # Errors
    /// Returns `attempt-required` or normal validation/scope/version errors.
    fn assess_record_check(
        &self,
        _task: u64,
        _check: u64,
        _input: &checks::AssessmentInput,
    ) -> Result<checks::RecordCheck, RegistrationError> {
        Err(RegistrationError::new(
            "checks-unavailable",
            "Task Checks are unavailable",
        ))
    }
    /// Reject unbound Assignment Assessments; live Check-bound outcomes remain unsupported.
    ///
    /// # Errors
    /// Returns `attempt-required`, unsupported-operation or normal Check validation errors.
    fn assess_assignment_check(
        &self,
        _task: u64,
        _check: u64,
        _input: &checks::AssessmentInput,
        _assignment: &str,
    ) -> Result<checks::RecordCheck, RegistrationError> {
        Err(RegistrationError::new(
            "checks-unavailable",
            "Assignment Checks are unavailable",
        ))
    }
    /// Invalidate with authenticated Assignment provenance.
    ///
    /// # Errors
    /// Returns unsupported-operation or normal Check validation errors.
    fn invalidate_assignment_check(
        &self,
        _task: u64,
        _check: u64,
        _input: &checks::InvalidationInput,
        _assignment: &str,
    ) -> Result<checks::RecordCheck, RegistrationError> {
        Err(RegistrationError::new(
            "checks-unavailable",
            "Assignment Checks are unavailable",
        ))
    }
    /// Invalidate the latest active Assessment atomically with its Record Update.
    ///
    /// # Errors
    /// Returns validation, scope, stale version, transition or storage errors.
    fn invalidate_record_check(
        &self,
        _task: u64,
        _check: u64,
        _input: &checks::InvalidationInput,
    ) -> Result<checks::RecordCheck, RegistrationError> {
        Err(RegistrationError::new(
            "checks-unavailable",
            "Check invalidation is unavailable",
        ))
    }
    /// Returns a request-local repository confined to an existing Workspace.
    ///
    /// # Errors
    /// Returns an error when Workspace selection is unsupported or invalid.
    fn records_in_workspace(
        &self,
        workspace: &str,
    ) -> Result<Box<dyn RecordRepository>, RegistrationError> {
        let _ = workspace;
        Err(RegistrationError::new(
            "workspace-records-unavailable",
            "Workspace-confined Records are unavailable",
        ))
    }

    /// Opens a Record after validating that all selected Projects exist.
    ///
    /// # Errors
    ///
    /// Returns a user-facing validation or storage failure.
    fn open_record(&self, record: &NewRecord) -> Result<Record, RegistrationError>;
    /// Opens a non-generic Record after validating its type-specific invariants.
    ///
    /// # Errors
    ///
    /// Returns a user-facing validation or storage failure.
    fn open_typed_record(&self, record: &NewTypedRecord) -> Result<Record, RegistrationError>;
    /// Persists the immutable Task Base Commit captured when the Task opens.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when persistence is unsupported or storage fails.
    fn set_task_base_commit(&self, record_id: u64, commit: &str) -> Result<(), RegistrationError> {
        let _ = (record_id, commit);
        Err(RegistrationError::new(
            "base-commit-unavailable",
            "Task Base Commit persistence is unavailable",
        ))
    }
    /// Returns the immutable Task Base Commit, if the Task has Project scope.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when lookup is unsupported or storage fails.
    fn task_base_commit(&self, record_id: u64) -> Result<Option<String>, RegistrationError> {
        let _ = record_id;
        Err(RegistrationError::new(
            "base-commit-unavailable",
            "Task Base Commit persistence is unavailable",
        ))
    }
    /// Opens a Task beneath a Spec, assigning the Spec's scope and membership atomically.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the parent is not a Spec or storage fails.
    fn open_task_under_spec(
        &self,
        body: &str,
        spec_id: u64,
        generated_by_ids: Vec<u64>,
    ) -> Result<Record, RegistrationError>;
    /// Opens a Task beneath a Wayfinder, selecting one Project from its scope.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the parent or selected Project is invalid.
    fn open_task_under_wayfinder(
        &self,
        body: &str,
        wayfinder_id: u64,
        project_slug: Option<String>,
        generated_by_ids: Vec<u64>,
    ) -> Result<Record, RegistrationError>;
    /// Opens a Grilling beneath a Wayfinder, inheriting or selecting its Project Scope.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the parent or selected scope is invalid.
    fn open_grilling_under_wayfinder(
        &self,
        body: &str,
        wayfinder_id: u64,
        project_slugs: Option<Vec<String>>,
        generated_by_ids: Vec<u64>,
    ) -> Result<Record, RegistrationError>;
    /// Lists Records matching Project Scope membership and lifecycle status.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when storage fails.
    fn list_records(&self, filter: &RecordListFilter) -> Result<Vec<Record>, RegistrationError>;
    /// Returns the Record identified by `id`.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the Record is absent or storage fails.
    fn show_record(&self, id: u64) -> Result<Record, RegistrationError>;
    /// Persists a non-Done Record Body.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the Record is Done, absent, or storage fails.
    fn edit_record(&self, id: u64, body: &str) -> Result<Record, RegistrationError>;
    /// Advances a Record from Open to In Progress.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for an invalid transition, absent Record, or storage failure.
    fn start_record(&self, id: u64) -> Result<Record, RegistrationError>;

    /// Start only under a matching open Assignment after capability authorization.
    /// # Errors
    /// Returns ownership, lifecycle or storage errors.
    fn start_owned_record(&self, id: u64, assignment: &str) -> Result<Record, RegistrationError> {
        let _ = assignment;
        self.start_record(id)
    }

    /// Change explicit execution policy at a safe between-attempt boundary.
    ///
    /// # Errors
    /// Rejects unsafe ownership/resources, open attempts, invalid profiles or stale mode.
    fn set_record_execution_mode(
        &self,
        id: u64,
        expected: ExecutionMode,
        mode: ExecutionMode,
        actor: &str,
        message: &str,
    ) -> Result<Record, RegistrationError> {
        let _ = (id, expected, mode, actor, message);
        Err(RegistrationError::new(
            "unsupported",
            "Execution Mode changes unavailable",
        ))
    }
    /// Returns an In Progress Task to Open so it can become dispatch-eligible again.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when reopening is unsupported, the transition is invalid,
    /// the Record is absent, or storage fails.
    fn reopen_record(&self, id: u64) -> Result<Record, RegistrationError> {
        let _ = id;
        Err(RegistrationError::new(
            "reopen-unavailable",
            "Task reopen persistence is unavailable",
        ))
    }
    /// Advances a Record from In Progress to Done.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for an invalid transition, absent Record, or storage failure.
    fn done_record(
        &self,
        id: u64,
        completion_resolution: Option<&str>,
    ) -> Result<Record, RegistrationError>;
    /// Adds a typed directional Relationship from `from_id` to `to_id`.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for invalid Relationship changes.
    fn relate_records(
        &self,
        from_id: u64,
        to_id: u64,
        kind: RecordRelationshipKind,
    ) -> Result<Record, RegistrationError>;
    /// Removes a typed directional Relationship from `from_id` to `to_id`.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the requested Relationship is absent.
    fn unrelate_records(
        &self,
        from_id: u64,
        to_id: u64,
        kind: RecordRelationshipKind,
    ) -> Result<Record, RegistrationError>;
    /// Lists the Record's updates newest first, using identity to break timestamp ties.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error if the Record is absent or storage fails.
    fn list_record_updates(&self, id: u64) -> Result<Vec<RecordUpdate>, RegistrationError>;
    /// Adds an immutable Comment, including to a Done Record.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the Record is absent or storage fails.
    fn add_record_comment(
        &self,
        id: u64,
        body: &str,
        author: Option<&str>,
    ) -> Result<RecordComment, RegistrationError>;
    /// Lists current (not deleted) Comments newest first.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the Record is absent or storage fails.
    fn list_record_comments(&self, id: u64) -> Result<Vec<RecordComment>, RegistrationError>;

    /// Returns the greatest existing Comment identifier for a stable page traversal.
    /// # Errors
    /// Returns an error when bounded collection reads are unavailable.
    fn record_comment_head(&self, _id: u64) -> Result<u64, RegistrationError> {
        Err(RegistrationError::new(
            "assignment-pages-unavailable",
            "bounded collection reads unavailable",
        ))
    }
    /// Reads at most `limit` Comments with identifiers in `(after, through]`, ascending.
    /// # Errors
    /// Returns an error when bounded collection reads are unavailable.
    fn record_comment_page(
        &self,
        _id: u64,
        _after: u64,
        _through: u64,
        _limit: u32,
    ) -> Result<Vec<RecordComment>, RegistrationError> {
        Err(RegistrationError::new(
            "assignment-pages-unavailable",
            "bounded collection reads unavailable",
        ))
    }
    /// Reads at most `limit` Record Updates in `(after, through]`, ascending.
    /// # Errors
    /// Returns an error when bounded collection reads are unavailable.
    fn record_update_page(
        &self,
        _id: u64,
        _after: u64,
        _through: u64,
        _limit: u32,
    ) -> Result<Vec<RecordUpdate>, RegistrationError> {
        Err(RegistrationError::new(
            "assignment-pages-unavailable",
            "bounded collection reads unavailable",
        ))
    }
    /// Deletes a Comment without retaining its content.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the Comment is absent or storage fails.
    fn delete_record_comment(
        &self,
        id: u64,
        comment_id: RecordCommentId,
    ) -> Result<(), RegistrationError>;
}

/// An immutable provider-neutral Agent provisioning policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentProfile {
    pub record_type: RecordType,
    pub revision: u32,
    pub provider: String,
    pub agent_kind: String,
    pub model: String,
    pub prompt_template: String,
    pub retry_limit: u32,
    pub retry_backoff_millis: u64,
    pub startup_timeout_millis: u64,
    pub acknowledgement_timeout_millis: u64,
    /// Whether this revision has an explicit acknowledgement timeout override.
    pub acknowledgement_timeout_overridden: bool,
    pub unknown_state_grace_millis: u64,
    /// Frozen registered policy. None denotes a preserved legacy revision.
    pub launch_policy: Option<agent_profiles::ResolvedLaunchPolicy>,
}

/// A durable attempt to dispatch one Task or interactive Grilling to an external Agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentAssignment {
    pub id: String,
    pub record_id: u64,
    pub project_slug: String,
    pub profile: AgentProfile,
    pub attempt_number: u32,
    pub claimed_at_millis: u64,
    pub state: AgentAssignmentState,
    /// Timestamp of the most recent normalized state observation.
    pub state_changed_at_millis: u64,
    /// Durable retry gate for adapter and cleanup failures.
    pub next_retry_at_millis: u64,
    /// Number of consecutively scheduled reconciliation retries.
    pub retry_count: u32,
    pub closed_at_millis: Option<u64>,
    pub closure_reason: Option<AgentAssignmentClosureReason>,
}

/// A consistent persisted snapshot for operator investigation. Never includes Task contents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentAssignmentInfo {
    pub assignment: AgentAssignment,
    pub task_status: RecordStatus,
    pub launch_intent: Option<AgentLaunchIntent>,
    pub acknowledged_at_millis: Option<u64>,
    pub dispatch_hold: Option<DispatchHold>,
    pub resources: Vec<AgentAssignmentResource>,
    pub unknown_resource_resolution: Option<UnknownResourceResolution>,
}

/// Immutable operator assertion, not provider proof of process exit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnknownResourceResolution {
    pub confirmed_at_millis: u64,
    pub authority: String,
    pub confirmation: String,
}

impl AgentAssignmentInfo {
    /// Matches dispatch capacity accounting, including retained resources on closed attempts.
    #[must_use]
    pub fn capacity_reserved(&self) -> bool {
        self.assignment.closed_at_millis.is_none()
            || self
                .resources
                .iter()
                .any(|resource| resource.retired_at_millis.is_none())
    }

    /// Describes retained recovery evidence, independently of the operator-controlled hold.
    #[must_use]
    pub fn recovery_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if self
            .resources
            .iter()
            .any(|resource| resource.retired_at_millis.is_none())
        {
            blockers.push("unretired-resources".into());
        }
        if self.resources.iter().any(|resource| {
            resource.resource_kind == "provision-intent" && resource.retired_at_millis.is_none()
        }) {
            blockers.push("uncertain-provisioning".into());
        }
        if self.assignment.closed_at_millis.is_none()
            && matches!(
                self.assignment.state,
                AgentAssignmentState::Provisioning | AgentAssignmentState::Unknown
            )
        {
            blockers.push("unresolved-open-assignment".into());
        }
        blockers
    }
}

/// Durable evidence of a single potentially uncertain agent launch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentLaunchIntent {
    pub started_at_millis: u64,
    pub acknowledgement_deadline_millis: u64,
}

/// The durable lifecycle of an Agent Assignment while it retains Project capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentAssignmentState {
    Provisioning,
    Working,
    Idle,
    Blocked,
    Unknown,
}

/// Why a closed Agent Assignment no longer retains Project capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentAssignmentClosureReason {
    Lost,
    Crashed,
    Retired,
    Replaced,
}

/// Provider-neutral outcome from an adapter call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentAdapterError {
    Endpoint(String),
    Transient(String),
    NotFound(String),
    Conflict(String),
    InvalidRequest(String),
    Cleanup(String),
}

// Adapter messages may contain provider-controlled data or credentials. Only emit explicitly
// reviewed static messages; never write arbitrary adapter payloads to daemon logs.
fn log_adapter_failure(assignment: &AgentAssignment, operation: &str, error: &AgentAdapterError) {
    let (category, message) = match error {
        AgentAdapterError::Endpoint(message) => ("endpoint", message),
        AgentAdapterError::Transient(message) => ("transient", message),
        AgentAdapterError::NotFound(message) => ("not-found", message),
        AgentAdapterError::Conflict(message) => ("conflict", message),
        AgentAdapterError::InvalidRequest(message) => ("invalid-request", message),
        AgentAdapterError::Cleanup(message) => ("cleanup", message),
    };
    tracing::warn!(record_id = assignment.record_id, assignment_id = %assignment.id, operation, category,
        detail = %safe_adapter_detail(message), "agent dispatch adapter failed");
}

fn safe_adapter_detail(message: &str) -> String {
    if let Some((method, code)) = message
        .strip_prefix("Herdr ")
        .and_then(|s| s.split_once(": "))
        && matches!(
            method,
            "ping"
                | "state.get"
                | "workspace.create"
                | "workspace.get"
                | "tab.create"
                | "tab.get"
                | "pane.split"
                | "pane.get"
                | "agent.start"
                | "agent.prompt"
                | "agent.get"
                | "agent.list"
                | "agent.stop"
                | "tab.close"
                | "pane.close"
        )
        && matches!(
            code,
            "not_found"
                | "not-found"
                | "conflict"
                | "already_exists"
                | "already-exists"
                | "unavailable"
                | "timeout"
                | "temporarily_unavailable"
                | "unknown"
        )
    {
        return format!("Herdr {method}: {code}");
    }
    match message {
        "Assignment has no Agent Workspace" => "Assignment has no Agent Workspace",
        "assignment ID cannot safely name a Herdr resource" => {
            "assignment ID cannot safely name a Herdr resource"
        }
        "assignment ID is too long for a Herdr agent name" => {
            "assignment ID is too long for a Herdr agent name"
        }
        "Herdr Unix socket endpoint is not configured" => {
            "Herdr Unix socket endpoint is not configured"
        }
        "could not connect to Herdr Unix socket" => "could not connect to Herdr Unix socket",
        "could not issue Assignment Capability" => "could not issue Assignment Capability",
        "could not resolve Forge data root" => "could not resolve Forge data root",
        "malformed Herdr response" => "malformed Herdr response",
        "Herdr response has no result" => "Herdr response has no result",
        "Herdr resource does not match the persisted resource tuple" => {
            "Herdr resource does not match the persisted resource tuple"
        }
        _ => "details withheld; adapter message is not allowlisted",
    }
    .to_owned()
}

/// Provider-neutral observation of externally managed work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentObservation {
    Working,
    Idle,
    Blocked,
    Done,
    Unknown,
    Absent,
}

/// Provider-neutral facts supplied to an adapter for one lifecycle operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentDispatchContext {
    pub assignment: AgentAssignment,
    pub project_root: PathBuf,
    /// The prepared active Managed Worktree in which this Task's Pi process runs.
    pub agent_workspace: Option<PathBuf>,
    pub dispatch_workspace: Option<DispatchWorkspace>,
    pub rendered_initial_prompt: String,
    /// A short, verified bootstrap, distinct from the full rendered instructions.
    pub bootstrap_prompt: Option<String>,
    /// Capability delivery is intentionally opaque to the provider. A later capability issuer
    /// supplies this value without exposing its secret in assignment persistence.
    pub capability_delivery: Option<AssignmentCapabilityDelivery>,
    pub resources: Vec<AgentAssignmentResource>,
}

/// Opaque launch input through which an Assignment Capability is delivered to an Agent.
#[derive(Clone, Eq, PartialEq)]
pub struct AssignmentCapabilityDelivery {
    pub endpoint: String,
    pub secret_file: PathBuf,
    pub cli_path: PathBuf,
}

impl std::fmt::Debug for AssignmentCapabilityDelivery {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AssignmentCapabilityDelivery")
            .field("endpoint", &self.endpoint)
            .field("secret_file", &self.secret_file)
            .finish_non_exhaustive()
    }
}

/// Port implemented by a concrete agent provider; it deliberately exposes no provider semantics.
#[allow(clippy::missing_errors_doc)]
pub trait AgentDispatchAdapter: Send + Sync {
    fn probe(&self) -> Result<(), AgentAdapterError>;
    fn verify_workspace(&self, _id: &str) -> Result<DispatchWorkspace, AgentAdapterError> {
        Err(AgentAdapterError::InvalidRequest(
            "workspace selection is unsupported".into(),
        ))
    }
    fn provision(
        &self,
        context: &AgentDispatchContext,
        resources: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError>;
    fn observe(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentObservation, AgentAdapterError>;
    fn recover(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentObservation, AgentAdapterError>;
    fn retire(
        &self,
        context: &AgentDispatchContext,
        resources: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError>;
}

/// Selects the concrete provider adapter from immutable profile provenance.
#[allow(clippy::missing_errors_doc)]
pub trait AgentDispatchAdapterFactory: Send + Sync {
    fn validate_profile(&self, _profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        Ok(())
    }
    /// Validate mutable direct inputs for a new dispatch, not existing Assignment recovery.
    fn validate_preflight(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        self.validate_profile(profile)
    }
    fn adapter_for(
        &self,
        profile: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError>;
}

/// The persisted result of reconciling one already-open assignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssignmentReconciliation {
    AwaitingAcknowledgement,
    StateUpdated(AgentAssignment),
    Closed(AgentAssignment),
    Stale,
    Retained(AdapterRetention),
}

/// Why reconciliation retained an Assignment without mutating its durable state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterRetention {
    Endpoint,
    Transient,
    Conflict,
    InvalidRequest,
    Cleanup,
    NotFound,
}

/// Reconciles one Assignment without holding persistence while the adapter runs.
///
/// A stale observer is harmless: its compare-and-set fails after another reconciliation
/// changes or closes the attempt. Provider terminal observations never complete the Task.
#[allow(clippy::missing_errors_doc)]
pub fn reconcile_agent_assignment<A: AgentDispatchAdapter + ?Sized>(
    repository: &dyn AgentDispatchRepository,
    adapter: &A,
    assignment: &AgentAssignment,
) -> Result<AssignmentReconciliation, RegistrationError> {
    reconcile_assignment(repository, adapter, assignment, false)
}

fn reconcile_assignment<A: AgentDispatchAdapter + ?Sized>(
    repository: &dyn AgentDispatchRepository,
    adapter: &A,
    assignment: &AgentAssignment,
    controlled_acceptance: bool,
) -> Result<AssignmentReconciliation, RegistrationError> {
    if !controlled_acceptance
        && assignment.profile.record_type == RecordType::Task
        && assignment.state == AgentAssignmentState::Provisioning
    {
        return Err(task_dispatch_unsupported());
    }
    let context = agent_dispatch_context(repository, assignment)?;
    let observed = match assignment.state {
        AgentAssignmentState::Provisioning => adapter
            .provision(&context, repository)
            .map(|()| AgentObservation::Working),
        AgentAssignmentState::Working
        | AgentAssignmentState::Idle
        | AgentAssignmentState::Blocked
        | AgentAssignmentState::Unknown => adapter.observe(&context),
    };
    // Both Tasks and Grillings require authenticated bootstrap. Failed launches with
    // retained intent still require conservative recovery for either kind.
    if assignment.state == AgentAssignmentState::Provisioning
        && (observed.is_ok() || repository.agent_launch_intent(&assignment.id)?.is_some())
    {
        return if repository.agent_assignment(&assignment.id)?.state
            == AgentAssignmentState::Provisioning
        {
            Ok(AssignmentReconciliation::AwaitingAcknowledgement)
        } else {
            Ok(AssignmentReconciliation::Stale)
        };
    }
    let observation = match observed {
        Ok(observation) => observation,
        Err(error) => {
            let operation = if assignment.state == AgentAssignmentState::Provisioning {
                "provision"
            } else {
                "observe"
            };
            log_adapter_failure(assignment, operation, &error);
            return Ok(AssignmentReconciliation::Retained(match error {
                AgentAdapterError::Endpoint(_) => AdapterRetention::Endpoint,
                AgentAdapterError::Transient(_) => AdapterRetention::Transient,
                AgentAdapterError::NotFound(_) => AdapterRetention::NotFound,
                AgentAdapterError::Conflict(_) => AdapterRetention::Conflict,
                AgentAdapterError::InvalidRequest(_) => AdapterRetention::InvalidRequest,
                AgentAdapterError::Cleanup(_) => AdapterRetention::Cleanup,
            }));
        }
    };
    match observation {
        AgentObservation::Working => {
            update_assignment_state(repository, assignment, AgentAssignmentState::Working)
        }
        AgentObservation::Idle => {
            update_assignment_state(repository, assignment, AgentAssignmentState::Idle)
        }
        AgentObservation::Blocked => {
            update_assignment_state(repository, assignment, AgentAssignmentState::Blocked)
        }
        AgentObservation::Unknown => {
            update_assignment_state(repository, assignment, AgentAssignmentState::Unknown)
        }
        AgentObservation::Done | AgentObservation::Absent => {
            if let Err(error) = adapter.retire(&context, repository) {
                log_adapter_failure(assignment, "retire", &error);
                return Ok(AssignmentReconciliation::Retained(match error {
                    AgentAdapterError::Endpoint(_) => AdapterRetention::Endpoint,
                    AgentAdapterError::Transient(_) => AdapterRetention::Transient,
                    AgentAdapterError::NotFound(_) => AdapterRetention::NotFound,
                    AgentAdapterError::Conflict(_) => AdapterRetention::Conflict,
                    AgentAdapterError::InvalidRequest(_) => AdapterRetention::InvalidRequest,
                    AgentAdapterError::Cleanup(_) => AdapterRetention::Cleanup,
                }));
            }
            repository
                .compare_and_close_agent_assignment(
                    &assignment.id,
                    assignment.state,
                    AgentAssignmentClosureReason::Lost,
                )
                .map(|assignment| {
                    assignment.map_or(
                        AssignmentReconciliation::Stale,
                        AssignmentReconciliation::Closed,
                    )
                })
        }
    }
}

fn task_dispatch_unsupported() -> RegistrationError {
    RegistrationError::new(
        "task-dispatch-unsupported-contract",
        "Task dispatch is disabled: the Check execution contract is not yet supported; no agent was launched. Manual execution is never selected implicitly.",
    )
}

fn update_assignment_state<R: AgentDispatchRepository + ?Sized>(
    repository: &R,
    assignment: &AgentAssignment,
    next: AgentAssignmentState,
) -> Result<AssignmentReconciliation, RegistrationError> {
    repository
        .compare_and_set_agent_assignment_state(&assignment.id, assignment.state, next)
        .map(|assignment| {
            assignment.map_or(
                AssignmentReconciliation::Stale,
                AssignmentReconciliation::StateUpdated,
            )
        })
}

/// Daemon-owned policy for one bounded reconciliation scan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentDispatchReconciliationPolicy {
    pub scan_lease_millis: u64,
}

/// Result of a daemon-owned one-shot reconciliation scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentDispatchScan {
    pub acquired_lease: bool,
    pub reconciled: Vec<AssignmentReconciliation>,
}

/// Reconciles all open Assignments without holding storage over adapter calls.
///
/// The caller supplies time so grace and retry behavior is deterministic in tests and durable
/// across daemon restarts. A provisioning failure holds the Task and retains its Assignment.
#[allow(clippy::missing_errors_doc)]
pub fn reconcile_agent_dispatch<F: AgentDispatchAdapterFactory + ?Sized>(
    repository: &dyn AgentDispatchRepository,
    factory: &F,
    daemon_id: &str,
    now_millis: u64,
    policy: AgentDispatchReconciliationPolicy,
) -> Result<AgentDispatchScan, RegistrationError> {
    if !repository.try_acquire_agent_dispatch_scan_lease(
        daemon_id,
        now_millis,
        policy.scan_lease_millis,
    )? {
        return Ok(AgentDispatchScan {
            acquired_lease: false,
            reconciled: vec![],
        });
    }
    let mut reconciled = Vec::new();
    for assignment in repository.list_open_agent_assignments()? {
        // Do not even run provisioning guards: they can mutate retry/hold state.
        // Existing live ownership remains observable and explicitly retireable.
        if assignment.profile.record_type == RecordType::Task
            && assignment.state == AgentAssignmentState::Provisioning
        {
            tracing::warn!(assignment_id = %assignment.id,
                code = "task-dispatch-unsupported-contract",
                "Task dispatch is disabled pending the Check execution contract");
            continue;
        }
        if assignment.state == AgentAssignmentState::Provisioning
            && !repository.guard_agent_provisioning(&assignment.id, now_millis)?
        {
            continue;
        }
        if let Some(_intent) = repository.agent_launch_intent(&assignment.id)? {
            // Acknowledgement can win while agent.start is still in flight. Until the
            // response has retained the external identity, an absent observation is not
            // evidence of agent death (and a lost response may never retain it).
            if assignment.state == AgentAssignmentState::Working
                && !repository
                    .list_agent_assignment_resources(&assignment.id)?
                    .iter()
                    .any(|resource| {
                        resource.resource_kind == "agent-name"
                            && resource.retired_at_millis.is_none()
                    })
            {
                continue;
            }
        }
        if assignment.next_retry_at_millis > now_millis {
            continue;
        }
        if assignment.state == AgentAssignmentState::Unknown
            && now_millis.saturating_sub(assignment.state_changed_at_millis)
                < assignment.profile.unknown_state_grace_millis
        {
            continue;
        }
        if assignment.state == AgentAssignmentState::Provisioning
            && !workspace_dispatch::provisioning_available(repository, factory, &assignment)
        {
            continue;
        }
        let adapter = match factory.adapter_for(&assignment.profile) {
            Ok(adapter) => adapter,
            Err(error) => {
                log_adapter_failure(&assignment, "select-adapter", &error);
                let retry_count = assignment.retry_count.saturating_add(1);
                let next_retry = now_millis.saturating_add(assignment.profile.retry_backoff_millis);
                record_dispatch_failure(repository, &assignment, next_retry, retry_count)?;
                reconciled.push(AssignmentReconciliation::Retained(retention_from_error(
                    &error,
                )));
                continue;
            }
        };
        let outcome = if assignment.state == AgentAssignmentState::Unknown {
            reconcile_recovery(repository, adapter.as_ref(), &assignment)?
        } else {
            reconcile_agent_assignment(repository, adapter.as_ref(), &assignment)?
        };
        if matches!(outcome, AssignmentReconciliation::Retained(_)) {
            let retry_count = assignment.retry_count.saturating_add(1);
            let next_retry = now_millis.saturating_add(assignment.profile.retry_backoff_millis);
            record_dispatch_failure(repository, &assignment, next_retry, retry_count)?;
        }
        reconciled.push(outcome);
    }
    Ok(AgentDispatchScan {
        acquired_lease: true,
        reconciled,
    })
}

fn record_dispatch_failure(
    repository: &dyn AgentDispatchRepository,
    assignment: &AgentAssignment,
    next_retry_at_millis: u64,
    retry_count: u32,
) -> Result<(), RegistrationError> {
    if !repository.schedule_agent_assignment_retry(
        &assignment.id,
        assignment.state,
        next_retry_at_millis,
        retry_count,
    )? || assignment.state != AgentAssignmentState::Provisioning
    {
        return Ok(());
    }
    if !repository.hold_uncertain_agent_assignment(&assignment.id)? {
        return Ok(());
    }
    repository.add_record_comment(
        assignment.record_id,
        "Automatic dispatch stopped after an uncertain provisioning attempt; the Assignment remains open because an Agent may still be live. A Dispatch Hold requires operator investigation.",
        Some("agent-dispatch"),
    )
    .map(|_| ())
}

fn reconcile_recovery<A: AgentDispatchAdapter + ?Sized>(
    repository: &dyn AgentDispatchRepository,
    adapter: &A,
    assignment: &AgentAssignment,
) -> Result<AssignmentReconciliation, RegistrationError> {
    let context = agent_dispatch_context(repository, assignment)?;
    match adapter.recover(&context) {
        Ok(AgentObservation::Working) => {
            update_assignment_state(repository, assignment, AgentAssignmentState::Working)
        }
        Ok(AgentObservation::Idle) => {
            update_assignment_state(repository, assignment, AgentAssignmentState::Idle)
        }
        Ok(AgentObservation::Blocked) => {
            update_assignment_state(repository, assignment, AgentAssignmentState::Blocked)
        }
        Ok(AgentObservation::Unknown | AgentObservation::Done | AgentObservation::Absent) => {
            if let Err(error) = adapter.retire(&context, repository) {
                log_adapter_failure(assignment, "recover-retire", &error);
                return Ok(AssignmentReconciliation::Retained(retention_from_error(
                    &error,
                )));
            }
            repository
                .compare_and_close_agent_assignment(
                    &assignment.id,
                    assignment.state,
                    AgentAssignmentClosureReason::Crashed,
                )
                .map(|closed| {
                    closed.map_or(
                        AssignmentReconciliation::Stale,
                        AssignmentReconciliation::Closed,
                    )
                })
        }
        Err(error) => {
            log_adapter_failure(assignment, "recover", &error);
            Ok(AssignmentReconciliation::Retained(retention_from_error(
                &error,
            )))
        }
    }
}

fn agent_dispatch_context(
    repository: &dyn AgentDispatchRepository,
    assignment: &AgentAssignment,
) -> Result<AgentDispatchContext, RegistrationError> {
    use std::fmt::Write;
    let project = repository.project(&assignment.project_slug)?;
    let record = repository.show_record(assignment.record_id)?;
    let agent_workspace = repository.assignment_execution_directory(&assignment.id)?;
    let execution_instruction = if record.record_type == RecordType::Grilling {
        let mut instruction = agent_workspace.as_ref().map_or_else(String::new, |path| {
            format!("\n\nYour Forge Workspace root is {}.", path.display())
        });
        instruction.push_str("\nApplicable Project scope paths:");
        for reference in &record.project_slugs {
            let scoped_project = repository.project(reference)?;
            write!(
                instruction,
                "\n- {}: {}",
                reference,
                scoped_project.project_root.display()
            )
            .expect("writing to a String cannot fail");
        }
        instruction
    } else {
        agent_workspace.as_ref().map_or_else(String::new, |path| {
            format!(
                "\n\nYour assigned Managed Worktree is {}. Perform Task work only in this worktree.",
                path.display()
            )
        })
    };
    Ok(AgentDispatchContext {
        assignment: assignment.clone(),
        project_root: project.project_root,
        agent_workspace,
        dispatch_workspace: repository.assignment_dispatch_workspace(&assignment.id)?,
        bootstrap_prompt: None,
        rendered_initial_prompt: format!(
            "{}\n\n{}{}",
            assignment.profile.prompt_template, record.body, execution_instruction
        ),
        capability_delivery: None,
        resources: repository.list_agent_assignment_resources(&assignment.id)?,
    })
}

fn retention_from_error(error: &AgentAdapterError) -> AdapterRetention {
    match error {
        AgentAdapterError::Endpoint(_) => AdapterRetention::Endpoint,
        AgentAdapterError::Transient(_) => AdapterRetention::Transient,
        AgentAdapterError::NotFound(_) => AdapterRetention::NotFound,
        AgentAdapterError::Conflict(_) => AdapterRetention::Conflict,
        AgentAdapterError::InvalidRequest(_) => AdapterRetention::InvalidRequest,
        AgentAdapterError::Cleanup(_) => AdapterRetention::Cleanup,
    }
}

/// The caller-supplied immutable identity and provenance for an Agent Assignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewAgentAssignment {
    pub id: String,
    pub record_id: u64,
    pub project_slug: String,
    pub profile_revision: u32,
}

/// A retained provider-neutral external resource fact for an Assignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentAssignmentResource {
    pub assignment_id: String,
    pub resource_kind: String,
    pub resource_id: String,
    pub created_at_millis: u64,
    pub retired_at_millis: Option<u64>,
}

/// A new retained provider-neutral external resource fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewAgentAssignmentResource {
    pub assignment_id: String,
    pub resource_kind: String,
    pub resource_id: String,
}

/// An immutable, redacted authorization decision for an Assignment Capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignmentAuthorizationAudit {
    pub assignment_id: String,
    pub capability_id: String,
    pub created_at_millis: u64,
    pub operation: AssignmentAuthorizationOperation,
    pub target_record_id: u64,
    pub outcome: AssignmentAuthorizationOutcome,
    pub reason: String,
    pub correlation_id: String,
}

/// The bounded operation attempted with an Assignment Capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentAuthorizationOperation {
    ReadContext,
    AddComment,
    CompleteTask,
    StartTask,
    Acknowledge,
}

/// The recorded decision; neither capability material nor Comment content is retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentAuthorizationOutcome {
    Allowed,
    Denied,
}

/// The records visible through a valid Assignment Capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignmentContext {
    pub assignment_id: String,
    pub task: Record,
    pub directly_related_records: Vec<Record>,
}

/// The operator-selected provider workspace shared by dispatched Agents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchWorkspace {
    pub id: String,
}

/// A durable operator-cleared block on Agent dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchHold {
    pub record_id: u64,
    pub reason: String,
}

/// Configuration identity whose profiles and provider binding were preflighted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchPreflightSelection {
    pub task_revision: u32,
    pub grilling_revision: u32,
    pub provider_generation: u64,
    pub binding_id: String,
}

/// Durable persistence boundary for provider-neutral dispatch foundations.
#[allow(clippy::missing_errors_doc)]
pub trait AgentDispatchRepository: RecordRepository + WorkspaceDispatchRepository {
    /// Returns the greatest committed global Record Update identifier for read freshness.
    fn record_update_head(&self) -> Result<RecordUpdateId, RegistrationError>;
    /// Returns the operator-cleared automatic-dispatch block, if any.
    fn dispatch_hold(&self, record_id: u64) -> Result<Option<DispatchHold>, RegistrationError>;
    /// Creates or replaces the durable block on automatic dispatch.
    fn hold_dispatch(
        &self,
        record_id: u64,
        reason: &str,
    ) -> Result<DispatchHold, RegistrationError>;
    /// Clears a durable automatic-dispatch block.
    fn resume_dispatch(&self, record_id: u64) -> Result<(), RegistrationError>;
    /// Returns an immutable seeded Agent Profile revision.
    fn agent_profile(
        &self,
        record_type: RecordType,
        revision: u32,
    ) -> Result<AgentProfile, RegistrationError>;
    /// Returns the active immutable Task profile revision.
    fn active_task_agent_profile(&self) -> Result<AgentProfile, RegistrationError>;
    /// Creates an immutable Task profile revision; None restores the default timeout.
    fn configure_task_acknowledgement_timeout(
        &self,
        timeout_millis: Option<u64>,
    ) -> Result<AgentProfile, RegistrationError>;
    /// Creates an Assignment with immutable profile provenance.
    fn create_agent_assignment(
        &self,
        assignment: &NewAgentAssignment,
    ) -> Result<AgentAssignment, RegistrationError>;
    /// Atomically claims an explicitly selected Task or Grilling, or returns its open Assignment.
    /// The boolean reports an existing Assignment; all eligibility and recovery checks are atomic.
    fn claim_record_agent_assignment(
        &self,
        record_id: u64,
        assignment_id: &str,
    ) -> Result<(AgentAssignment, bool), RegistrationError> {
        self.claim_record_agent_assignment_with_selection(record_id, assignment_id, None)
    }
    fn claim_record_agent_assignment_checked(
        &self,
        record_id: u64,
        assignment_id: &str,
        selection: &DispatchPreflightSelection,
    ) -> Result<(AgentAssignment, bool), RegistrationError> {
        self.claim_record_agent_assignment_with_selection(record_id, assignment_id, Some(selection))
    }
    fn claim_record_agent_assignment_with_selection(
        &self,
        record_id: u64,
        assignment_id: &str,
        selection: Option<&DispatchPreflightSelection>,
    ) -> Result<(AgentAssignment, bool), RegistrationError>;
    /// Controlled acceptance only: atomically bind the next Check, pinned profile and input.
    /// Normal dispatch must not call this entry point while the rollout gate is closed.
    #[allow(clippy::too_many_arguments)]
    fn claim_task_check_acceptance(
        &self,
        _record: u64,
        _assignment: &str,
        _selection: &DispatchPreflightSelection,
        _evidence: &outcomes::Evidence,
        _check: u64,
        _profile_revision: u32,
    ) -> Result<(AgentAssignment, bool), RegistrationError> {
        Err(task_dispatch_unsupported())
    }
    /// Immutable Task-specific binding; absent on Grilling and legacy test Assignments.
    fn assignment_check_attempt(
        &self,
        _assignment: &str,
    ) -> Result<Option<outcomes::Attempt>, RegistrationError> {
        Ok(None)
    }
    /// Persists successful trusted-context bootstrap independently of source files.
    fn mark_assignment_context_read(&self, _assignment_id: &str) -> Result<(), RegistrationError> {
        Err(RegistrationError::new(
            "assignment-context-unavailable",
            "context bootstrap persistence unavailable",
        ))
    }
    /// Returns whether trusted context was fetched after acknowledged start.
    fn assignment_context_read(&self, _assignment_id: &str) -> Result<bool, RegistrationError> {
        Ok(false)
    }
    /// Returns one Assignment by opaque identifier.
    fn agent_assignment(&self, assignment_id: &str) -> Result<AgentAssignment, RegistrationError>;
    /// Returns a consistent snapshot of persisted investigation evidence, including closed attempts.
    fn agent_assignment_info(
        &self,
        assignment_id: &str,
    ) -> Result<AgentAssignmentInfo, RegistrationError>;
    /// Returns retained Assignment history for a Record, newest attempt first.
    fn list_agent_assignments(
        &self,
        record_id: u64,
    ) -> Result<Vec<AgentAssignment>, RegistrationError>;
    /// Lists all open Assignments for one daemon reconciliation scan.
    fn list_open_agent_assignments(&self) -> Result<Vec<AgentAssignment>, RegistrationError>;
    /// Acquires the short-lived daemon scan lease, returning false when another scan owns it.
    fn try_acquire_agent_dispatch_scan_lease(
        &self,
        owner: &str,
        now_millis: u64,
        duration_millis: u64,
    ) -> Result<bool, RegistrationError>;
    /// Atomically claims one eligible open Task for `project_slug`, respecting its capacity.
    /// Returns `None` when no capacity or candidate is available.
    fn claim_next_agent_assignment(
        &self,
        project_slug: &str,
        assignment_id: &str,
    ) -> Result<Option<AgentAssignment>, RegistrationError> {
        self.claim_next_agent_assignment_with_selection(project_slug, assignment_id, None)
    }
    fn claim_next_agent_assignment_checked(
        &self,
        project_slug: &str,
        assignment_id: &str,
        selection: &DispatchPreflightSelection,
    ) -> Result<Option<AgentAssignment>, RegistrationError> {
        self.claim_next_agent_assignment_with_selection(
            project_slug,
            assignment_id,
            Some(selection),
        )
    }
    fn claim_next_agent_assignment_with_selection(
        &self,
        project_slug: &str,
        assignment_id: &str,
        selection: Option<&DispatchPreflightSelection>,
    ) -> Result<Option<AgentAssignment>, RegistrationError>;
    /// Conditionally persists a normalized observation without letting a stale observer win.
    fn compare_and_set_agent_assignment_state(
        &self,
        assignment_id: &str,
        expected: AgentAssignmentState,
        next: AgentAssignmentState,
    ) -> Result<Option<AgentAssignment>, RegistrationError>;
    /// Atomically acknowledges an open provisioning Assignment, including retries.
    fn acknowledge_agent_assignment(
        &self,
        assignment_id: &str,
    ) -> Result<AgentAssignment, RegistrationError>;
    /// Closes an open Assignment conditionally, releasing its capacity for a later attempt.
    fn close_agent_assignment(
        &self,
        assignment_id: &str,
        reason: AgentAssignmentClosureReason,
    ) -> Result<AgentAssignment, RegistrationError>;
    /// Closes only the Assignment snapshot observed by this reconciler.
    fn compare_and_close_agent_assignment(
        &self,
        assignment_id: &str,
        expected: AgentAssignmentState,
        reason: AgentAssignmentClosureReason,
    ) -> Result<Option<AgentAssignment>, RegistrationError>;
    /// Durably backs off a failed adapter operation, rejecting a stale observer.
    fn schedule_agent_assignment_retry(
        &self,
        assignment_id: &str,
        expected: AgentAssignmentState,
        next_retry_at_millis: u64,
        retry_count: u32,
    ) -> Result<bool, RegistrationError>;
    /// Persists launch intent and deadline once, immediately before the launch request.
    fn begin_agent_launch(
        &self,
        assignment_id: &str,
        now_millis: u64,
    ) -> Result<bool, RegistrationError>;
    /// Returns durable launch intent, if one was recorded.
    fn agent_launch_intent(
        &self,
        assignment_id: &str,
    ) -> Result<Option<AgentLaunchIntent>, RegistrationError>;
    /// Hold an open provisioning Assignment without racing an acknowledgement.
    fn hold_uncertain_agent_assignment(
        &self,
        assignment_id: &str,
    ) -> Result<bool, RegistrationError>;
    /// Atomically holds uncertain provisioning attempts and returns whether a fresh attempt may run.
    fn guard_agent_provisioning(
        &self,
        assignment_id: &str,
        now_millis: u64,
    ) -> Result<bool, RegistrationError>;
    /// Atomically records intent before any potentially side-effecting provisioning call.
    fn begin_agent_provision(&self, _assignment_id: &str) -> Result<bool, RegistrationError> {
        Ok(false)
    }
    /// Retains a provider-neutral external resource fact.
    fn add_agent_assignment_resource(
        &self,
        resource: &NewAgentAssignmentResource,
    ) -> Result<AgentAssignmentResource, RegistrationError>;
    /// Marks a retained resource as retired without deleting its recovery history.
    fn retire_agent_assignment_resource(
        &self,
        assignment_id: &str,
        resource_kind: &str,
        resource_id: &str,
    ) -> Result<AgentAssignmentResource, RegistrationError>;
    /// Atomically audits an explicit operator assertion and resolves provisioning uncertainty.
    /// Requires a closed (capability-revoked) Assignment; never retires known resources.
    fn resolve_unknown_agent_resources(&self, assignment_id: &str)
    -> Result<(), RegistrationError>;
    /// Returns retained resource history for an Assignment.
    fn list_agent_assignment_resources(
        &self,
        assignment_id: &str,
    ) -> Result<Vec<AgentAssignmentResource>, RegistrationError>;
    /// Retains and returns the Assignment's execution identity once its directory is available.
    fn assignment_execution_directory(
        &self,
        assignment_id: &str,
    ) -> Result<Option<PathBuf>, RegistrationError> {
        self.agent_workspace(self.agent_assignment(assignment_id)?.record_id)
    }
    /// Returns the Task's active Managed Worktree or the Grilling's owning Workspace root.
    fn agent_workspace(&self, record_id: u64) -> Result<Option<PathBuf>, RegistrationError>;
    /// Returns the immutable provider container bound when this attempt was created.
    fn assignment_dispatch_workspace(
        &self,
        assignment_id: &str,
    ) -> Result<Option<DispatchWorkspace>, RegistrationError>;
    /// Legacy development configuration; not used by dispatch or operator routing.
    fn dispatch_workspace(&self) -> Result<Option<DispatchWorkspace>, RegistrationError>;
    /// Persists the shared workspace identity immediately after it is created.
    fn retain_dispatch_workspace(
        &self,
        workspace: &DispatchWorkspace,
    ) -> Result<DispatchWorkspace, RegistrationError>;
    /// Selects a verified workspace only when changing it cannot strand assignment resources.
    fn select_dispatch_workspace(
        &self,
        workspace: &DispatchWorkspace,
    ) -> Result<DispatchWorkspace, RegistrationError> {
        self.retain_dispatch_workspace(workspace)
    }
    /// Clears the selected workspace only when no unresolved resources depend on it.
    fn unset_dispatch_workspace(&self) -> Result<(), RegistrationError>;
    /// Sets or clears the daemon-wide absolute Herdr Unix socket path.
    fn set_herdr_socket_path(&self, path: Option<&Path>) -> Result<(), RegistrationError>;
    /// Returns the configured daemon-wide Herdr Unix socket path.
    fn herdr_socket_path(&self) -> Result<Option<PathBuf>, RegistrationError>;
    /// Sets or clears a Project Task concurrency override.
    fn set_project_task_concurrency_limit(
        &self,
        project_slug: &str,
        limit: Option<u16>,
    ) -> Result<(), RegistrationError>;
    /// Returns the configured Project limit or the V1 default of one.
    fn effective_project_task_concurrency_limit(
        &self,
        project_slug: &str,
    ) -> Result<u16, RegistrationError>;
    /// Returns the explicit Project override, if one is configured.
    fn project_task_concurrency_limit(
        &self,
        project_slug: &str,
    ) -> Result<Option<u16>, RegistrationError>;
    /// Appends a redacted immutable authorization decision.
    fn add_assignment_authorization_audit(
        &self,
        audit: &AssignmentAuthorizationAudit,
    ) -> Result<(), RegistrationError>;
    /// Lists immutable authorization decisions oldest first.
    fn list_assignment_authorization_audits(
        &self,
        assignment_id: &str,
    ) -> Result<Vec<AssignmentAuthorizationAudit>, RegistrationError>;
}

/// Authorizes a capability-authenticated context read and records its decision.
#[allow(clippy::missing_errors_doc)]
pub fn assignment_context<R: AgentDispatchRepository>(
    repository: &R,
    assignment_id: &str,
    capability_id: &str,
    correlation_id: &str,
) -> Result<AssignmentContext, RegistrationError> {
    let assignment = repository.agent_assignment(assignment_id)?;
    let task = repository.show_record(assignment.record_id)?;
    let related = task
        .relationships
        .iter()
        .filter(|relationship| {
            matches!(
                relationship.kind,
                RecordRelationshipKind::ParentChild
                    | RecordRelationshipKind::DependsOn
                    | RecordRelationshipKind::GeneratedBy
            )
        })
        .filter_map(|relationship| {
            (relationship.from_id == task.id)
                .then_some(relationship.to_id)
                .or_else(|| (relationship.to_id == task.id).then_some(relationship.from_id))
        })
        .map(|id| repository.show_record(id))
        .collect::<Result<Vec<_>, _>>()?;
    authorize_assignment_operation(
        repository,
        &assignment,
        capability_id,
        correlation_id,
        AssignmentAuthorizationOperation::ReadContext,
        task.id,
        assignment.closed_at_millis.is_none(),
    )?;
    Ok(AssignmentContext {
        assignment_id: assignment.id,
        task,
        directly_related_records: related,
    })
}

/// Authorizes a capability-authenticated Comment on its bound Task and records its decision.
#[allow(clippy::missing_errors_doc)]
pub fn assignment_add_comment<R: AgentDispatchRepository + ?Sized>(
    repository: &R,
    assignment_id: &str,
    capability_id: &str,
    correlation_id: &str,
    task_id: u64,
    body: &str,
) -> Result<RecordComment, RegistrationError> {
    let assignment = repository.agent_assignment(assignment_id)?;
    let allowed = assignment.closed_at_millis.is_none() && assignment.record_id == task_id;
    authorize_assignment_operation(
        repository,
        &assignment,
        capability_id,
        correlation_id,
        AssignmentAuthorizationOperation::AddComment,
        task_id,
        allowed,
    )?;
    if body.trim().is_empty() {
        return Err(RegistrationError::new(
            "invalid-comment",
            "comment must not be blank",
        ));
    }
    repository.add_record_comment(task_id, body, None)
}

/// Records an acknowledgement after the daemon authenticates the Assignment Capability.
#[allow(clippy::missing_errors_doc)]
pub fn assignment_acknowledge<R: AgentDispatchRepository + ?Sized>(
    repository: &R,
    assignment_id: &str,
    capability_id: &str,
    correlation_id: &str,
) -> Result<AgentAssignment, RegistrationError> {
    let assignment = repository.agent_assignment(assignment_id)?;
    authorize_assignment_operation(
        repository,
        &assignment,
        capability_id,
        correlation_id,
        AssignmentAuthorizationOperation::Acknowledge,
        assignment.record_id,
        assignment.closed_at_millis.is_none(),
    )?;
    repository.acknowledge_agent_assignment(assignment_id)
}

/// Authorizes idempotent capability-authenticated start of its bound Open Task.
#[allow(clippy::missing_errors_doc)]
pub fn assignment_start_task<R: AgentDispatchRepository + ?Sized>(
    repository: &R,
    assignment_id: &str,
    capability_id: &str,
    correlation_id: &str,
    task_id: u64,
) -> Result<Record, RegistrationError> {
    let assignment = repository.agent_assignment(assignment_id)?;
    let task = repository.show_record(task_id)?;
    let allowed = assignment.closed_at_millis.is_none()
        && assignment.record_id == task_id
        && task.record_type == RecordType::Task
        && matches!(task.status, RecordStatus::Open | RecordStatus::InProgress);
    authorize_assignment_operation(
        repository,
        &assignment,
        capability_id,
        correlation_id,
        AssignmentAuthorizationOperation::StartTask,
        task_id,
        allowed,
    )?;
    if task.status == RecordStatus::Open {
        repository.start_owned_record(task_id, assignment_id)
    } else {
        Ok(task)
    }
}

/// Authorizes capability-authenticated completion of its bound Task and records its decision.
#[allow(clippy::missing_errors_doc)]
pub fn assignment_complete_task<R: AgentDispatchRepository + ?Sized>(
    repository: &R,
    assignment_id: &str,
    capability_id: &str,
    correlation_id: &str,
    task_id: u64,
    resolution: Option<&str>,
) -> Result<Record, RegistrationError> {
    let assignment = repository.agent_assignment(assignment_id)?;
    let allowed = assignment.closed_at_millis.is_none() && assignment.record_id == task_id;
    authorize_assignment_operation(
        repository,
        &assignment,
        capability_id,
        correlation_id,
        AssignmentAuthorizationOperation::CompleteTask,
        task_id,
        allowed,
    )?;
    repository.done_record(task_id, resolution)
}

fn authorize_assignment_operation<R: AgentDispatchRepository + ?Sized>(
    repository: &R,
    assignment: &AgentAssignment,
    _capability_id: &str,
    _correlation_id: &str,
    operation: AssignmentAuthorizationOperation,
    target_record_id: u64,
    allowed: bool,
) -> Result<(), RegistrationError> {
    repository.add_assignment_authorization_audit(&AssignmentAuthorizationAudit {
        assignment_id: assignment.id.clone(),
        capability_id: assignment.id.clone(),
        created_at_millis: 0,
        operation,
        // Denials are scoped to the owner: foreign/nonexistent IDs must not become
        // storage-FK errors or a target-existence oracle in the agent interface.
        target_record_id: if allowed {
            target_record_id
        } else {
            assignment.record_id
        },
        outcome: if allowed {
            AssignmentAuthorizationOutcome::Allowed
        } else {
            AssignmentAuthorizationOutcome::Denied
        },
        reason: if allowed {
            "bound-open-assignment".into()
        } else {
            "outside-bound-open-assignment".into()
        },
        correlation_id: "assignment-operation".into(),
    })?;
    if allowed {
        Ok(())
    } else {
        Err(RegistrationError::new(
            "assignment-capability-denied",
            "capability is not authorized for this Assignment operation",
        ))
    }
}

/// The application boundary for operations shared by all transports.
pub struct Application {
    workspaces: Option<Box<dyn WorkspaceRepository>>,
    records: Box<dyn RecordRepository>,
    managed_worktrees: Option<Box<dyn ManagedWorktreeRepository>>,
    worktree_adapter: Option<Box<dyn ManagedWorktreeAdapter>>,
    dispatch: Option<Box<dyn AgentDispatchRepository>>,
    adapter_factory: Option<Box<dyn AgentDispatchAdapterFactory>>,
    inspector: std::sync::Arc<dyn RepositoryInspector>,
    // Serialize provider verification with configuration writes. Atomic repository guards
    // protect Assignments/resources without holding storage over provider calls.
    provider_configuration: std::sync::Mutex<()>,
}

/// Stable operator-facing facts about one Project's dispatch capacity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentDispatchProjectStatus {
    pub project_slug: String,
    pub capacity: u16,
    pub configured_capacity: Option<u16>,
}

/// Stable operator-facing facts about daemon dispatch configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentDispatchStatus {
    pub herdr_socket_path: Option<PathBuf>,
    pub endpoint_diagnostic: String,
    pub project: Option<AgentDispatchProjectStatus>,
}

/// An effective Dispatcher configuration value and its origin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatcherConfigEntry {
    pub ownership: String,
    pub key: String,
    pub value: Option<String>,
    pub provenance: String,
}

/// Result of requesting a single daemon-owned reconciliation pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentDispatchReconcileResult {
    pub acquired_lease: bool,
    pub reconciled_assignments: usize,
}

static DISPATCH_ASSIGNMENT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn next_dispatch_assignment_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = DISPATCH_ASSIGNMENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{millis:x}-{sequence:x}")
}

impl Application {
    /// Creates an application from its infrastructure boundaries.
    #[must_use]
    pub fn new(
        records: Box<dyn RecordRepository>,
        inspector: Box<dyn RepositoryInspector>,
    ) -> Self {
        Self {
            records,
            workspaces: None,
            managed_worktrees: None,
            worktree_adapter: None,
            dispatch: None,
            adapter_factory: None,
            provider_configuration: std::sync::Mutex::new(()),
            inspector: inspector.into(),
        }
    }

    /// Creates an application with managed Worktree persistence and server operations enabled.
    #[must_use]
    pub fn with_managed_worktrees(
        records: Box<dyn RecordRepository>,
        managed_worktrees: Box<dyn ManagedWorktreeRepository>,
        worktree_adapter: Box<dyn ManagedWorktreeAdapter>,
        inspector: Box<dyn RepositoryInspector>,
    ) -> Self {
        Self {
            records,
            workspaces: None,
            managed_worktrees: Some(managed_worktrees),
            worktree_adapter: Some(worktree_adapter),
            dispatch: None,
            adapter_factory: None,
            provider_configuration: std::sync::Mutex::new(()),
            inspector: inspector.into(),
        }
    }

    /// Creates an application with the durable dispatch boundary enabled.
    #[must_use]
    pub fn with_agent_dispatch(
        records: Box<dyn RecordRepository>,
        dispatch: Box<dyn AgentDispatchRepository>,
        adapter_factory: Box<dyn AgentDispatchAdapterFactory>,
        inspector: Box<dyn RepositoryInspector>,
    ) -> Self {
        Self {
            records,
            workspaces: None,
            managed_worktrees: None,
            worktree_adapter: None,
            dispatch: Some(dispatch),
            adapter_factory: Some(adapter_factory),
            provider_configuration: std::sync::Mutex::new(()),
            inspector: inspector.into(),
        }
    }

    /// Creates an application with both managed Worktrees and Agent Dispatch enabled.
    #[must_use]
    pub fn with_managed_worktrees_and_agent_dispatch(
        records: Box<dyn RecordRepository>,
        managed_worktrees: Box<dyn ManagedWorktreeRepository>,
        worktree_adapter: Box<dyn ManagedWorktreeAdapter>,
        dispatch: Box<dyn AgentDispatchRepository>,
        adapter_factory: Box<dyn AgentDispatchAdapterFactory>,
        inspector: Box<dyn RepositoryInspector>,
    ) -> Self {
        Self {
            records,
            workspaces: None,
            managed_worktrees: Some(managed_worktrees),
            worktree_adapter: Some(worktree_adapter),
            dispatch: Some(dispatch),
            adapter_factory: Some(adapter_factory),
            provider_configuration: std::sync::Mutex::new(()),
            inspector: inspector.into(),
        }
    }

    /// Creates a request-local application for Record operations in one Workspace.
    /// Dispatch, provider configuration and Worktree operations are deliberately unavailable
    /// by this record-only composition; it never changes the daemon's default selection.
    ///
    /// # Errors
    /// Returns a user-facing error for missing or unsupported Workspace selection.
    pub fn records_in_workspace(&self, workspace: &str) -> Result<Self, RegistrationError> {
        Ok(Self {
            records: self.records.records_in_workspace(workspace)?,
            workspaces: None,
            managed_worktrees: None,
            worktree_adapter: None,
            dispatch: None,
            adapter_factory: None,
            inspector: self.inspector.clone(),
            provider_configuration: std::sync::Mutex::new(()),
        })
    }

    fn managed_worktrees(&self) -> Result<&dyn ManagedWorktreeRepository, RegistrationError> {
        self.managed_worktrees.as_deref().ok_or_else(|| {
            RegistrationError::new(
                "managed-worktrees-unavailable",
                "Managed Worktrees are not configured for this daemon",
            )
        })
    }

    fn worktree_adapter(&self) -> Result<&dyn ManagedWorktreeAdapter, RegistrationError> {
        self.worktree_adapter.as_deref().ok_or_else(|| {
            RegistrationError::new(
                "managed-worktrees-unavailable",
                "Managed Worktree Git operations are not configured for this daemon",
            )
        })
    }

    fn dispatch(&self) -> Result<&dyn AgentDispatchRepository, RegistrationError> {
        self.dispatch.as_deref().ok_or_else(|| {
            RegistrationError::new(
                "agent-dispatch-unavailable",
                "Agent Dispatch is not configured for this daemon",
            )
        })
    }

    fn adapter_factory(&self) -> Result<&dyn AgentDispatchAdapterFactory, RegistrationError> {
        self.adapter_factory.as_deref().ok_or_else(|| {
            RegistrationError::new(
                "agent-dispatch-unavailable",
                "Agent Dispatch provider is not configured for this daemon",
            )
        })
    }

    /// Sets or clears the absolute Herdr endpoint socket path.
    ///
    /// # Errors
    ///
    /// Returns validation, configuration, or persistence failures.
    pub fn set_herdr_socket_path(&self, path: Option<&Path>) -> Result<(), RegistrationError> {
        let _guard = self.provider_configuration.lock().map_err(|_| {
            RegistrationError::new(
                "configuration-lock",
                "provider configuration lock is poisoned",
            )
        })?;
        self.dispatch()?.set_herdr_socket_path(path)
    }

    fn dispatcher_config_project(project: Option<&str>) -> Result<&str, RegistrationError> {
        project
            .filter(|slug| !slug.trim().is_empty())
            .ok_or_else(|| {
                RegistrationError::new(
                    "project-required",
                    "project-limit requires explicit --project <slug> scope",
                )
            })
    }

    /// Explicitly dispatches an interactive Grilling using its immutable profile.
    /// Task dispatch fails closed until the Check execution contract is supported.
    ///
    /// # Errors
    ///
    /// Returns eligibility, recovery, provider, or persistence failures.
    pub fn dispatch_record(
        &self,
        record_id: u64,
    ) -> Result<(AgentAssignment, bool), RegistrationError> {
        let dispatch = self.dispatch()?;
        let record = dispatch.show_record(record_id)?;
        if !matches!(record.record_type, RecordType::Task | RecordType::Grilling) {
            return Err(RegistrationError::new(
                "unsupported-record-type",
                "explicit dispatch supports only Tasks and Grillings",
            ));
        }
        if record.record_type == RecordType::Task {
            validate_task_dispatch_mode(&record)?;
            return Err(task_dispatch_unsupported());
        }
        if let Some(assignment) = dispatch
            .list_agent_assignments(record_id)?
            .into_iter()
            .find(|a| a.closed_at_millis.is_none())
        {
            return Ok((assignment, true));
        }
        let readiness = self.workspace_dispatch_readiness(&record.workspace_id)?;
        let selection = readiness.preflight_selection()?;
        let (assignment, already_assigned) = dispatch.claim_record_agent_assignment_checked(
            record_id,
            &next_dispatch_assignment_id(),
            &selection,
        )?;
        if already_assigned {
            return Ok((assignment, true));
        }
        let adapter = self
            .adapter_factory()?
            .adapter_for(&assignment.profile)
            .map_err(|error| {
                RegistrationError::new("agent-dispatch-unavailable", format!("{error:?}"))
            })?;
        if workspace_dispatch::provisioning_available(
            dispatch,
            self.adapter_factory()?,
            &assignment,
        ) {
            let _ = reconcile_agent_assignment(dispatch, adapter.as_ref(), &assignment)?;
        }
        Ok((dispatch.agent_assignment(&assignment.id)?, false))
    }

    /// Acknowledges an Assignment after transport-level capability authentication.
    #[allow(clippy::missing_errors_doc)]
    pub fn acknowledge_assignment(
        &self,
        assignment_id: &str,
        correlation_id: &str,
    ) -> Result<AgentAssignment, RegistrationError> {
        assignment_acknowledge(
            self.dispatch()?,
            assignment_id,
            assignment_id,
            correlation_id,
        )
    }

    /// Reads rendered instructions after authenticated start and persisted trusted context.
    #[allow(clippy::missing_errors_doc)]
    pub fn assignment_instructions(
        &self,
        assignment_id: &str,
        correlation_id: &str,
    ) -> Result<String, RegistrationError> {
        let repository = self.dispatch()?;
        let assignment = repository.agent_assignment(assignment_id)?;
        let task = repository.show_record(assignment.record_id)?;
        authorize_assignment_operation(
            repository,
            &assignment,
            assignment_id,
            correlation_id,
            AssignmentAuthorizationOperation::ReadContext,
            task.id,
            assignment.closed_at_millis.is_none()
                && matches!(
                    assignment.state,
                    AgentAssignmentState::Working
                        | AgentAssignmentState::Idle
                        | AgentAssignmentState::Blocked
                )
                && matches!(task.record_type, RecordType::Task | RecordType::Grilling)
                && task.status == RecordStatus::InProgress
                && repository.assignment_context_read(assignment_id)?,
        )?;
        if let Some(policy) = assignment.profile.launch_policy {
            Ok(policy.behavior)
        } else {
            Ok(assignment.profile.prompt_template)
        }
    }

    /// Reports a bootstrap failure after transport authentication.
    #[allow(clippy::missing_errors_doc)]
    pub fn report_assignment_bootstrap_failure(
        &self,
        assignment_id: &str,
        correlation_id: &str,
        message: &str,
    ) -> Result<(), RegistrationError> {
        let repository = self.dispatch()?;
        let assignment = repository.agent_assignment(assignment_id)?;
        assignment_add_comment(
            repository,
            assignment_id,
            assignment_id,
            correlation_id,
            assignment.record_id,
            message,
        )
        .map(|_| ())
    }

    /// Starts a Task only after its Assignment has acknowledged.
    #[allow(clippy::missing_errors_doc)]
    pub fn start_acknowledged_assignment_task(
        &self,
        assignment_id: &str,
        correlation_id: &str,
        task_id: u64,
    ) -> Result<Record, RegistrationError> {
        let info = self.dispatch()?.agent_assignment_info(assignment_id)?;
        if info.acknowledged_at_millis.is_none() {
            return Err(RegistrationError::new(
                "assignment-not-acknowledged",
                "Assignment has not acknowledged",
            ));
        }
        self.start_assignment_task(assignment_id, assignment_id, correlation_id, task_id)
    }

    /// Starts an Assignment's bound Task through its delivered capability.
    ///
    /// Repeating this operation returns the already In Progress Task rather than creating a
    /// second lifecycle transition.
    ///
    /// # Errors
    ///
    /// Returns authorization or lifecycle failures.
    pub fn start_assignment_task(
        &self,
        assignment_id: &str,
        capability_id: &str,
        correlation_id: &str,
        task_id: u64,
    ) -> Result<Record, RegistrationError> {
        assignment_start_task(
            self.dispatch()?,
            assignment_id,
            capability_id,
            correlation_id,
            task_id,
        )
    }

    /// Reopens an unassigned In Progress Task without clearing any Dispatch Hold.
    ///
    /// # Errors
    ///
    /// Returns lifecycle, assignment, or persistence failures.
    pub fn reopen_task(&self, task_id: u64) -> Result<Record, RegistrationError> {
        let dispatch = self.dispatch()?;
        let task = dispatch.show_record(task_id)?;
        if task.record_type != RecordType::Task {
            return Err(RegistrationError::new(
                "not-task",
                "only Tasks may be reopened",
            ));
        }
        if dispatch
            .list_agent_assignments(task_id)?
            .iter()
            .any(|assignment| assignment.closed_at_millis.is_none())
        {
            return Err(RegistrationError::new(
                "task-assignment-active",
                "retire the active Assignment before reopening this Task",
            ));
        }
        dispatch.reopen_record(task_id)
    }

    /// Clears the operator-controlled automatic-dispatch hold for a Task.
    ///
    /// # Errors
    ///
    /// Returns persistence failures.
    pub fn resume_task_dispatch(&self, task_id: u64) -> Result<(), RegistrationError> {
        self.dispatch()?.resume_dispatch(task_id)
    }

    /// Inspects persisted Assignment evidence without contacting the provider or changing state.
    ///
    /// # Errors
    /// Returns an error when dispatch is unavailable or the Assignment does not exist.
    pub fn agent_assignment_info(
        &self,
        assignment_id: &str,
    ) -> Result<AgentAssignmentInfo, RegistrationError> {
        self.dispatch()?.agent_assignment_info(assignment_id)
    }

    /// Revokes an Assignment before provider cleanup, preserving all Git evidence.
    ///
    /// A durable retirement marker reserves capacity until cleanup succeeds, including after
    /// a crash. Closed Assignments with unresolved resources remain retryable. A separate
    /// Dispatch Hold requires explicit operator review before replacement.
    ///
    /// # Errors
    ///
    /// Returns adapter, storage, or lifecycle failures.
    pub fn retire_agent_assignment(
        &self,
        assignment_id: &str,
    ) -> Result<AgentAssignment, RegistrationError> {
        let dispatch = self.dispatch()?;
        let assignment = dispatch.agent_assignment(assignment_id)?;
        if assignment.closed_at_millis.is_some()
            && dispatch
                .list_agent_assignment_resources(assignment_id)?
                .iter()
                .all(|resource| resource.retired_at_millis.is_some())
        {
            return Ok(assignment);
        }
        dispatch.hold_dispatch(
            assignment.record_id,
            "Assignment retirement requested; resume dispatch explicitly after recovery review.",
        )?;
        // Persist the blocker before revocation: failure or a crash at any later step must
        // not free capacity. Closure revokes capability access atomically.
        dispatch.add_agent_assignment_resource(&NewAgentAssignmentResource {
            assignment_id: assignment_id.into(),
            resource_kind: "retirement-pending".into(),
            resource_id: "operator".into(),
        })?;
        let assignment = if assignment.closed_at_millis.is_none() {
            dispatch.close_agent_assignment(assignment_id, AgentAssignmentClosureReason::Retired)?
        } else {
            assignment
        };
        let context = agent_dispatch_context(dispatch, &assignment)?;
        let adapter = self
            .adapter_factory()?
            .adapter_for(&assignment.profile)
            .map_err(|error| {
                RegistrationError::new("agent-retirement-unavailable", format!("{error:?}"))
            })?;
        adapter.retire(&context, dispatch).map_err(|error| {
            RegistrationError::new("agent-retirement-failed", format!("{error:?}"))
        })?;
        dispatch.retire_agent_assignment_resource(
            assignment_id,
            "retirement-pending",
            "operator",
        )?;
        Ok(assignment)
    }

    /// Records an operator assertion that unknown resources are safe, after revocation.
    /// This is not provider verification and does not clear known resources or the hold.
    ///
    /// # Errors
    /// Returns an error for absent confirmation, an open Assignment, or storage failure.
    pub fn resolve_unknown_agent_resources(
        &self,
        assignment_id: &str,
        confirm_old_agent_cannot_act: bool,
        confirm_unknown_resources_safe: bool,
    ) -> Result<AgentAssignmentInfo, RegistrationError> {
        if !confirm_old_agent_cannot_act || !confirm_unknown_resources_safe {
            return Err(RegistrationError::new(
                "confirmation-required",
                "Risk of duplicate agents and external actions: explicitly confirm the old agent can no longer act AND unknown resources have been removed or otherwise made safe. Revocation cannot undo external actions.",
            ));
        }
        self.dispatch()?
            .resolve_unknown_agent_resources(assignment_id)?;
        self.agent_assignment_info(assignment_id)
    }

    /// Returns the greatest committed global Record Update cursor.
    ///
    /// # Errors
    ///
    /// Returns a dispatch storage error.
    pub fn record_update_head(&self) -> Result<RecordUpdateId, RegistrationError> {
        self.dispatch()?.record_update_head()
    }

    /// Sets or clears a registered Project's Task dispatch capacity override.
    ///
    /// # Errors
    ///
    /// Returns validation, configuration, or persistence failures.
    pub fn set_project_task_concurrency_limit(
        &self,
        project_slug: &str,
        limit: Option<u16>,
    ) -> Result<(), RegistrationError> {
        self.dispatch()?
            .set_project_task_concurrency_limit(project_slug, limit)
    }

    /// Returns dispatch configuration and, optionally, one Project's capacity facts.
    ///
    /// # Errors
    ///
    /// Returns configuration, Project lookup, or persistence failures.
    pub fn agent_dispatch_status(
        &self,
        project_slug: Option<&str>,
    ) -> Result<AgentDispatchStatus, RegistrationError> {
        let dispatch = self.dispatch()?;
        let socket = dispatch.herdr_socket_path()?;
        let project = project_slug
            .map(|slug| {
                Ok(AgentDispatchProjectStatus {
                    project_slug: slug.into(),
                    capacity: dispatch.effective_project_task_concurrency_limit(slug)?,
                    configured_capacity: dispatch.project_task_concurrency_limit(slug)?,
                })
            })
            .transpose()?;
        Ok(AgentDispatchStatus {
            endpoint_diagnostic: if socket.is_some() {
                "daemon endpoint configured; see Workspace readiness for current availability"
                    .into()
            } else {
                "unset".into()
            },
            herdr_socket_path: socket,
            project,
        })
    }

    /// Requests one bounded reconciliation scan owned entirely by this daemon.
    ///
    /// # Errors
    ///
    /// Returns configuration or persistence failures.
    pub fn reconcile_agent_dispatch(
        &self,
        daemon_id: &str,
        now_millis: u64,
    ) -> Result<AgentDispatchReconcileResult, RegistrationError> {
        let scan = reconcile_agent_dispatch(
            self.dispatch()?,
            self.adapter_factory()?,
            daemon_id,
            now_millis,
            AgentDispatchReconciliationPolicy {
                scan_lease_millis: 30_000,
            },
        )
        .inspect_err(|error| {
            tracing::warn!(
                phase = "reconcile-assignments",
                code = error.code,
                "agent dispatch phase failed"
            );
        })?;
        if scan.acquired_lease {
            self.schedule_agent_dispatch().inspect_err(|error| {
                tracing::warn!(
                    phase = "schedule-tasks",
                    code = error.code,
                    "agent dispatch phase failed"
                );
            })?;
        }
        Ok(AgentDispatchReconcileResult {
            acquired_lease: scan.acquired_lease,
            reconciled_assignments: scan.reconciled.len(),
        })
    }

    /// Rejects automatic Task dispatch before claiming capacity or preparing Git resources.
    fn schedule_agent_dispatch(&self) -> Result<(), RegistrationError> {
        let dispatch = self.dispatch()?;
        let projects: BTreeSet<_> = dispatch
            .list_records(&RecordListFilter {
                project_slug: None,
                status: Some(RecordStatus::Open),
                record_type: Some(RecordType::Task),
            })
            .inspect_err(|error| {
                tracing::warn!(
                    operation = "list-open-tasks",
                    code = error.code,
                    "agent dispatch scheduling failed"
                );
            })?
            .into_iter()
            .filter(|record| {
                record.project_slugs.len() == 1
                    && record.execution_mode == ExecutionMode::Dispatched
            })
            .map(|record| record.project_slugs[0].clone())
            .collect();
        for project_slug in projects {
            tracing::warn!(project = %project_slug,
                code = "task-dispatch-unsupported-contract",
                "Task dispatch is disabled pending the Check execution contract");
        }
        Ok(())
    }

    /// Prepares the deterministic dispatcher-owned checkout for an eligible Task.
    ///
    /// The persisted Base Commit is the only permitted checkout base and the Task Branch is
    /// derived solely from the Record identity. A failed preparation retains its Claim and
    /// checkout evidence for operator recovery; it never removes or resets Git state.
    ///
    /// # Errors
    ///
    /// Returns a stable lifecycle, base-commit, hook, or Git failure.
    pub fn prepare_dispatch_worktree(
        &self,
        record_id: u64,
    ) -> Result<WorktreeClaim, RegistrationError> {
        let managed = self.managed_worktrees()?;
        validate_task_dispatch_mode(&managed.show_record(record_id)?)?;
        let base = managed.task_base_commit(record_id)?.ok_or_else(|| {
            let error = RegistrationError::new(
                "task-base-commit-unavailable",
                "Task has no persisted Base Commit for Managed Worktree preparation",
            );
            let _ = managed.add_record_comment(
                record_id,
                "Managed Worktree preparation failed: Task has no persisted Base Commit.",
                Some("agent-dispatch"),
            );
            error
        })?;
        self.create_managed_worktree(record_id, &format!("feat/forge-{record_id}"), &base)
    }

    /// Creates and activates a Managed Worktree for an eligible open Task.
    ///
    /// Dispatcher callers should use [`Self::prepare_dispatch_worktree`] so branch and base
    /// evidence cannot be selected by the caller.
    ///
    /// # Errors
    ///
    /// Returns a stable lifecycle, hook, or Git failure while retaining failure evidence.
    pub fn create_managed_worktree(
        &self,
        record_id: u64,
        branch: &str,
        base: &str,
    ) -> Result<WorktreeClaim, RegistrationError> {
        let managed = self.managed_worktrees()?;
        let adapter = self.worktree_adapter()?;
        let record = managed.show_record(record_id)?;
        if !is_worktree_eligible(&record) {
            return Err(RegistrationError::new(
                "task-not-open",
                "a Managed Worktree requires an Open Task scoped to exactly one Project",
            ));
        }
        if let Some(claim) = managed.worktree_claim(record_id)? {
            return match claim.state {
                WorktreeClaimState::Active => {
                    let info = self.managed_worktree_info(record_id)?;
                    if info.safety.facts.iter().any(|fact| {
                        matches!(fact.as_str(), "checkout-missing" | "git-metadata-invalid")
                    }) {
                        return Err(RegistrationError::new(
                            "managed-worktree-invalid",
                            "Retained Managed Worktree is missing or invalid; inspect and repair it before retrying",
                        ));
                    }
                    Ok(claim)
                }
                WorktreeClaimState::Reserving | WorktreeClaimState::CleanupRequired => {
                    Err(RegistrationError::new(
                        "claim-requires-recovery",
                        "Task Managed Worktree preparation requires explicit operator recovery",
                    ))
                }
            };
        }
        validate_managed_worktree_placement(&managed.worktree_path(record_id)?)?;
        let claim = managed.reserve_worktree_claim(record_id)?;
        let project = match managed.project(&claim.project_slug) {
            Ok(project) => project,
            Err(error) => return self.retain_worktree_preparation_failure(&claim, error),
        };
        if let Err(error) = adapter.create(&project.project_root, &claim.path, branch, base) {
            return self.retain_worktree_preparation_failure(&claim, error);
        }
        let hook = match managed.project_setup_hook(&claim.project_slug) {
            Ok(hook) => hook,
            Err(error) => return self.retain_worktree_preparation_failure(&claim, error),
        };
        if let Some(hook) = hook
            && let Err(error) = adapter.run_setup_hook(
                &claim.path,
                &hook,
                &claim.project_slug,
                record_id,
                branch,
                base,
            )
        {
            return self.retain_worktree_preparation_failure(&claim, error);
        }
        let assessment = adapter.assess(&claim.path);
        if assessment
            .facts
            .iter()
            .any(|fact| matches!(fact.as_str(), "checkout-missing" | "git-metadata-invalid"))
        {
            return self.retain_worktree_preparation_failure(
                &claim,
                RegistrationError::new(
                    "managed-worktree-invalid",
                    "Managed Worktree checkout is missing or invalid after preparation",
                ),
            );
        }
        managed.activate_worktree_claim(record_id)
    }

    fn retain_worktree_preparation_failure(
        &self,
        claim: &WorktreeClaim,
        error: RegistrationError,
    ) -> Result<WorktreeClaim, RegistrationError> {
        let managed = self.managed_worktrees()?;
        managed.require_worktree_claim_cleanup(claim.record_id)?;
        let _ = managed.add_record_comment(
            claim.record_id,
            &format!("Managed Worktree preparation failed: {}", error.message),
            Some("agent-dispatch"),
        );
        Err(error)
    }

    /// Recover an unclaimed checkout after explicit operator confirmation.
    ///
    /// # Errors
    /// Returns validation, assignment, or persistence errors; the dispatch hold is retained.
    pub fn recover_managed_worktree(
        &self,
        record_id: u64,
        confirmed: bool,
    ) -> Result<WorktreeClaim, RegistrationError> {
        if !confirmed {
            return Err(RegistrationError::new(
                "confirmation-required",
                "Confirm no old agent is active before recovery",
            ));
        }
        let managed = self.managed_worktrees()?;
        let dispatch = self.dispatch()?;
        let record = managed.show_record(record_id)?;
        if !is_worktree_eligible(&record) {
            return Err(RegistrationError::new(
                "task-not-open",
                "Recovery requires an Open single-Project Task",
            ));
        }
        dispatch.hold_dispatch(
            record_id,
            "Operator worktree recovery; resume only after verification",
        )?;
        let project = managed.project(&record.project_slugs[0])?;
        let path = managed.worktree_path(record_id)?;
        validate_managed_worktree_placement(&path)?;
        let base = managed.task_base_commit(record_id)?.ok_or_else(|| {
            RegistrationError::new(
                "task-base-commit-unavailable",
                "Task has no persisted Base Commit",
            )
        })?;
        self.worktree_adapter()?.validate_recovery(
            &project.project_root,
            &path,
            &format!("feat/forge-{record_id}"),
            &base,
        )?;
        managed.recover_worktree_claim(record_id, &path)
    }

    /// Configures a registered Project's Setup Hook.
    ///
    /// # Errors
    ///
    /// Returns persistence or Project-validation failures.
    pub fn set_project_setup_hook(
        &self,
        project_slug: &str,
        hook: Option<&str>,
    ) -> Result<(), RegistrationError> {
        self.managed_worktrees()?
            .set_project_setup_hook(project_slug, hook)
    }

    /// Lists managed Worktrees and their current safety assessments.
    ///
    /// # Errors
    ///
    /// Returns persistence or adapter failures.
    pub fn list_managed_worktrees(
        &self,
        include_all: bool,
    ) -> Result<Vec<ManagedWorktreeInfo>, RegistrationError> {
        self.list_workspace_worktrees(None, include_all)
    }

    /// Lists only the selected Workspace's Claims, assessing no foreign checkout.
    ///
    /// # Errors
    /// Returns invalid selection, persistence, or placement failures.
    pub fn list_workspace_managed_worktrees(
        &self,
        workspace: &str,
        include_all: bool,
    ) -> Result<Vec<ManagedWorktreeInfo>, RegistrationError> {
        self.records.records_in_workspace(workspace)?;
        self.list_workspace_worktrees(Some(workspace), include_all)
    }

    fn list_workspace_worktrees(
        &self,
        workspace: Option<&str>,
        include_all: bool,
    ) -> Result<Vec<ManagedWorktreeInfo>, RegistrationError> {
        let managed = self.managed_worktrees()?;
        let claims = if include_all {
            managed.worktree_claims()?
        } else {
            managed.worktree_claims_requiring_cleanup()?
        };
        let mut result = Vec::new();
        for claim in claims {
            let record = managed.show_record(claim.record_id)?;
            if workspace.is_none_or(|workspace| record.workspace_id == workspace) {
                result.push(self.managed_worktree_info(claim.record_id)?);
            }
        }
        Ok(result)
    }

    /// Returns a Managed Worktree Claim and its local safety assessment.
    ///
    /// # Errors
    ///
    /// Returns a missing Claim or Git adapter failure.
    pub fn managed_worktree_info(
        &self,
        record_id: u64,
    ) -> Result<ManagedWorktreeInfo, RegistrationError> {
        let claim = self
            .managed_worktrees()?
            .worktree_claim(record_id)?
            .ok_or_else(|| {
                RegistrationError::new("claim-not-found", "Task has no Managed Worktree Claim")
            })?;
        let expected = self.managed_worktrees()?.worktree_path(record_id)?;
        if claim.path != expected {
            return Err(RegistrationError::new(
                "managed-worktree-path-invalid",
                "Retained checkout is outside the fixed Workspace layout; repair its placement explicitly",
            ));
        }
        validate_managed_worktree_placement(&claim.path)?;
        let mut safety = self.worktree_adapter()?.assess(&claim.path);
        match self
            .managed_worktrees()?
            .ensure_worktree_removable(record_id)
        {
            Ok(()) => {}
            Err(error) if error.code == "worktree-in-use" => {
                safety.facts.push("worktree-in-use".into());
                safety.blockers.push("worktree-in-use".into());
                safety.removable = false;
            }
            Err(error) => return Err(error),
        }
        Ok(ManagedWorktreeInfo { claim, safety })
    }

    /// Removes a Managed Worktree after local safety checks, unless explicitly forced.
    ///
    /// # Errors
    ///
    /// Returns an unsafe-removal, cleanup, or Git adapter failure.
    pub fn remove_managed_worktree(
        &self,
        record_id: u64,
        force: bool,
    ) -> Result<(), RegistrationError> {
        self.managed_worktrees()?
            .ensure_worktree_removable(record_id)?;
        let info = self.managed_worktree_info(record_id)?;
        if info.claim.state != WorktreeClaimState::Active && !force {
            return Err(RegistrationError::new(
                "managed-worktree-invalid",
                "Managed Worktree Claim requires doctor repair",
            ));
        }
        if !force && !info.safety.removable {
            return Err(RegistrationError::new(
                "unsafe-removal",
                "Managed Worktree cannot be ordinarily removed",
            ));
        }
        if let Err(error) = self.worktree_adapter()?.remove(&info.claim.path, force) {
            self.managed_worktrees()?
                .require_worktree_claim_cleanup(record_id)?;
            return Err(error);
        }
        self.managed_worktrees()?.delete_worktree_claim(record_id)
    }

    /// Explicitly repairs a Claim requiring cleanup by force-removing its checkout.
    ///
    /// # Errors
    ///
    /// Returns a cleanup or Git adapter failure.
    pub fn repair_managed_worktree(&self, record_id: u64) -> Result<(), RegistrationError> {
        self.managed_worktrees()?
            .ensure_worktree_removable(record_id)?;
        let info = self.managed_worktree_info(record_id)?;
        if let Err(error) = self.worktree_adapter()?.remove(&info.claim.path, true) {
            self.managed_worktrees()?
                .require_worktree_claim_cleanup(record_id)?;
            return Err(error);
        }
        self.managed_worktrees()?.delete_worktree_claim(record_id)
    }

    /// Registers an eligible Git main checkout using a qualified `<workspace>/<slug>` reference.
    ///
    /// # Errors
    ///
    /// Returns the validation or persistence failure suitable for the calling adapter.
    pub fn register_project(
        &self,
        folder: &Path,
        reference: &str,
    ) -> Result<Project, RegistrationError> {
        let reference = ProjectReference::parse(reference)?;
        self.register_workspace_project(&reference.workspace_id, folder, &reference.slug)
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
        self.open_record_with_generators(body, project_slugs, vec![])
    }

    /// Opens a generic Record and atomically records its provenance.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for invalid input, missing generators, or storage failures.
    pub fn open_record_with_generators(
        &self,
        body: &str,
        project_slugs: Vec<String>,
        generated_by_ids: Vec<u64>,
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
            generated_by_ids,
        })
    }

    /// Opens a Spec for exactly one Project.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for a blank body or storage failure.
    pub fn open_spec(&self, body: &str, project_slug: String) -> Result<Record, RegistrationError> {
        self.open_typed_record(RecordType::Spec, body, vec![project_slug])
    }

    /// Opens a Wayfinder with immutable Project Scope, possibly Projectless within a selected Workspace.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for a blank body, invalid scope, or storage failure.
    pub fn open_wayfinder(
        &self,
        body: &str,
        mut project_slugs: Vec<String>,
    ) -> Result<Record, RegistrationError> {
        project_slugs.sort();
        if project_slugs.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(RegistrationError::new(
                "duplicate-project",
                "Wayfinder Project Scope must not contain duplicate Project slugs",
            ));
        }
        self.open_typed_record(RecordType::Wayfinder, body, project_slugs)
    }

    /// Opens a standalone Task with zero or one Project Scope.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for a blank body or storage failure.
    pub fn open_task(
        &self,
        body: &str,
        project_slug: Option<String>,
    ) -> Result<Record, RegistrationError> {
        self.open_typed_record(RecordType::Task, body, project_slug.into_iter().collect())
    }

    /// Opens a Task under `spec_id`, inheriting its scope and membership atomically.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for a blank body, invalid parent, or storage failure.
    pub fn open_task_under_spec(
        &self,
        body: &str,
        spec_id: u64,
        generated_by_ids: Vec<u64>,
    ) -> Result<Record, RegistrationError> {
        Self::require_body(body)?;
        let spec = self.records.show_record(spec_id)?;
        let base_commit = self.task_base_commit_for_scope(&spec.project_slugs)?;
        let task = self
            .records
            .open_task_under_spec(body, spec_id, generated_by_ids)?;
        if let Some(base_commit) = base_commit {
            self.records.set_task_base_commit(task.id, &base_commit)?;
        }
        Ok(task)
    }

    /// Opens a Task under a Wayfinder, selecting one Project from its scope.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for a blank body, invalid parent or scope, or storage failure.
    pub fn open_task_under_wayfinder(
        &self,
        body: &str,
        wayfinder_id: u64,
        project_slug: Option<String>,
        generated_by_ids: Vec<u64>,
    ) -> Result<Record, RegistrationError> {
        Self::require_body(body)?;
        let scope = match &project_slug {
            Some(project_slug) => vec![project_slug.clone()],
            None => self.records.show_record(wayfinder_id)?.project_slugs,
        };
        let base_commit = self.task_base_commit_for_scope(&scope)?;
        let task = self.records.open_task_under_wayfinder(
            body,
            wayfinder_id,
            project_slug,
            generated_by_ids,
        )?;
        if let Some(base_commit) = base_commit {
            self.records.set_task_base_commit(task.id, &base_commit)?;
        }
        Ok(task)
    }

    /// Opens a Grilling beneath a Wayfinder, inheriting or selecting its Project Scope.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for a blank body, invalid parent, or invalid scope.
    pub fn open_grilling_under_wayfinder(
        &self,
        body: &str,
        wayfinder_id: u64,
        project_slugs: Option<Vec<String>>,
        generated_by_ids: Vec<u64>,
    ) -> Result<Record, RegistrationError> {
        Self::require_body(body)?;
        self.records.open_grilling_under_wayfinder(
            body,
            wayfinder_id,
            project_slugs,
            generated_by_ids,
        )
    }

    /// Lists Records matching the supplied optional discovery filters.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when storage fails.
    pub fn list_records(
        &self,
        project_slug: Option<String>,
        status: Option<RecordStatus>,
        record_type: Option<RecordType>,
    ) -> Result<Vec<Record>, RegistrationError> {
        self.records.list_records(&RecordListFilter {
            project_slug,
            status,
            record_type,
        })
    }

    /// Returns a Record by its opaque identity.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error if the Record is absent.
    pub fn show_record(&self, id: u64) -> Result<Record, RegistrationError> {
        self.records.show_record(id)
    }

    /// Edits a Record Body before it is Done.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for blank Bodies or completed Records.
    pub fn edit_record(&self, id: u64, body: &str) -> Result<Record, RegistrationError> {
        if body.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-body",
                "record body must not be blank",
            ));
        }
        self.records.edit_record(id, body)
    }

    /// Advances a Record from Open to In Progress.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the transition is invalid.
    pub fn start_record(&self, id: u64) -> Result<Record, RegistrationError> {
        self.records.start_record(id)
    }

    /// Explicitly change policy without claiming, launching or clearing safety blocks.
    ///
    /// # Errors
    /// Rejects invalid pinned profiles, unsafe handoff, stale mode or scope violations.
    pub fn set_record_execution_mode(
        &self,
        id: u64,
        expected: ExecutionMode,
        mode: ExecutionMode,
        actor: &str,
        message: &str,
    ) -> Result<Record, RegistrationError> {
        let record = self.records.show_record(id)?;
        if record.record_type == RecordType::Task
            && mode == ExecutionMode::Dispatched
            && expected != mode
        {
            self.validate_record_check_profiles(id)?;
        }
        self.records
            .set_record_execution_mode(id, expected, mode, actor, message)
    }

    /// Completes an in-progress Record with an optional Resolution.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for an invalid transition or blank Resolution.
    pub fn done_record(
        &self,
        id: u64,
        completion_resolution: Option<&str>,
    ) -> Result<Record, RegistrationError> {
        if completion_resolution.is_some_and(|resolution| resolution.trim().is_empty()) {
            return Err(RegistrationError::new(
                "invalid-resolution",
                "completion resolution must not be blank",
            ));
        }
        self.records.done_record(id, completion_resolution)
    }

    /// Adds a typed directional Relationship.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for invalid Relationship changes.
    pub fn relate_records(
        &self,
        from_id: u64,
        to_id: u64,
        kind: RecordRelationshipKind,
    ) -> Result<Record, RegistrationError> {
        self.records.relate_records(from_id, to_id, kind)
    }

    /// Removes a typed directional Relationship.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the requested Relationship is absent.
    pub fn unrelate_records(
        &self,
        from_id: u64,
        to_id: u64,
        kind: RecordRelationshipKind,
    ) -> Result<Record, RegistrationError> {
        self.records.unrelate_records(from_id, to_id, kind)
    }

    fn require_body(body: &str) -> Result<(), RegistrationError> {
        if body.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-body",
                "record body must not be blank",
            ));
        }
        Ok(())
    }

    fn open_typed_record(
        &self,
        record_type: RecordType,
        body: &str,
        project_slugs: Vec<String>,
    ) -> Result<Record, RegistrationError> {
        self.open_typed_record_with_generators(record_type, body, project_slugs, vec![])
    }

    /// Opens a typed Record and atomically records its provenance.
    ///
    /// Transport adapters use this entry point when Record-opening input names generators.
    ///
    /// # Errors
    ///
    /// Returns a user-facing validation, generator lookup, or storage error.
    pub fn open_typed_record_with_generators(
        &self,
        record_type: RecordType,
        body: &str,
        project_slugs: Vec<String>,
        generated_by_ids: Vec<u64>,
    ) -> Result<Record, RegistrationError> {
        Self::require_body(body)?;
        let base_commit = (record_type == RecordType::Task)
            .then(|| self.task_base_commit_for_scope(&project_slugs))
            .transpose()?;
        let record = self.records.open_typed_record(&NewTypedRecord {
            record_type,
            body: body.to_owned(),
            project_slugs,
            generated_by_ids,
        })?;
        if let Some(Some(base_commit)) = base_commit {
            self.records.set_task_base_commit(record.id, &base_commit)?;
        }
        Ok(record)
    }

    fn task_base_commit_for_scope(
        &self,
        project_slugs: &[String],
    ) -> Result<Option<String>, RegistrationError> {
        match project_slugs {
            [project_slug] => {
                let project = self.records.project(project_slug)?;
                self.inspector.head_commit(&project.project_root).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// Lists a Record's Forge-authored Update Log newest first.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error if the Record is absent or storage fails.
    pub fn list_record_updates(&self, id: u64) -> Result<Vec<RecordUpdate>, RegistrationError> {
        self.records.list_record_updates(id)
    }

    /// Adds a non-blank immutable Comment to a Record.
    ///
    /// # Errors
    ///
    /// Returns a validation, Record lookup, or storage error.
    pub fn add_record_comment(
        &self,
        id: u64,
        body: &str,
        author: Option<&str>,
    ) -> Result<RecordComment, RegistrationError> {
        if body.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-comment",
                "comment must not be blank",
            ));
        }
        if author.is_some_and(|author| author.trim().is_empty()) {
            return Err(RegistrationError::new(
                "invalid-comment-author",
                "comment author must not be blank",
            ));
        }
        self.records.add_record_comment(id, body, author)
    }

    /// Lists current Comments newest first.
    ///
    /// # Errors
    ///
    /// Returns a Record lookup or storage error.
    pub fn list_record_comments(&self, id: u64) -> Result<Vec<RecordComment>, RegistrationError> {
        self.records.list_record_comments(id)
    }

    /// Deletes a Comment from a Record.
    ///
    /// # Errors
    ///
    /// Returns a Record/Comment lookup or storage error.
    pub fn delete_record_comment(&self, id: u64, comment_id: u64) -> Result<(), RegistrationError> {
        self.records
            .delete_record_comment(id, RecordCommentId(comment_id))
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

    #[test]
    fn generated_assignment_ids_fit_herdr_agent_names() {
        let id = next_dispatch_assignment_id();
        assert!(format!("forge-dispatch-{id}").len() <= 32);
        assert!(
            id.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        );
    }

    #[test]
    fn dispatch_diagnostics_only_expose_reviewed_adapter_details() {
        assert_eq!(
            safe_adapter_detail("Assignment has no Agent Workspace"),
            "Assignment has no Agent Workspace"
        );
        assert_eq!(
            safe_adapter_detail("assignment ID is too long for a Herdr agent name"),
            "assignment ID is too long for a Herdr agent name"
        );
        assert_eq!(
            safe_adapter_detail("Herdr tab.create: conflict"),
            "Herdr tab.create: conflict"
        );
        assert_eq!(
            safe_adapter_detail("Herdr tab.create: secret-token"),
            "details withheld; adapter message is not allowlisted"
        );
        assert_eq!(
            safe_adapter_detail("Assignment has no Agent Workspace secret-token"),
            "details withheld; adapter message is not allowlisted"
        );
    }

    struct Repository {
        records: Mutex<Vec<Record>>,
        comments: Mutex<Vec<RecordComment>>,
        claims: Mutex<Vec<WorktreeClaim>>,
    }
    impl ProjectRepository for Repository {
        fn project(&self, slug: &str) -> Result<Project, RegistrationError> {
            Ok(Project {
                reference: slug.into(),
                workspace_id: "test".into(),
                slug: slug.into(),
                project_root: PathBuf::from("/test-project"),
                origin_url: "test".into(),
            })
        }

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
                workspace_id: "test".into(),
                id: (records.len() + 1) as u64,
                record_type: RecordType::Generic,
                execution_mode: ExecutionMode::Manual,
                body: record.body.clone(),
                project_slugs: record.project_slugs.clone(),
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![],
            };
            records.push(record.clone());
            Ok(record)
        }
        fn open_typed_record(&self, record: &NewTypedRecord) -> Result<Record, RegistrationError> {
            let mut records = self.records.lock().unwrap();
            let record = Record {
                workspace_id: "test".into(),
                id: (records.len() + 1) as u64,
                record_type: record.record_type,
                execution_mode: if record.record_type == RecordType::Task
                    && !record.project_slugs.is_empty()
                {
                    ExecutionMode::Dispatched
                } else {
                    ExecutionMode::Manual
                },
                body: record.body.clone(),
                project_slugs: record.project_slugs.clone(),
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![],
            };
            records.push(record.clone());
            Ok(record)
        }
        fn set_task_base_commit(&self, _: u64, _: &str) -> Result<(), RegistrationError> {
            Ok(())
        }

        fn task_base_commit(&self, _: u64) -> Result<Option<String>, RegistrationError> {
            Ok(Some("test-base".into()))
        }

        fn open_task_under_spec(
            &self,
            body: &str,
            spec_id: u64,

            _generated_by_ids: Vec<u64>,
        ) -> Result<Record, RegistrationError> {
            let spec = self.show_record(spec_id)?;
            if spec.record_type != RecordType::Spec {
                return Err(RegistrationError::new(
                    "invalid-parent-type",
                    "a Task may be opened under a Spec only",
                ));
            }
            let mut records = self.records.lock().unwrap();
            let task_id = (records.len() + 1) as u64;
            let relationship = RecordRelationship {
                kind: RecordRelationshipKind::ParentChild,
                from_id: task_id,
                to_id: spec_id,
                parent_id: spec_id,
                child_id: task_id,
            };
            let task = Record {
                id: task_id,
                record_type: RecordType::Task,
                body: body.to_owned(),
                project_slugs: spec.project_slugs,
                execution_mode: ExecutionMode::Dispatched,
                workspace_id: spec.workspace_id,
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![relationship.clone()],
            };
            let parent = records
                .iter_mut()
                .find(|record| record.id == spec_id)
                .expect("shown Spec must remain present while the repository lock is held");
            parent.relationships.push(relationship);
            records.push(task.clone());
            Ok(task)
        }
        fn open_task_under_wayfinder(
            &self,
            body: &str,
            wayfinder_id: u64,
            project_slug: Option<String>,

            _generated_by_ids: Vec<u64>,
        ) -> Result<Record, RegistrationError> {
            let wayfinder = self.show_record(wayfinder_id)?;
            if wayfinder.record_type != RecordType::Wayfinder {
                return Err(RegistrationError::new(
                    "invalid-parent-type",
                    "a Task may be opened under a Wayfinder only",
                ));
            }
            let project_slug = match (project_slug, wayfinder.project_slugs.as_slice()) {
                (Some(project_slug), scope) if scope.contains(&project_slug) => project_slug,
                (Some(_), _) => {
                    return Err(RegistrationError::new(
                        "scope-mismatch",
                        "Task Project must belong to the Wayfinder scope",
                    ));
                }
                (None, [project_slug]) => project_slug.clone(),
                (None, _) => {
                    return Err(RegistrationError::new(
                        "missing-project",
                        "a multi-Project Wayfinder Task requires a Project",
                    ));
                }
            };
            let mut records = self.records.lock().unwrap();
            let task_id = (records.len() + 1) as u64;
            let relationship = RecordRelationship {
                kind: RecordRelationshipKind::ParentChild,
                from_id: task_id,
                to_id: wayfinder_id,
                parent_id: wayfinder_id,
                child_id: task_id,
            };
            let task = Record {
                id: task_id,
                record_type: RecordType::Task,
                body: body.to_owned(),
                project_slugs: vec![project_slug],
                execution_mode: ExecutionMode::Dispatched,
                workspace_id: wayfinder.workspace_id,
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![relationship.clone()],
            };
            let parent = records
                .iter_mut()
                .find(|record| record.id == wayfinder_id)
                .unwrap();
            parent.relationships.push(relationship);
            records.push(task.clone());
            Ok(task)
        }
        fn open_grilling_under_wayfinder(
            &self,
            body: &str,
            wayfinder_id: u64,
            project_slugs: Option<Vec<String>>,
            _generated_by_ids: Vec<u64>,
        ) -> Result<Record, RegistrationError> {
            let wayfinder = self.show_record(wayfinder_id)?;
            if wayfinder.record_type != RecordType::Wayfinder {
                return Err(RegistrationError::new(
                    "invalid-parent-type",
                    "a Grilling may be opened under a Wayfinder only",
                ));
            }
            let mut project_slugs = project_slugs.unwrap_or(wayfinder.project_slugs.clone());
            project_slugs.sort();
            if project_slugs.is_empty() {
                return Err(RegistrationError::new(
                    "empty-project-scope",
                    "a Grilling must have at least one Project in its scope",
                ));
            }
            if project_slugs.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(RegistrationError::new(
                    "duplicate-project",
                    "Grilling Project Scope must not contain duplicate Project slugs",
                ));
            }
            if project_slugs
                .iter()
                .any(|slug| !wayfinder.project_slugs.contains(slug))
            {
                return Err(RegistrationError::new(
                    "scope-mismatch",
                    "Grilling Project Scope must be a subset of the Wayfinder scope",
                ));
            }
            let mut records = self.records.lock().unwrap();
            let grilling_id = (records.len() + 1) as u64;
            let relationship = RecordRelationship {
                kind: RecordRelationshipKind::ParentChild,
                from_id: grilling_id,
                to_id: wayfinder_id,
                parent_id: wayfinder_id,
                child_id: grilling_id,
            };
            let grilling = Record {
                workspace_id: wayfinder.workspace_id,
                id: grilling_id,
                record_type: RecordType::Grilling,
                execution_mode: ExecutionMode::Manual,
                body: body.to_owned(),
                project_slugs,
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![relationship.clone()],
            };
            records
                .iter_mut()
                .find(|record| record.id == wayfinder_id)
                .expect("shown Wayfinder must remain present while the repository lock is held")
                .relationships
                .push(relationship);
            records.push(grilling.clone());
            Ok(grilling)
        }
        fn list_records(
            &self,
            filter: &RecordListFilter,
        ) -> Result<Vec<Record>, RegistrationError> {
            Ok(self
                .records
                .lock()
                .unwrap()
                .iter()
                .filter(|record| {
                    filter
                        .project_slug
                        .as_ref()
                        .is_none_or(|slug| record.project_slugs.contains(slug))
                        && filter.status.is_none_or(|status| record.status == status)
                        && filter
                            .record_type
                            .is_none_or(|record_type| record.record_type == record_type)
                })
                .cloned()
                .rev()
                .collect())
        }
        fn show_record(&self, id: u64) -> Result<Record, RegistrationError> {
            self.records
                .lock()
                .unwrap()
                .iter()
                .find(|record| record.id == id)
                .cloned()
                .ok_or_else(|| RegistrationError::new("record-not-found", "Record does not exist"))
        }
        fn edit_record(&self, id: u64, body: &str) -> Result<Record, RegistrationError> {
            let mut records = self.records.lock().unwrap();
            let record = records
                .iter_mut()
                .find(|record| record.id == id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            if record.status == RecordStatus::Done {
                return Err(RegistrationError::new(
                    "invalid-status-transition",
                    "Record is Done",
                ));
            }
            record.body = body.into();
            Ok(record.clone())
        }
        fn start_record(&self, id: u64) -> Result<Record, RegistrationError> {
            let mut records = self.records.lock().unwrap();
            let unfinished = records
                .iter()
                .find(|record| record.id == id)
                .ok_or_else(|| RegistrationError::new("record-not-found", "Record does not exist"))?
                .relationships
                .iter()
                .filter(|relationship| {
                    relationship.kind == RecordRelationshipKind::DependsOn
                        && relationship.from_id == id
                })
                .filter_map(|relationship| {
                    records
                        .iter()
                        .find(|record| record.id == relationship.to_id)
                        .filter(|record| record.status != RecordStatus::Done)
                        .map(|record| record.id)
                })
                .collect::<Vec<_>>();
            if !unfinished.is_empty() {
                return Err(RegistrationError::new(
                    "dependencies-not-done",
                    format!(
                        "direct prerequisite Records are not Done: {}",
                        unfinished
                            .iter()
                            .map(u64::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
            let record = records
                .iter_mut()
                .find(|record| record.id == id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            if record.status != RecordStatus::Open {
                return Err(RegistrationError::new(
                    "invalid-status-transition",
                    "invalid transition",
                ));
            }
            record.status = RecordStatus::InProgress;
            Ok(record.clone())
        }
        fn done_record(
            &self,
            id: u64,
            completion_resolution: Option<&str>,
        ) -> Result<Record, RegistrationError> {
            let mut records = self.records.lock().unwrap();
            let index = records
                .iter()
                .position(|record| record.id == id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            if records[index].status != RecordStatus::InProgress {
                return Err(RegistrationError::new(
                    "invalid-status-transition",
                    "invalid transition",
                ));
            }
            if records[index].relationships.iter().any(|relationship| {
                relationship.parent_id == id
                    && records
                        .iter()
                        .find(|record| record.id == relationship.child_id)
                        .is_some_and(|child| child.status != RecordStatus::Done)
            }) {
                return Err(RegistrationError::new(
                    "children-not-done",
                    "all child Records must be Done before completing a parent",
                ));
            }
            records[index].status = RecordStatus::Done;
            records[index].completion_resolution = completion_resolution.map(str::to_owned);
            Ok(records[index].clone())
        }
        #[allow(clippy::too_many_lines)]
        fn relate_records(
            &self,
            child_id: u64,
            parent_id: u64,
            kind: RecordRelationshipKind,
        ) -> Result<Record, RegistrationError> {
            if child_id == parent_id {
                return Err(RegistrationError::new(
                    "invalid-relationship",
                    "a Record cannot be its own parent",
                ));
            }
            let mut records = self.records.lock().unwrap();
            let child = records
                .iter()
                .position(|record| record.id == child_id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            let parent = records
                .iter()
                .position(|record| record.id == parent_id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            if kind == RecordRelationshipKind::DependsOn {
                if records[child].status != RecordStatus::Open {
                    return Err(RegistrationError::new(
                        "blocked-record-not-open",
                        "a dependency may change only while its blocked Record is Open",
                    ));
                }
                if records[child].record_type != records[parent].record_type
                    || !matches!(
                        records[child].record_type,
                        RecordType::Task | RecordType::Spec
                    )
                {
                    return Err(RegistrationError::new(
                        "invalid-relationship",
                        "depends-on requires same-type Task or Spec Records",
                    ));
                }
                let relationship = RecordRelationship {
                    kind,
                    from_id: child_id,
                    to_id: parent_id,
                    parent_id: 0,
                    child_id: 0,
                };
                records[child].relationships.push(relationship.clone());
                records[parent].relationships.push(relationship);
                return Ok(records[child].clone());
            }
            if records[child].record_type != RecordType::Generic
                || !matches!(
                    records[parent].record_type,
                    RecordType::Generic | RecordType::Task
                )
            {
                return Err(RegistrationError::new(
                    "invalid-relationship",
                    "only a Generic Record may be attached to a Generic or Task parent after opening",
                ));
            }
            if records[child].project_slugs != records[parent].project_slugs {
                return Err(RegistrationError::new(
                    "scope-mismatch",
                    "parent and child Records must have identical Project Scopes",
                ));
            }
            if records.iter().any(|record| {
                record
                    .relationships
                    .iter()
                    .any(|relationship| relationship.child_id == child_id)
            }) {
                return Err(RegistrationError::new(
                    "child-already-related",
                    "a child Record can have only one parent",
                ));
            }
            let mut pending = vec![child_id];
            while let Some(current) = pending.pop() {
                if current == parent_id {
                    return Err(RegistrationError::new(
                        "relationship-cycle",
                        "parent/child Relationships cannot form a cycle",
                    ));
                }
                pending.extend(
                    records
                        .iter()
                        .flat_map(|record| record.relationships.iter())
                        .filter(|relationship| relationship.parent_id == current)
                        .map(|relationship| relationship.child_id),
                );
            }
            let relationship = RecordRelationship {
                kind: RecordRelationshipKind::ParentChild,
                from_id: child_id,
                to_id: parent_id,
                parent_id,
                child_id,
            };
            records[child].relationships.push(relationship.clone());
            records[parent].relationships.push(relationship);
            Ok(records[child].clone())
        }
        fn unrelate_records(
            &self,
            child_id: u64,
            parent_id: u64,
            kind: RecordRelationshipKind,
        ) -> Result<Record, RegistrationError> {
            if kind != RecordRelationshipKind::ParentChild {
                return Err(RegistrationError::new(
                    "invalid-relationship",
                    "test repository supports parent/child only",
                ));
            }
            let mut records = self.records.lock().unwrap();
            let child = records
                .iter()
                .position(|record| record.id == child_id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            let parent = records
                .iter()
                .position(|record| record.id == parent_id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            if records[child].record_type != RecordType::Generic
                || records[parent].record_type != RecordType::Generic
            {
                return Err(RegistrationError::new(
                    "invalid-relationship",
                    "only Generic Record Relationships may be removed",
                ));
            }
            let exists = records[child].relationships.iter().any(|relationship| {
                relationship.parent_id == parent_id && relationship.child_id == child_id
            });
            if !exists {
                return Err(RegistrationError::new(
                    "relationship-not-found",
                    "parent/child Relationship does not exist",
                ));
            }
            for index in [child, parent] {
                records[index].relationships.retain(|relationship| {
                    relationship.parent_id != parent_id || relationship.child_id != child_id
                });
            }
            Ok(records[child].clone())
        }
        fn list_record_updates(&self, id: u64) -> Result<Vec<RecordUpdate>, RegistrationError> {
            self.show_record(id)?;
            Ok(vec![])
        }
        fn add_record_comment(
            &self,
            id: u64,
            body: &str,
            author: Option<&str>,
        ) -> Result<RecordComment, RegistrationError> {
            self.show_record(id)?;
            let mut comments = self.comments.lock().unwrap();
            let comment = RecordComment {
                id: RecordCommentId((comments.len() + 1) as u64),
                created_at_millis: 0,
                author: author.map(str::to_owned),
                body: body.to_owned(),
            };
            comments.push(comment.clone());
            Ok(comment)
        }
        fn list_record_comments(&self, id: u64) -> Result<Vec<RecordComment>, RegistrationError> {
            self.show_record(id)?;
            Ok(self
                .comments
                .lock()
                .unwrap()
                .iter()
                .cloned()
                .rev()
                .collect())
        }
        fn delete_record_comment(
            &self,
            id: u64,
            comment_id: RecordCommentId,
        ) -> Result<(), RegistrationError> {
            self.show_record(id)?;
            let mut comments = self.comments.lock().unwrap();
            let before = comments.len();
            comments.retain(|comment| comment.id != comment_id);
            if comments.len() == before {
                return Err(RegistrationError::new(
                    "comment-not-found",
                    "Comment does not exist",
                ));
            }
            Ok(())
        }
    }
    impl ManagedWorktreeRepository for Repository {
        fn worktree_path(&self, record_id: u64) -> Result<PathBuf, RegistrationError> {
            Ok(managed_worktree_path(
                Path::new("/"),
                &self.show_record(record_id)?.project_slugs[0],
                record_id,
            ))
        }

        fn ensure_worktree_removable(&self, _: u64) -> Result<(), RegistrationError> {
            Ok(())
        }

        fn set_project_setup_hook(
            &self,
            _: &str,
            _: Option<&str>,
        ) -> Result<(), RegistrationError> {
            Ok(())
        }

        fn project_setup_hook(&self, _: &str) -> Result<Option<String>, RegistrationError> {
            Ok(None)
        }

        fn reserve_worktree_claim(
            &self,
            record_id: u64,
        ) -> Result<WorktreeClaim, RegistrationError> {
            if let Some(claim) = self.worktree_claim(record_id)? {
                return Ok(claim);
            }
            let record = self.show_record(record_id)?;
            if !is_worktree_eligible(&record) {
                return Err(RegistrationError::new(
                    "worktree-ineligible-record",
                    "ineligible",
                ));
            }
            let claim = WorktreeClaim {
                record_id,
                project_slug: record.project_slugs[0].clone(),
                path: managed_worktree_path(Path::new("/"), &record.project_slugs[0], record_id),
                state: WorktreeClaimState::Reserving,
            };
            self.claims.lock().unwrap().push(claim.clone());
            Ok(claim)
        }

        fn activate_worktree_claim(
            &self,
            record_id: u64,
        ) -> Result<WorktreeClaim, RegistrationError> {
            let mut claims = self.claims.lock().unwrap();
            let claim = claims
                .iter_mut()
                .find(|claim| claim.record_id == record_id)
                .ok_or_else(|| RegistrationError::new("worktree-claim-not-found", "missing"))?;
            claim.state = WorktreeClaimState::Active;
            Ok(claim.clone())
        }

        fn require_worktree_claim_cleanup(
            &self,
            record_id: u64,
        ) -> Result<WorktreeClaim, RegistrationError> {
            let mut claims = self.claims.lock().unwrap();
            let claim = claims
                .iter_mut()
                .find(|claim| claim.record_id == record_id)
                .ok_or_else(|| RegistrationError::new("worktree-claim-not-found", "missing"))?;
            claim.state = WorktreeClaimState::CleanupRequired;
            Ok(claim.clone())
        }

        fn worktree_claim(
            &self,
            record_id: u64,
        ) -> Result<Option<WorktreeClaim>, RegistrationError> {
            Ok(self
                .claims
                .lock()
                .unwrap()
                .iter()
                .find(|claim| claim.record_id == record_id)
                .cloned())
        }

        fn worktree_claims(&self) -> Result<Vec<WorktreeClaim>, RegistrationError> {
            Ok(self.claims.lock().unwrap().clone())
        }

        fn worktree_claims_requiring_cleanup(
            &self,
        ) -> Result<Vec<WorktreeClaim>, RegistrationError> {
            Ok(self
                .claims
                .lock()
                .unwrap()
                .iter()
                .filter(|claim| claim.state == WorktreeClaimState::CleanupRequired)
                .cloned()
                .collect())
        }

        fn delete_worktree_claim(&self, record_id: u64) -> Result<(), RegistrationError> {
            self.claims
                .lock()
                .unwrap()
                .retain(|claim| claim.record_id != record_id);
            Ok(())
        }
    }

    struct Inspector;
    impl RepositoryInspector for Inspector {
        fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
            unreachable!()
        }

        fn head_commit(&self, _: &Path) -> Result<String, RegistrationError> {
            Ok("test-base".into())
        }
    }
    fn application() -> Application {
        Application::new(
            Box::new(Repository {
                records: Mutex::new(vec![]),
                comments: Mutex::new(vec![]),
                claims: Mutex::new(vec![]),
            }),
            Box::new(Inspector),
        )
    }

    struct TestWorktreeAdapter {
        calls: Mutex<Vec<(String, String)>>,
        create_fails: bool,
    }

    impl ManagedWorktreeAdapter for TestWorktreeAdapter {
        fn create(
            &self,
            _: &Path,
            _: &Path,
            branch: &str,
            base: &str,
        ) -> Result<(), RegistrationError> {
            assert_eq!(branch, "feat/forge-7");
            assert_eq!(base, "test-base");
            self.calls
                .lock()
                .unwrap()
                .push((branch.into(), base.into()));
            if self.create_fails {
                Err(RegistrationError::new(
                    "managed-worktree-invalid",
                    "branch collision",
                ))
            } else {
                Ok(())
            }
        }

        fn remove(&self, _: &Path, _: bool) -> Result<(), RegistrationError> {
            panic!("preparation must not remove retained checkout evidence")
        }

        fn assess(&self, _: &Path) -> WorktreeSafetyAssessment {
            WorktreeSafetyAssessment {
                facts: vec!["clean".into()],
                blockers: vec![],
                removable: true,
            }
        }

        fn run_setup_hook(
            &self,
            _: &Path,
            _: &str,
            _: &str,
            _: u64,
            _: &str,
            _: &str,
        ) -> Result<(), RegistrationError> {
            Ok(())
        }
    }

    fn task_repository() -> Repository {
        Repository {
            records: Mutex::new(vec![Record {
                workspace_id: "test".into(),
                id: 7,
                record_type: RecordType::Task,
                execution_mode: ExecutionMode::Dispatched,
                body: "task".into(),
                project_slugs: vec!["alpha".into()],
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![],
            }]),
            comments: Mutex::new(vec![]),
            claims: Mutex::new(vec![]),
        }
    }

    #[test]
    fn dispatcher_preparation_derives_and_retains_the_record_checkout() {
        let adapter = TestWorktreeAdapter {
            calls: Mutex::new(vec![]),
            create_fails: false,
        };
        let app = Application::with_managed_worktrees(
            Box::new(task_repository()),
            Box::new(task_repository()),
            Box::new(adapter),
            Box::new(Inspector),
        );

        let claim = app.prepare_dispatch_worktree(7).unwrap();
        assert_eq!(claim.state, WorktreeClaimState::Active);
        assert_eq!(claim.path, PathBuf::from("/.forge/worktrees/alpha/7"));
        assert_eq!(app.prepare_dispatch_worktree(7).unwrap(), claim);
    }

    #[test]
    fn failed_dispatcher_preparation_requires_recovery_without_retrying_git() {
        let app = Application::with_managed_worktrees(
            Box::new(task_repository()),
            Box::new(task_repository()),
            Box::new(TestWorktreeAdapter {
                calls: Mutex::new(vec![]),
                create_fails: true,
            }),
            Box::new(Inspector),
        );

        assert_eq!(
            app.prepare_dispatch_worktree(7).unwrap_err().code,
            "managed-worktree-invalid"
        );
        assert_eq!(
            app.prepare_dispatch_worktree(7).unwrap_err().code,
            "claim-requires-recovery"
        );
    }

    #[test]
    fn worktree_eligibility_requires_an_open_single_project_task() {
        let mut record = Record {
            workspace_id: "test".into(),
            id: 7,
            record_type: RecordType::Task,
            execution_mode: ExecutionMode::Dispatched,
            body: "task".into(),
            project_slugs: vec!["alpha".into()],
            status: RecordStatus::Open,
            completion_resolution: None,
            relationships: vec![],
        };
        assert!(is_worktree_eligible(&record));
        assert_eq!(
            managed_worktree_path(Path::new("/"), "alpha", record.id),
            PathBuf::from("/.forge/worktrees/alpha/7")
        );
        record.status = RecordStatus::Done;
        assert!(!is_worktree_eligible(&record));
        record.status = RecordStatus::Open;
        record.project_slugs.push("beta".into());
        assert!(!is_worktree_eligible(&record));
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
    fn typed_records_have_immutable_opening_rules_and_hierarchy() {
        let app = application();
        let spec = app.open_spec("spec", "alpha".into()).unwrap();
        assert_eq!(spec.record_type, RecordType::Spec);
        assert_eq!(spec.project_slugs, ["alpha"]);

        let wayfinder = app
            .open_wayfinder("wayfinder", vec!["alpha".into()])
            .unwrap();
        let wayfinder_task = app
            .open_task_under_wayfinder("wayfinder child", wayfinder.id, None, vec![])
            .unwrap();
        assert_eq!(wayfinder_task.project_slugs, ["alpha"]);
        let multi_wayfinder = app
            .open_wayfinder("multi", vec!["beta".into(), "alpha".into()])
            .unwrap();
        assert_eq!(multi_wayfinder.project_slugs, ["alpha", "beta"]);
        assert_eq!(
            app.open_task_under_wayfinder("missing", multi_wayfinder.id, None, vec![])
                .unwrap_err()
                .code,
            "missing-project"
        );
        assert_eq!(
            app.open_task_under_wayfinder(
                "outside",
                multi_wayfinder.id,
                Some("gamma".into()),
                vec![]
            )
            .unwrap_err()
            .code,
            "scope-mismatch"
        );

        let standalone = app.open_task("standalone", None).unwrap();
        assert_eq!(standalone.record_type, RecordType::Task);
        assert!(standalone.project_slugs.is_empty());
        let task = app.open_task_under_spec("child", spec.id, vec![]).unwrap();
        assert_eq!(task.record_type, RecordType::Task);
        assert_eq!(task.project_slugs, ["alpha"]);
        assert_eq!(task.relationships[0].parent_id, spec.id);
        assert_eq!(
            app.open_task_under_spec("invalid", standalone.id, vec![])
                .unwrap_err()
                .code,
            "invalid-parent-type"
        );
        assert_eq!(
            app.relate_records(task.id, spec.id, RecordRelationshipKind::ParentChild)
                .unwrap_err()
                .code,
            "invalid-relationship"
        );
        assert_eq!(
            app.unrelate_records(task.id, spec.id, RecordRelationshipKind::ParentChild)
                .unwrap_err()
                .code,
            "invalid-relationship"
        );

        app.start_record(spec.id).unwrap();
        assert_eq!(
            app.done_record(spec.id, None).unwrap_err().code,
            "children-not-done"
        );
    }

    #[test]
    fn application_rejects_starts_with_unfinished_direct_prerequisites() {
        let app = application();
        let blocked = app.open_task("blocked", None).unwrap();
        let prerequisite = app.open_task("prerequisite", None).unwrap();
        app.relate_records(
            blocked.id,
            prerequisite.id,
            RecordRelationshipKind::DependsOn,
        )
        .unwrap();

        assert_eq!(
            app.start_record(blocked.id).unwrap_err().code,
            "dependencies-not-done"
        );
        app.start_record(prerequisite.id).unwrap();
        app.done_record(prerequisite.id, None).unwrap();
        assert_eq!(
            app.start_record(blocked.id).unwrap().status,
            RecordStatus::InProgress
        );
    }

    #[test]
    fn lists_records_by_scope_membership_and_status_newest_first() {
        let app = application();
        let alpha = app.open_record("alpha", vec!["alpha".into()]).unwrap();
        let beta = app
            .open_record("both", vec!["alpha".into(), "beta".into()])
            .unwrap();
        let unscoped = app.open_record("unscoped", vec![]).unwrap();
        app.start_record(beta.id).unwrap();

        assert_eq!(
            app.list_records(Some("alpha".into()), None, None)
                .unwrap()
                .into_iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec![beta.id, alpha.id]
        );
        assert_eq!(
            app.list_records(None, Some(RecordStatus::Open), None)
                .unwrap()
                .into_iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec![unscoped.id, alpha.id]
        );
    }

    #[test]
    fn record_content_and_lifecycle_advance_once_and_completion_is_immutable() {
        let app = application();
        let record = app.open_record("original", vec![]).unwrap();
        assert_eq!(
            app.edit_record(record.id, "revised").unwrap().body,
            "revised"
        );
        assert_eq!(
            app.done_record(record.id, None).unwrap_err().code,
            "invalid-status-transition"
        );
        assert_eq!(
            app.start_record(record.id).unwrap().status,
            RecordStatus::InProgress
        );
        let done = app.done_record(record.id, Some("completed")).unwrap();
        assert_eq!(done.status, RecordStatus::Done);
        assert_eq!(done.completion_resolution.as_deref(), Some("completed"));
        assert_eq!(
            app.edit_record(record.id, "later").unwrap_err().code,
            "invalid-status-transition"
        );
        assert_eq!(
            app.done_record(record.id, Some("other")).unwrap_err().code,
            "invalid-status-transition"
        );
        assert_eq!(
            app.done_record(record.id, Some(" ")).unwrap_err().code,
            "invalid-resolution"
        );
    }

    #[test]
    fn relationships_are_nested_and_gate_parent_completion() {
        let app = application();
        let parent = app.open_record("parent", vec![]).unwrap();
        let child = app.open_record("child", vec![]).unwrap();
        let grandchild = app.open_record("grandchild", vec![]).unwrap();
        assert_eq!(
            app.relate_records(child.id, parent.id, RecordRelationshipKind::ParentChild)
                .unwrap()
                .relationships
                .len(),
            1
        );
        app.relate_records(grandchild.id, child.id, RecordRelationshipKind::ParentChild)
            .unwrap();
        assert_eq!(
            app.relate_records(
                parent.id,
                grandchild.id,
                RecordRelationshipKind::ParentChild
            )
            .unwrap_err()
            .code,
            "relationship-cycle"
        );
        app.start_record(parent.id).unwrap();
        assert_eq!(
            app.done_record(parent.id, None).unwrap_err().code,
            "children-not-done"
        );
        for record_id in [grandchild.id, child.id] {
            app.start_record(record_id).unwrap();
            app.done_record(record_id, Some("complete")).unwrap();
        }
        assert_eq!(
            app.done_record(parent.id, None).unwrap().status,
            RecordStatus::Done
        );
        assert_eq!(
            app.unrelate_records(child.id, parent.id, RecordRelationshipKind::ParentChild)
                .unwrap()
                .relationships
                .len(),
            1
        );
    }

    #[test]
    fn comments_require_meaningful_content_and_are_immutable() {
        let app = application();
        let record = app.open_record("record", vec![]).unwrap();
        assert_eq!(
            app.add_record_comment(record.id, " ", None)
                .unwrap_err()
                .code,
            "invalid-comment"
        );
        assert_eq!(
            app.add_record_comment(record.id, "text", Some(" "))
                .unwrap_err()
                .code,
            "invalid-comment-author"
        );
        let comment = app
            .add_record_comment(record.id, "text", Some("agent"))
            .unwrap();
        assert_eq!(
            app.list_record_comments(record.id).unwrap(),
            vec![comment.clone()]
        );
        app.delete_record_comment(record.id, comment.id.0).unwrap();
        assert!(app.list_record_comments(record.id).unwrap().is_empty());
    }

    #[test]
    fn opening_assigns_open_status_and_sorts_scope() {
        assert_eq!(
            application()
                .open_record("body", vec!["zeta".into(), "alpha".into()])
                .unwrap(),
            Record {
                workspace_id: "test".into(),
                id: 1,
                record_type: RecordType::Generic,
                execution_mode: ExecutionMode::Manual,
                body: "body".into(),
                project_slugs: vec!["alpha".into(), "zeta".into()],
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![],
            }
        );
    }
}
