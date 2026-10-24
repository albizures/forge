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
    InProgress,
    Done,
}

/// The currently supported kind of typed Record Relationship.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordRelationshipKind {
    ParentChild,
    DependsOn,
    GeneratedBy,
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
}

/// Optional immutable metadata that classifies a Task without changing its workflow semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskKind {
    Research,
    Prototype,
    Review,
}

/// A unit of work whose immutable type governs its scope and hierarchy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Opaque, positive identity assigned by persistence.
    pub id: u64,
    /// Immutable type selected at opening.
    pub record_type: RecordType,
    /// Optional immutable metadata, populated only for Tasks.
    pub task_kind: Option<TaskKind>,
    /// Required Markdown content, separate from its structured metadata.
    pub body: String,
    /// Immutable, lexically ordered Project Scope selected at opening.
    pub project_slugs: Vec<String>,
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
    pub task_kind: Option<TaskKind>,
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
    pub task_kind: Option<TaskKind>,
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

/// A Record Update in Forge's global cursor order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordUpdateEnvelope {
    pub record_id: u64,
    pub update: RecordUpdate,
}

/// Opaque stable identity assigned to a durable Record Update Subscription.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RecordUpdateSubscriptionId(pub u64);

/// The update categories a subscription may select.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RecordUpdateKindFilter {
    Created,
    BodyEdited,
    StatusTransition,
    RelationshipAdded,
    RelationshipRemoved,
    CommentAdded,
    CommentDeleted,
}

/// A durable, leased request for updates to explicit Records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordUpdateSubscription {
    pub id: RecordUpdateSubscriptionId,
    pub assignment_id: String,
    /// Sorted, duplicate-free explicit Record IDs.
    pub target_record_ids: Vec<u64>,
    /// Sorted, duplicate-free categories; empty selects every category.
    pub kind_filters: Vec<RecordUpdateKindFilter>,
    /// The greatest committed global update cursor at creation.
    pub start_cursor: RecordUpdateId,
    /// Unix milliseconds at which the lease becomes invalid.
    pub lease_expires_at_millis: u64,
}

/// Persistence input for a durable Record Update Subscription.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewRecordUpdateSubscription {
    pub assignment_id: String,
    pub target_record_ids: Vec<u64>,
    pub kind_filters: Vec<RecordUpdateKindFilter>,
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
}

/// Persistence boundary owned by the application.
#[allow(clippy::missing_errors_doc)]
pub trait ProjectRepository: Send + Sync {
    /// Returns the Project registered under `slug`.
    fn project(&self, slug: &str) -> Result<Project, RegistrationError>;
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

/// Returns whether a Record may own a Managed Worktree.
#[must_use]
pub fn is_worktree_eligible(record: &Record) -> bool {
    record.record_type == RecordType::Task
        && record.status == RecordStatus::InProgress
        && record.project_slugs.len() == 1
}

/// Derives the only path Forge may use for a Record's Managed Worktree.
#[must_use]
pub fn managed_worktree_path(root: &Path, project_slug: &str, record_id: u64) -> PathBuf {
    root.join(project_slug).join(record_id.to_string())
}

/// Persistence boundary for managed-worktree configuration and claims.
#[allow(clippy::missing_errors_doc)]
pub trait ManagedWorktreeRepository: RecordRepository {
    /// Sets or clears the absolute user-wide Worktree Root.
    fn set_worktree_root(&self, root: Option<&Path>) -> Result<(), RegistrationError>;
    /// Returns the configured user-wide Worktree Root.
    fn worktree_root(&self) -> Result<Option<PathBuf>, RegistrationError>;
    /// Sets or clears a Project Setup Hook.
    fn set_project_setup_hook(
        &self,
        project_slug: &str,
        hook: Option<&str>,
    ) -> Result<(), RegistrationError>;
    /// Returns a Project's Setup Hook.
    fn project_setup_hook(&self, project_slug: &str) -> Result<Option<String>, RegistrationError>;
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
    /// Opens a Task beneath a Spec, assigning the Spec's scope and membership atomically.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the parent is not a Spec or storage fails.
    fn open_task_under_spec(
        &self,
        body: &str,
        spec_id: u64,
        task_kind: Option<TaskKind>,
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
        task_kind: Option<TaskKind>,
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
    pub unknown_state_grace_millis: u64,
}

/// A durable attempt to dispatch one Task to an external Agent.
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
    pub registered_project_folder: PathBuf,
    pub rendered_initial_prompt: String,
    /// Capability delivery is intentionally opaque to the provider. A later capability issuer
    /// supplies this value without exposing its secret in assignment persistence.
    pub capability_delivery: Option<AssignmentCapabilityDelivery>,
    pub resources: Vec<AgentAssignmentResource>,
}

/// Opaque launch input through which an Assignment Capability is delivered to an Agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignmentCapabilityDelivery {
    pub endpoint: String,
    pub secret_file: PathBuf,
}

/// Port implemented by a concrete agent provider; it deliberately exposes no provider semantics.
#[allow(clippy::missing_errors_doc)]
pub trait AgentDispatchAdapter: Send + Sync {
    fn probe(&self) -> Result<(), AgentAdapterError>;
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
    fn adapter_for(
        &self,
        profile: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError>;
}

/// The persisted result of reconciling one already-open assignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssignmentReconciliation {
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
    let observation = match observed {
        Ok(observation) => observation,
        Err(error) => {
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
/// across daemon restarts. Retry exhaustion leaves the Task in progress and records one Comment.
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
        if assignment.next_retry_at_millis > now_millis {
            continue;
        }
        if assignment.state == AgentAssignmentState::Unknown
            && now_millis.saturating_sub(assignment.state_changed_at_millis)
                < assignment.profile.unknown_state_grace_millis
        {
            continue;
        }
        let adapter = match factory.adapter_for(&assignment.profile) {
            Ok(adapter) => adapter,
            Err(error) => {
                let retry_count = assignment.retry_count.saturating_add(1);
                let next_retry = now_millis.saturating_add(assignment.profile.retry_backoff_millis);
                if repository.schedule_agent_assignment_retry(
                    &assignment.id,
                    assignment.state,
                    next_retry,
                    retry_count,
                )? && retry_count == assignment.profile.retry_limit.saturating_add(1)
                {
                    repository.add_record_comment(
                        assignment.record_id,
                        "Agent dispatch reconciliation retries exhausted; Task remains In Progress for human recovery.",
                        Some("agent-dispatch"),
                    )?;
                }
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
            if repository.schedule_agent_assignment_retry(
                &assignment.id,
                assignment.state,
                next_retry,
                retry_count,
            )? && retry_count == assignment.profile.retry_limit.saturating_add(1)
            {
                repository.add_record_comment(
                    assignment.record_id,
                    "Agent dispatch reconciliation retries exhausted; Task remains In Progress for human recovery.",
                    Some("agent-dispatch"),
                )?;
            }
        }
        reconciled.push(outcome);
    }
    Ok(AgentDispatchScan {
        acquired_lease: true,
        reconciled,
    })
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
        Err(error) => Ok(AssignmentReconciliation::Retained(retention_from_error(
            &error,
        ))),
    }
}

fn agent_dispatch_context(
    repository: &dyn AgentDispatchRepository,
    assignment: &AgentAssignment,
) -> Result<AgentDispatchContext, RegistrationError> {
    let project = repository.project(&assignment.project_slug)?;
    let record = repository.show_record(assignment.record_id)?;
    Ok(AgentDispatchContext {
        assignment: assignment.clone(),
        registered_project_folder: project.registered_folder,
        rendered_initial_prompt: format!(
            "{}\n\n{}",
            assignment.profile.prompt_template, record.body
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
    CreateSubscription,
    RenewSubscription,
    DeleteSubscription,
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

/// Persistence boundary for durable Record Update Subscription state and replay.
#[allow(clippy::missing_errors_doc)]
pub trait RecordUpdateSubscriptionRepository: AgentDispatchRepository {
    /// Creates an idempotent 30-minute leased Subscription and captures its start cursor.
    fn create_record_update_subscription(
        &self,
        subscription: &NewRecordUpdateSubscription,
    ) -> Result<RecordUpdateSubscription, RegistrationError>;
    /// Returns a Subscription while its lease remains valid.
    fn record_update_subscription(
        &self,
        id: RecordUpdateSubscriptionId,
    ) -> Result<RecordUpdateSubscription, RegistrationError>;
    /// Renews a Subscription's 30-minute lease.
    fn renew_record_update_subscription(
        &self,
        id: RecordUpdateSubscriptionId,
    ) -> Result<RecordUpdateSubscription, RegistrationError>;
    /// Deletes a Subscription and all of its selections.
    fn delete_record_update_subscription(
        &self,
        id: RecordUpdateSubscriptionId,
    ) -> Result<(), RegistrationError>;
    /// Returns the greatest committed global Record Update cursor, or zero before any updates.
    fn record_update_head(&self) -> Result<RecordUpdateId, RegistrationError>;
    /// Replays selected global updates strictly after `after`, ascending by cursor.
    fn replay_record_updates(
        &self,
        id: RecordUpdateSubscriptionId,
        after: RecordUpdateId,
    ) -> Result<Vec<RecordUpdateEnvelope>, RegistrationError>;
}

/// Durable persistence boundary for provider-neutral dispatch foundations.
#[allow(clippy::missing_errors_doc)]
pub trait AgentDispatchRepository: RecordRepository {
    /// Returns an immutable seeded Agent Profile revision.
    fn agent_profile(
        &self,
        record_type: RecordType,
        revision: u32,
    ) -> Result<AgentProfile, RegistrationError>;
    /// Creates an Assignment with immutable profile provenance.
    fn create_agent_assignment(
        &self,
        assignment: &NewAgentAssignment,
    ) -> Result<AgentAssignment, RegistrationError>;
    /// Returns one Assignment by opaque identifier.
    fn agent_assignment(&self, assignment_id: &str) -> Result<AgentAssignment, RegistrationError>;
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
    ) -> Result<Option<AgentAssignment>, RegistrationError>;
    /// Conditionally persists a normalized observation without letting a stale observer win.
    fn compare_and_set_agent_assignment_state(
        &self,
        assignment_id: &str,
        expected: AgentAssignmentState,
        next: AgentAssignmentState,
    ) -> Result<Option<AgentAssignment>, RegistrationError>;
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
    /// Returns retained resource history for an Assignment.
    fn list_agent_assignment_resources(
        &self,
        assignment_id: &str,
    ) -> Result<Vec<AgentAssignmentResource>, RegistrationError>;
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
pub fn assignment_add_comment<R: AgentDispatchRepository>(
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

/// Authorizes capability-authenticated completion of its bound Task and records its decision.
#[allow(clippy::missing_errors_doc)]
pub fn assignment_complete_task<R: AgentDispatchRepository>(
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

/// Authorizes creation of a Subscription for explicit direct prerequisites of the assigned Record.
#[allow(clippy::missing_errors_doc)]
pub fn assignment_create_record_update_subscription<
    R: RecordUpdateSubscriptionRepository + ?Sized,
>(
    repository: &R,
    assignment_id: &str,
    capability_id: &str,
    correlation_id: &str,
    target_record_ids: Vec<u64>,
    kind_filters: Vec<RecordUpdateKindFilter>,
) -> Result<RecordUpdateSubscription, RegistrationError> {
    let assignment = repository.agent_assignment(assignment_id)?;
    let allowed = assignment.closed_at_millis.is_none()
        && !target_record_ids.is_empty()
        && target_record_ids.iter().all(|target_id| {
            repository.show_record(*target_id).is_ok_and(|target| {
                target.relationships.iter().any(|relationship| {
                    relationship.kind == RecordRelationshipKind::DependsOn
                        && relationship.from_id == assignment.record_id
                        && relationship.to_id == *target_id
                })
            })
        });
    authorize_assignment_operation(
        repository,
        &assignment,
        capability_id,
        correlation_id,
        AssignmentAuthorizationOperation::CreateSubscription,
        assignment.record_id,
        allowed,
    )?;
    repository.create_record_update_subscription(&NewRecordUpdateSubscription {
        assignment_id: assignment.id,
        target_record_ids,
        kind_filters,
    })
}

/// Authorizes renewal only for the named open Assignment's Subscription.
#[allow(clippy::missing_errors_doc)]
pub fn assignment_renew_record_update_subscription<
    R: RecordUpdateSubscriptionRepository + ?Sized,
>(
    repository: &R,
    assignment_id: &str,
    capability_id: &str,
    correlation_id: &str,
    subscription_id: RecordUpdateSubscriptionId,
) -> Result<RecordUpdateSubscription, RegistrationError> {
    let assignment = repository.agent_assignment(assignment_id)?;
    let subscription = repository.record_update_subscription(subscription_id)?;
    let allowed =
        assignment.closed_at_millis.is_none() && subscription.assignment_id == assignment.id;
    authorize_assignment_operation(
        repository,
        &assignment,
        capability_id,
        correlation_id,
        AssignmentAuthorizationOperation::RenewSubscription,
        assignment.record_id,
        allowed,
    )?;
    repository.renew_record_update_subscription(subscription_id)
}

/// Authorizes deletion only for the named open Assignment's Subscription.
#[allow(clippy::missing_errors_doc)]
pub fn assignment_delete_record_update_subscription<
    R: RecordUpdateSubscriptionRepository + ?Sized,
>(
    repository: &R,
    assignment_id: &str,
    capability_id: &str,
    correlation_id: &str,
    subscription_id: RecordUpdateSubscriptionId,
) -> Result<(), RegistrationError> {
    let assignment = repository.agent_assignment(assignment_id)?;
    let subscription = repository.record_update_subscription(subscription_id)?;
    let allowed =
        assignment.closed_at_millis.is_none() && subscription.assignment_id == assignment.id;
    authorize_assignment_operation(
        repository,
        &assignment,
        capability_id,
        correlation_id,
        AssignmentAuthorizationOperation::DeleteSubscription,
        assignment.record_id,
        allowed,
    )?;
    repository.delete_record_update_subscription(subscription_id)
}

fn authorize_assignment_operation<R: AgentDispatchRepository + ?Sized>(
    repository: &R,
    assignment: &AgentAssignment,
    capability_id: &str,
    correlation_id: &str,
    operation: AssignmentAuthorizationOperation,
    target_record_id: u64,
    allowed: bool,
) -> Result<(), RegistrationError> {
    repository.add_assignment_authorization_audit(&AssignmentAuthorizationAudit {
        assignment_id: assignment.id.clone(),
        capability_id: capability_id.into(),
        created_at_millis: 0,
        operation,
        target_record_id,
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
        correlation_id: correlation_id.into(),
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
    records: Box<dyn RecordRepository>,
    managed_worktrees: Option<Box<dyn ManagedWorktreeRepository>>,
    worktree_adapter: Option<Box<dyn ManagedWorktreeAdapter>>,
    dispatch: Option<Box<dyn RecordUpdateSubscriptionRepository>>,
    adapter_factory: Option<Box<dyn AgentDispatchAdapterFactory>>,
    inspector: Box<dyn RepositoryInspector>,
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

/// Result of requesting a single daemon-owned reconciliation pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentDispatchReconcileResult {
    pub acquired_lease: bool,
    pub reconciled_assignments: usize,
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
            managed_worktrees: None,
            worktree_adapter: None,
            dispatch: None,
            adapter_factory: None,
            inspector,
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
            managed_worktrees: Some(managed_worktrees),
            worktree_adapter: Some(worktree_adapter),
            dispatch: None,
            adapter_factory: None,
            inspector,
        }
    }

    /// Creates an application with the durable dispatch boundary enabled.
    #[must_use]
    pub fn with_agent_dispatch(
        records: Box<dyn RecordRepository>,
        dispatch: Box<dyn RecordUpdateSubscriptionRepository>,
        adapter_factory: Box<dyn AgentDispatchAdapterFactory>,
        inspector: Box<dyn RepositoryInspector>,
    ) -> Self {
        Self {
            records,
            managed_worktrees: None,
            worktree_adapter: None,
            dispatch: Some(dispatch),
            adapter_factory: Some(adapter_factory),
            inspector,
        }
    }

    /// Creates an application with both managed Worktrees and Agent Dispatch enabled.
    #[must_use]
    pub fn with_managed_worktrees_and_agent_dispatch(
        records: Box<dyn RecordRepository>,
        managed_worktrees: Box<dyn ManagedWorktreeRepository>,
        worktree_adapter: Box<dyn ManagedWorktreeAdapter>,
        dispatch: Box<dyn RecordUpdateSubscriptionRepository>,
        adapter_factory: Box<dyn AgentDispatchAdapterFactory>,
        inspector: Box<dyn RepositoryInspector>,
    ) -> Self {
        Self {
            records,
            managed_worktrees: Some(managed_worktrees),
            worktree_adapter: Some(worktree_adapter),
            dispatch: Some(dispatch),
            adapter_factory: Some(adapter_factory),
            inspector,
        }
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

    fn dispatch(&self) -> Result<&dyn RecordUpdateSubscriptionRepository, RegistrationError> {
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
        self.dispatch()?.set_herdr_socket_path(path)
    }

    /// Returns the seeded immutable Task Agent Profile.
    ///
    /// # Errors
    ///
    /// Returns configuration or persistence failures.
    pub fn task_agent_profile(&self) -> Result<AgentProfile, RegistrationError> {
        self.dispatch()?.agent_profile(RecordType::Task, 1)
    }

    /// Creates an Assignment-authorized durable Record Update Subscription.
    ///
    /// # Errors
    ///
    /// Returns authorization, validation, or persistence failures.
    pub fn create_assignment_subscription(
        &self,
        assignment_id: &str,
        capability_id: &str,
        correlation_id: &str,
        target_record_ids: Vec<u64>,
        kind_filters: Vec<RecordUpdateKindFilter>,
    ) -> Result<RecordUpdateSubscription, RegistrationError> {
        assignment_create_record_update_subscription(
            self.dispatch()?,
            assignment_id,
            capability_id,
            correlation_id,
            target_record_ids,
            kind_filters,
        )
    }

    /// Renews an Assignment-owned durable Record Update Subscription.
    ///
    /// # Errors
    ///
    /// Returns authorization, expiry, or persistence failures.
    pub fn renew_assignment_subscription(
        &self,
        assignment_id: &str,
        capability_id: &str,
        correlation_id: &str,
        subscription_id: RecordUpdateSubscriptionId,
    ) -> Result<RecordUpdateSubscription, RegistrationError> {
        assignment_renew_record_update_subscription(
            self.dispatch()?,
            assignment_id,
            capability_id,
            correlation_id,
            subscription_id,
        )
    }

    /// Deletes an Assignment-owned durable Record Update Subscription.
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence failures.
    pub fn delete_assignment_subscription(
        &self,
        assignment_id: &str,
        capability_id: &str,
        correlation_id: &str,
        subscription_id: RecordUpdateSubscriptionId,
    ) -> Result<(), RegistrationError> {
        assignment_delete_record_update_subscription(
            self.dispatch()?,
            assignment_id,
            capability_id,
            correlation_id,
            subscription_id,
        )
    }

    /// Validates delivery ownership without renewing the Subscription lease.
    ///
    /// # Errors
    ///
    /// Returns authorization, ownership, or lease failures.
    pub fn assignment_subscription_delivery(
        &self,
        assignment_id: &str,
        subscription_id: RecordUpdateSubscriptionId,
    ) -> Result<RecordUpdateSubscription, RegistrationError> {
        let dispatch = self.dispatch()?;
        let assignment = dispatch.agent_assignment(assignment_id)?;
        let subscription = dispatch.record_update_subscription(subscription_id)?;
        if assignment.closed_at_millis.is_none() && subscription.assignment_id == assignment.id {
            Ok(subscription)
        } else {
            Err(RegistrationError::new(
                "assignment-subscription-mismatch",
                "Subscription is not owned by the named open Assignment",
            ))
        }
    }

    /// Returns the greatest committed global Record Update cursor.
    ///
    /// # Errors
    ///
    /// Returns a dispatch storage error.
    pub fn record_update_head(&self) -> Result<RecordUpdateId, RegistrationError> {
        self.dispatch()?.record_update_head()
    }

    /// Replays selected committed updates after a durable global cursor.
    ///
    /// # Errors
    ///
    /// Returns storage, lease, or cursor failures.
    pub fn replay_assignment_subscription_updates(
        &self,
        subscription_id: RecordUpdateSubscriptionId,
        after: RecordUpdateId,
    ) -> Result<Vec<RecordUpdateEnvelope>, RegistrationError> {
        self.dispatch()?
            .replay_record_updates(subscription_id, after)
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
                "configured; Herdr adapter will probe on reconciliation".into()
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
        )?;
        Ok(AgentDispatchReconcileResult {
            acquired_lease: scan.acquired_lease,
            reconciled_assignments: scan.reconciled.len(),
        })
    }

    /// Creates and activates a Managed Worktree for an In Progress Task.
    ///
    /// # Errors
    ///
    /// Returns a stable lifecycle, hook, or Git failure.
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
                "task-not-in-progress",
                "a Managed Worktree requires an In Progress Task scoped to exactly one Project",
            ));
        }
        if managed.worktree_claim(record_id)?.is_some() {
            return Err(RegistrationError::new(
                "claim-exists",
                "Task already has a Managed Worktree Claim",
            ));
        }
        let claim = managed.reserve_worktree_claim(record_id)?;
        let project = match managed.project(&claim.project_slug) {
            Ok(project) => project,
            Err(error) => return self.rollback_worktree_claim(&claim, error),
        };
        if let Err(error) = adapter.create(&project.registered_folder, &claim.path, branch, base) {
            return self.rollback_worktree_claim(&claim, error);
        }
        if let Some(hook) = managed.project_setup_hook(&claim.project_slug)?
            && adapter
                .run_setup_hook(
                    &claim.path,
                    &hook,
                    &claim.project_slug,
                    record_id,
                    branch,
                    base,
                )
                .is_err()
        {
            let error = RegistrationError::new("setup-hook-failed", "Project Setup Hook failed");
            return self.rollback_checkout_and_claim(&claim, error);
        }
        managed.activate_worktree_claim(record_id)
    }

    fn rollback_worktree_claim(
        &self,
        claim: &WorktreeClaim,
        error: RegistrationError,
    ) -> Result<WorktreeClaim, RegistrationError> {
        if self
            .managed_worktrees()?
            .delete_worktree_claim(claim.record_id)
            .is_err()
        {
            self.managed_worktrees()?
                .require_worktree_claim_cleanup(claim.record_id)?;
        }
        Err(error)
    }

    fn rollback_checkout_and_claim(
        &self,
        claim: &WorktreeClaim,
        error: RegistrationError,
    ) -> Result<WorktreeClaim, RegistrationError> {
        if self.worktree_adapter()?.remove(&claim.path, true).is_ok()
            && self
                .managed_worktrees()?
                .delete_worktree_claim(claim.record_id)
                .is_ok()
        {
            return Err(error);
        }
        self.managed_worktrees()?
            .require_worktree_claim_cleanup(claim.record_id)?;
        Err(error)
    }

    /// Configures the absolute user-wide Worktree Root.
    ///
    /// # Errors
    ///
    /// Returns persistence or path-validation failures.
    pub fn set_worktree_root(&self, root: Option<&Path>) -> Result<(), RegistrationError> {
        self.managed_worktrees()?.set_worktree_root(root)
    }

    /// Returns the configured user-wide Worktree Root.
    ///
    /// # Errors
    ///
    /// Returns persistence failures.
    pub fn worktree_root(&self) -> Result<Option<PathBuf>, RegistrationError> {
        self.managed_worktrees()?.worktree_root()
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
        let managed = self.managed_worktrees()?;
        let claims = if include_all {
            managed.worktree_claims()?
        } else {
            managed.worktree_claims_requiring_cleanup()?
        };
        let adapter = self.worktree_adapter()?;
        Ok(claims
            .into_iter()
            .map(|claim| ManagedWorktreeInfo {
                safety: adapter.assess(&claim.path),
                claim,
            })
            .collect())
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
        Ok(ManagedWorktreeInfo {
            safety: self.worktree_adapter()?.assess(&claim.path),
            claim,
        })
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
        let info = self.managed_worktree_info(record_id)?;
        if let Err(error) = self.worktree_adapter()?.remove(&info.claim.path, true) {
            self.managed_worktrees()?
                .require_worktree_claim_cleanup(record_id)?;
            return Err(error);
        }
        self.managed_worktrees()?.delete_worktree_claim(record_id)
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
        self.open_typed_record(RecordType::Spec, body, vec![project_slug], None)
    }

    /// Opens a Wayfinder with a non-empty immutable Project Scope.
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
        self.open_typed_record(RecordType::Wayfinder, body, project_slugs, None)
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
        task_kind: Option<TaskKind>,
    ) -> Result<Record, RegistrationError> {
        self.open_typed_record(
            RecordType::Task,
            body,
            project_slug.into_iter().collect(),
            task_kind,
        )
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
        task_kind: Option<TaskKind>,
    ) -> Result<Record, RegistrationError> {
        Self::require_body(body)?;
        self.records.open_task_under_spec(body, spec_id, task_kind)
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
        task_kind: Option<TaskKind>,
    ) -> Result<Record, RegistrationError> {
        Self::require_body(body)?;
        self.records
            .open_task_under_wayfinder(body, wayfinder_id, project_slug, task_kind)
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
    ) -> Result<Record, RegistrationError> {
        Self::require_body(body)?;
        self.records
            .open_grilling_under_wayfinder(body, wayfinder_id, project_slugs)
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
        task_kind: Option<TaskKind>,
    ) -> Result<Vec<Record>, RegistrationError> {
        self.records.list_records(&RecordListFilter {
            project_slug,
            status,
            record_type,
            task_kind,
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
        task_kind: Option<TaskKind>,
    ) -> Result<Record, RegistrationError> {
        self.open_typed_record_with_generators(record_type, body, project_slugs, task_kind, vec![])
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
        task_kind: Option<TaskKind>,
        generated_by_ids: Vec<u64>,
    ) -> Result<Record, RegistrationError> {
        Self::require_body(body)?;
        self.records.open_typed_record(&NewTypedRecord {
            record_type,
            task_kind,
            body: body.to_owned(),
            project_slugs,
            generated_by_ids,
        })
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

    struct Repository {
        records: Mutex<Vec<Record>>,
        comments: Mutex<Vec<RecordComment>>,
    }
    impl ProjectRepository for Repository {
        fn project(&self, _: &str) -> Result<Project, RegistrationError> {
            unreachable!()
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
                id: (records.len() + 1) as u64,
                record_type: RecordType::Generic,
                task_kind: None,
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
                id: (records.len() + 1) as u64,
                record_type: record.record_type,
                task_kind: record.task_kind,
                body: record.body.clone(),
                project_slugs: record.project_slugs.clone(),
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![],
            };
            records.push(record.clone());
            Ok(record)
        }
        fn open_task_under_spec(
            &self,
            body: &str,
            spec_id: u64,
            task_kind: Option<TaskKind>,
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
                task_kind,
                body: body.to_owned(),
                project_slugs: spec.project_slugs,
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
            task_kind: Option<TaskKind>,
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
                task_kind,
                body: body.to_owned(),
                project_slugs: vec![project_slug],
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
                id: grilling_id,
                record_type: RecordType::Grilling,
                task_kind: None,
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
                comments: Mutex::new(vec![]),
            }),
            Box::new(Inspector),
        )
    }

    #[test]
    fn worktree_eligibility_requires_an_in_progress_single_project_task() {
        let mut record = Record {
            id: 7,
            record_type: RecordType::Task,
            task_kind: None,
            body: "task".into(),
            project_slugs: vec!["alpha".into()],
            status: RecordStatus::InProgress,
            completion_resolution: None,
            relationships: vec![],
        };
        assert!(is_worktree_eligible(&record));
        assert_eq!(
            managed_worktree_path(Path::new("/worktrees"), "alpha", record.id),
            PathBuf::from("/worktrees/alpha/7")
        );
        record.status = RecordStatus::Done;
        assert!(!is_worktree_eligible(&record));
        record.status = RecordStatus::InProgress;
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
            .open_task_under_wayfinder("wayfinder child", wayfinder.id, None, None)
            .unwrap();
        assert_eq!(wayfinder_task.project_slugs, ["alpha"]);
        let multi_wayfinder = app
            .open_wayfinder("multi", vec!["beta".into(), "alpha".into()])
            .unwrap();
        assert_eq!(multi_wayfinder.project_slugs, ["alpha", "beta"]);
        assert_eq!(
            app.open_task_under_wayfinder("missing", multi_wayfinder.id, None, None)
                .unwrap_err()
                .code,
            "missing-project"
        );
        assert_eq!(
            app.open_task_under_wayfinder(
                "outside",
                multi_wayfinder.id,
                Some("gamma".into()),
                None
            )
            .unwrap_err()
            .code,
            "scope-mismatch"
        );

        let standalone = app.open_task("standalone", None, None).unwrap();
        assert_eq!(standalone.record_type, RecordType::Task);
        assert!(standalone.project_slugs.is_empty());
        let task = app.open_task_under_spec("child", spec.id, None).unwrap();
        assert_eq!(task.record_type, RecordType::Task);
        assert_eq!(task.project_slugs, ["alpha"]);
        assert_eq!(task.relationships[0].parent_id, spec.id);
        assert_eq!(
            app.open_task_under_spec("invalid", standalone.id, None)
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
        let blocked = app.open_task("blocked", None, None).unwrap();
        let prerequisite = app.open_task("prerequisite", None, None).unwrap();
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
            app.list_records(Some("alpha".into()), None, None, None)
                .unwrap()
                .into_iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec![beta.id, alpha.id]
        );
        assert_eq!(
            app.list_records(None, Some(RecordStatus::Open), None, None)
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
                id: 1,
                record_type: RecordType::Generic,
                task_kind: None,
                body: "body".into(),
                project_slugs: vec!["alpha".into(), "zeta".into()],
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![],
            }
        );
    }
}
