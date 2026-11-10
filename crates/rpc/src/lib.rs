//! Shared JSON-RPC wire contract ownership boundary.

mod agent_profiles;
mod assignment;
pub use agent_profiles::*;
pub use assignment::*;
mod workspaces;
pub use workspaces::*;
mod record_selection;
pub use record_selection::*;

use forge_app::{
    AgentDispatchProjectStatus, AgentDispatchReconcileResult, AgentDispatchStatus, AgentProfile,
    ApplicationStatus, ManagedWorktreeInfo, Project, Record, RecordComment, RecordRelationshipKind,
    RecordStatus, RecordUpdate, RecordUpdateKind, RecordUpdateKindFilter, RecordUpdateSubscription,
    RegistrationError, TaskKind, WorktreeClaimState,
};
use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use serde::{Deserialize, Serialize};

pub const SYSTEM_STATUS_METHOD: &str = "system.status";
pub const PROJECT_REGISTER_METHOD: &str = "project.register";
pub const PROJECT_HERE_METHOD: &str = "project.here";
pub const RECORD_OPEN_METHOD: &str = "record.open";
pub const RECORD_SHOW_METHOD: &str = "record.show";
pub const RECORD_LIST_METHOD: &str = "record.list";
pub const RECORD_EDIT_METHOD: &str = "record.edit";
pub const RECORD_START_METHOD: &str = "record.start";
pub const RECORD_DONE_METHOD: &str = "record.done";
pub const RECORD_RELATE_METHOD: &str = "record.relate";
pub const RECORD_UNRELATE_METHOD: &str = "record.unrelate";
pub const RECORD_UPDATES_METHOD: &str = "record.updates";
pub const RECORD_COMMENT_METHOD: &str = "record.comment";
pub const RECORD_COMMENTS_METHOD: &str = "record.comments";
pub const RECORD_COMMENT_DELETE_METHOD: &str = "record.commentDelete";
pub const AGENT_DISPATCH_ENDPOINT_GET_METHOD: &str = "agentDispatch.endpointGet";
pub const AGENT_DISPATCH_ENDPOINT_SET_METHOD: &str = "agentDispatch.endpointSet";
pub const AGENT_DISPATCH_PROFILE_METHOD: &str = "agentDispatch.profile";
pub const AGENT_DISPATCH_PROJECT_SET_METHOD: &str = "agentDispatch.projectSet";
pub const AGENT_DISPATCH_STATUS_METHOD: &str = "agentDispatch.status";
pub const AGENT_DISPATCH_RECONCILE_METHOD: &str = "agentDispatch.reconcile";
pub const AGENT_DISPATCH_TASK_START_METHOD: &str = "agentDispatch.taskStart";
pub const AGENT_DISPATCH_TASK_REOPEN_METHOD: &str = "agentDispatch.taskReopen";
pub const AGENT_DISPATCH_RESUME_METHOD: &str = "agentDispatch.resume";
pub const AGENT_DISPATCH_ASSIGNMENT_RETIRE_METHOD: &str = "agentDispatch.assignmentRetire";
pub const AGENT_DISPATCH_ASSIGNMENT_INFO_METHOD: &str = "agentDispatch_assignmentInfo";
pub const AGENT_SUBSCRIPTION_CREATE_METHOD: &str = "agentSubscription.create";
pub const AGENT_SUBSCRIPTION_RENEW_METHOD: &str = "agentSubscription.renew";
pub const AGENT_SUBSCRIPTION_DELETE_METHOD: &str = "agentSubscription.delete";
pub const WORKTREE_SETUP_HOOK_SET_METHOD: &str = "worktree.setupHookSet";
pub const WORKTREE_CREATE_METHOD: &str = "worktree.create";
pub const WORKTREE_INFO_METHOD: &str = "worktree.info";
pub const WORKTREE_LIST_METHOD: &str = "worktree.list";
pub const WORKTREE_REMOVE_METHOD: &str = "worktree.remove";
pub const WORKTREE_DOCTOR_METHOD: &str = "worktree.doctor";

#[rpc(client, server, namespace = "system")]
pub trait SystemApi {
    #[method(name = "status")]
    async fn status(&self) -> RpcResult<SystemStatusResponse>;
}
#[rpc(client, server, namespace = "project")]
pub trait ProjectApi {
    #[method(name = "register")]
    async fn register(&self, request: RegisterProjectRequest)
    -> RpcResult<RegisterProjectResponse>;
    #[method(name = "here")]
    async fn here(&self, request: ProjectHereRequest) -> RpcResult<ProjectHereResponse>;
}
#[rpc(client, server, namespace = "agentDispatch")]
pub trait AgentDispatchApi {
    #[method(name = "configGet")]
    async fn config_get(
        &self,
        request: SelectedRecordRequest<DispatcherConfigGetRequest>,
    ) -> RpcResult<DispatcherConfigResponse>;
    #[method(name = "configSet")]
    async fn config_set(
        &self,
        request: SelectedRecordRequest<DispatcherConfigSetRequest>,
    ) -> RpcResult<DispatcherConfigResponse>;
    #[method(name = "configUnset")]
    async fn config_unset(
        &self,
        request: SelectedRecordRequest<DispatcherConfigUnsetRequest>,
    ) -> RpcResult<DispatcherConfigResponse>;
    #[method(name = "profile")]
    async fn profile(
        &self,
        request: SelectedRecordRequest<AgentDispatchProfileRequest>,
    ) -> RpcResult<AgentDispatchProfileResponse>;
    #[method(name = "status")]
    async fn dispatch_status(
        &self,
        request: SelectedRecordRequest<AgentDispatchStatusRequest>,
    ) -> RpcResult<AgentDispatchStatusResponse>;
    #[method(name = "reconcile")]
    async fn reconcile(&self) -> RpcResult<AgentDispatchReconcileResponse>;
    #[method(name = "dispatchRecord")]
    async fn dispatch_record(
        &self,
        request: SelectedRecordRequest<DispatchRecordRequest>,
    ) -> RpcResult<DispatchRecordResponse>;
    #[method(name = "assignmentAcknowledge")]
    async fn assignment_acknowledge(
        &self,
        request: AcknowledgeAssignmentRequest,
    ) -> RpcResult<AgentAssignmentResponse>;
    #[method(name = "taskStart")]
    async fn task_start(&self, request: StartAssignmentTaskRequest) -> RpcResult<RecordResponse>;
    #[method(name = "assignmentTaskStart")]
    async fn assignment_task_start(
        &self,
        request: AuthenticatedAssignmentTaskStartRequest,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "assignmentInstructions")]
    async fn assignment_instructions(
        &self,
        request: AcknowledgeAssignmentRequest,
    ) -> RpcResult<String>;
    #[method(name = "assignmentBootstrapFailure")]
    async fn assignment_bootstrap_failure(
        &self,
        request: AssignmentBootstrapFailureRequest,
    ) -> RpcResult<()>;
    #[method(name = "taskReopen")]
    async fn task_reopen(&self, request: RecordIdRequest) -> RpcResult<RecordResponse>;
    #[method(name = "resume")]
    async fn resume(&self, request: RecordIdRequest) -> RpcResult<()>;
    /// Operator diagnostics on the trusted daemon interface, not a capability-authenticated agent operation.
    #[method(name = "assignmentInfo")]
    async fn assignment_info(
        &self,
        request: AssignmentInfoRequest,
    ) -> RpcResult<AgentAssignmentInfoResponse>;
    #[method(name = "assignmentRetire")]
    async fn assignment_retire(&self, request: RetireAgentAssignmentRequest) -> RpcResult<()>;
    #[method(name = "assignmentResolveUnknown")]
    async fn assignment_resolve_unknown(
        &self,
        request: ResolveUnknownAgentResourcesRequest,
    ) -> RpcResult<AgentAssignmentInfoResponse>;
}

#[rpc(client, server, namespace = "agentSubscription")]
pub trait AgentSubscriptionApi {
    #[method(name = "create")]
    async fn create(
        &self,
        request: CreateAgentSubscriptionRequest,
    ) -> RpcResult<RecordUpdateSubscriptionResponse>;
    #[method(name = "renew")]
    async fn renew(
        &self,
        request: ManageAgentSubscriptionRequest,
    ) -> RpcResult<RecordUpdateSubscriptionResponse>;
    #[method(name = "delete")]
    async fn delete(&self, request: ManageAgentSubscriptionRequest) -> RpcResult<()>;
}

#[rpc(client, server, namespace = "worktree")]
pub trait WorktreeApi {
    #[method(name = "setupHookSet")]
    async fn setup_hook_set(
        &self,
        request: SelectedRecordRequest<SetProjectSetupHookRequest>,
    ) -> RpcResult<()>;
    #[method(name = "create")]
    async fn create(
        &self,
        request: SelectedRecordRequest<CreateManagedWorktreeRequest>,
    ) -> RpcResult<ManagedWorktreeResponse>;
    #[method(name = "recover")]
    async fn recover(
        &self,
        request: SelectedRecordRequest<RecoverManagedWorktreeRequest>,
    ) -> RpcResult<ManagedWorktreeResponse>;
    #[method(name = "info")]
    async fn info(
        &self,
        request: SelectedRecordRequest<RecordIdRequest>,
    ) -> RpcResult<ManagedWorktreeResponse>;
    #[method(name = "list")]
    async fn list(
        &self,
        request: SelectedRecordRequest<ListManagedWorktreesRequest>,
    ) -> RpcResult<ListManagedWorktreesResponse>;
    #[method(name = "remove")]
    async fn remove(
        &self,
        request: SelectedRecordRequest<RemoveManagedWorktreeRequest>,
    ) -> RpcResult<()>;
    #[method(name = "doctor")]
    async fn doctor(
        &self,
        request: SelectedRecordRequest<DoctorManagedWorktreesRequest>,
    ) -> RpcResult<ListManagedWorktreesResponse>;
}

#[rpc(client, server, namespace = "record")]
pub trait RecordApi {
    #[method(name = "openInitiative")]
    async fn open_initiative(
        &self,
        request: SelectedRecordRequest<OpenRecordRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "ready")]
    async fn ready(
        &self,
        request: SelectedRecordRequest<ReadyRecordsRequest>,
    ) -> RpcResult<ReadyRecordsResponse>;
    #[method(name = "open")]
    async fn open(
        &self,
        request: SelectedRecordRequest<OpenRecordRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "openSpec")]
    async fn open_spec(
        &self,
        request: SelectedRecordRequest<OpenSpecRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "openTask")]
    async fn open_task(
        &self,
        request: SelectedRecordRequest<OpenTaskRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "openWayfinder")]
    async fn open_wayfinder(
        &self,
        request: SelectedRecordRequest<OpenWayfinderRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "openTaskUnderSpec")]
    async fn open_task_under_spec(
        &self,
        request: SelectedRecordRequest<OpenTaskUnderSpecRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "openTaskUnderWayfinder")]
    async fn open_task_under_wayfinder(
        &self,
        request: SelectedRecordRequest<OpenTaskUnderWayfinderRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "openGrillingUnderWayfinder")]
    async fn open_grilling_under_wayfinder(
        &self,
        request: SelectedRecordRequest<OpenGrillingUnderWayfinderRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "show")]
    async fn show(
        &self,
        request: SelectedRecordRequest<RecordIdRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "list")]
    async fn list(
        &self,
        request: SelectedRecordRequest<ListRecordsRequest>,
    ) -> RpcResult<ListRecordsResponse>;
    #[method(name = "edit")]
    async fn edit(
        &self,
        request: SelectedRecordRequest<EditRecordRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "start")]
    async fn start(
        &self,
        request: SelectedRecordRequest<RecordIdRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "done")]
    async fn done(
        &self,
        request: SelectedRecordRequest<DoneRecordRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "relate")]
    async fn relate(
        &self,
        request: SelectedRecordRequest<RelateRecordRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "unrelate")]
    async fn unrelate(
        &self,
        request: SelectedRecordRequest<RelateRecordRequest>,
    ) -> RpcResult<RecordResponse>;
    #[method(name = "updates")]
    async fn updates(
        &self,
        request: SelectedRecordRequest<RecordIdRequest>,
    ) -> RpcResult<ListRecordUpdatesResponse>;
    #[method(name = "comment")]
    async fn comment(
        &self,
        request: SelectedRecordRequest<AddRecordCommentRequest>,
    ) -> RpcResult<RecordCommentResponse>;
    #[method(name = "comments")]
    async fn comments(
        &self,
        request: SelectedRecordRequest<RecordIdRequest>,
    ) -> RpcResult<ListRecordCommentsResponse>;
    #[method(name = "commentDelete")]
    async fn comment_delete(
        &self,
        request: SelectedRecordRequest<DeleteRecordCommentRequest>,
    ) -> RpcResult<()>;
}

/// Immutable daemon build facts reported through the management protocol.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RevisionIdentity {
    pub commit: String,
    pub dirty: bool,
    pub management_protocol_revision: u32,
    pub version: String,
}

impl From<forge_build_info::BuildInfo> for RevisionIdentity {
    fn from(build_info: forge_build_info::BuildInfo) -> Self {
        Self {
            commit: build_info.commit.to_owned(),
            dirty: build_info.dirty,
            management_protocol_revision: build_info.management_protocol_revision,
            version: build_info.version.to_owned(),
        }
    }
}

/// Management facts added to `system.status` without changing its legacy fields.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManagementStatus {
    pub management_protocol_revision: u32,
    pub revision_identity: RevisionIdentity,
    pub daemon_instance_id: Option<String>,
}

impl ManagementStatus {
    #[must_use]
    pub fn new(revision_identity: RevisionIdentity, daemon_instance_id: Option<String>) -> Self {
        Self {
            management_protocol_revision: revision_identity.management_protocol_revision,
            revision_identity,
            daemon_instance_id,
        }
    }

    /// Validates this status against revisions declared by the consumer.
    ///
    /// # Errors
    ///
    /// Returns an explicit error rather than allowing a consumer to infer compatibility.
    pub fn validate_supported_by(
        &self,
        supported_revisions: &[u32],
    ) -> Result<(), UnsupportedManagementProtocolRevision> {
        if supported_revisions.contains(&self.management_protocol_revision) {
            Ok(())
        } else {
            Err(UnsupportedManagementProtocolRevision {
                reported: self.management_protocol_revision,
                supported: supported_revisions.to_vec(),
            })
        }
    }
}

/// A consumer does not implement the daemon's reported protocol revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsupportedManagementProtocolRevision {
    pub reported: u32,
    pub supported: Vec<u32>,
}

impl std::fmt::Display for UnsupportedManagementProtocolRevision {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "unsupported management protocol revision {}; supported revisions: {:?}",
            self.reported, self.supported
        )
    }
}

impl std::error::Error for UnsupportedManagementProtocolRevision {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SystemStatusResponse {
    pub name: String,
    pub version: String,
    pub ready: bool,
    pub management: ManagementStatus,
}

impl SystemStatusResponse {
    #[must_use]
    pub fn new(status: &ApplicationStatus, management: ManagementStatus) -> Self {
        Self {
            name: status.name.to_owned(),
            version: status.version.to_owned(),
            ready: status.ready,
            management,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DispatcherConfigGetRequest {
    pub key: Option<String>,
    pub project: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DispatcherConfigSetRequest {
    pub key: String,
    pub value: String,
    pub project: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DispatcherConfigUnsetRequest {
    pub key: String,
    pub project: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DispatcherConfigEntryResponse {
    pub ownership: String,
    pub key: String,
    pub value: Option<String>,
    pub provenance: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DispatcherConfigResponse {
    pub entries: Vec<DispatcherConfigEntryResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applicability: Option<String>,
}
impl From<Vec<forge_app::DispatcherConfigEntry>> for DispatcherConfigResponse {
    fn from(entries: Vec<forge_app::DispatcherConfigEntry>) -> Self {
        Self {
            applicability: None,
            entries: entries
                .into_iter()
                .map(|entry| DispatcherConfigEntryResponse {
                    ownership: entry.ownership,
                    key: entry.key,
                    value: entry.value,
                    provenance: entry.provenance,
                })
                .collect(),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentDispatchStatusRequest {
    pub project_slug: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentDispatchProjectStatusResponse {
    pub project_slug: String,
    pub capacity: u16,
    pub configured_capacity: Option<u16>,
}
impl From<AgentDispatchProjectStatus> for AgentDispatchProjectStatusResponse {
    fn from(value: AgentDispatchProjectStatus) -> Self {
        Self {
            project_slug: value.project_slug,
            capacity: value.capacity,
            configured_capacity: value.configured_capacity,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DispatchBlockerResponse {
    pub code: String,
    pub message: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentDispatchStatusResponse {
    pub workspace_id: String,
    pub configured: bool,
    pub dispatchable: bool,
    pub blockers: Vec<DispatchBlockerResponse>,
    pub herdr_socket_path: Option<String>,
    pub endpoint_diagnostic: String,
    pub project: Option<AgentDispatchProjectStatusResponse>,
}
impl From<(AgentDispatchStatus, forge_app::WorkspaceDispatchReadiness)>
    for AgentDispatchStatusResponse
{
    fn from(
        (value, readiness): (AgentDispatchStatus, forge_app::WorkspaceDispatchReadiness),
    ) -> Self {
        Self {
            workspace_id: readiness.configuration.workspace_id,
            configured: readiness.configured,
            dispatchable: readiness.dispatchable,
            blockers: readiness
                .blockers
                .into_iter()
                .map(|b| DispatchBlockerResponse {
                    code: b.code,
                    message: b.message,
                })
                .collect(),
            herdr_socket_path: value
                .herdr_socket_path
                .map(|path| path.to_string_lossy().into_owned()),
            endpoint_diagnostic: value.endpoint_diagnostic,
            project: value.project.map(Into::into),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentDispatchProfileRequest {
    pub kind: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentDispatchProfileResponse {
    pub record_type: String,
    pub revision: u32,
    pub provider: String,
    pub agent_kind: String,
    pub model: String,
    pub prompt_template: String,
    pub startup_timeout_millis: u64,
    pub acknowledgement_timeout_millis: u64,
}
impl From<AgentProfile> for AgentDispatchProfileResponse {
    fn from(value: AgentProfile) -> Self {
        Self {
            record_type: record_type_name(value.record_type),
            revision: value.revision,
            provider: value.provider,
            agent_kind: value.agent_kind,
            model: value.model,
            prompt_template: value.prompt_template,
            startup_timeout_millis: value.startup_timeout_millis,
            acknowledgement_timeout_millis: value.acknowledgement_timeout_millis,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DispatchRecordRequest {
    pub record_id: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DispatchRecordResponse {
    pub assignment: AgentAssignmentResponse,
    pub already_assigned: bool,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentAssignmentResponse {
    pub id: String,
    pub record_id: u64,
    pub state: String,
}
impl From<forge_app::AgentAssignment> for AgentAssignmentResponse {
    fn from(value: forge_app::AgentAssignment) -> Self {
        Self {
            id: value.id,
            record_id: value.record_id,
            state: format!("{:?}", value.state).to_lowercase(),
        }
    }
}

/// Explicit allowlist of diagnostic fields; never serialize an Agent Profile or Task body.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentAssignmentInfoResponse {
    pub id: String,
    pub record_id: u64,
    pub project_slug: String,
    pub task_status: String,
    pub state: String,
    pub provider: String,
    pub attempt_number: u32,
    pub claimed_at_millis: u64,
    pub state_changed_at_millis: u64,
    pub closed_at_millis: Option<u64>,
    pub closure_reason: Option<String>,
    pub launch_started_at_millis: Option<u64>,
    pub acknowledgement_deadline_millis: Option<u64>,
    pub acknowledged_at_millis: Option<u64>,
    pub dispatch_hold: Option<String>,
    pub resources: Vec<AgentAssignmentResourceResponse>,
    pub recovery_blockers: Vec<String>,
    pub capacity_reserved: bool,
    pub operations: Vec<String>,
    #[serde(default)]
    pub unknown_resource_resolution: Option<UnknownResourceResolutionResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UnknownResourceResolutionResponse {
    pub confirmed_at_millis: u64,
    pub authority: String,
    pub confirmation: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentAssignmentResourceResponse {
    pub resource_kind: String,
    pub resource_id: String,
    pub created_at_millis: u64,
    pub retired_at_millis: Option<u64>,
}

impl From<forge_app::AgentAssignmentInfo> for AgentAssignmentInfoResponse {
    fn from(value: forge_app::AgentAssignmentInfo) -> Self {
        let capacity_reserved = value.capacity_reserved();
        let recovery_blockers = value.recovery_blockers();
        let assignment = value.assignment;
        let operations = vec![
            format!("forge dispatcher assignment-info {}", assignment.id),
            format!("forge worktree info {}", assignment.record_id),
            format!("forge dispatcher assignment-retire {}", assignment.id),
            format!(
                "forge dispatcher assignment-resolve-unknown {} --confirm-old-agent-cannot-act --confirm-unknown-resources-safe",
                assignment.id
            ),
            format!("forge dispatcher resume {}", assignment.record_id),
        ];
        Self {
            id: assignment.id,
            record_id: assignment.record_id,
            project_slug: assignment.project_slug,
            task_status: record_status_name(value.task_status),
            state: format!("{:?}", assignment.state).to_lowercase(),
            provider: assignment.profile.provider,
            attempt_number: assignment.attempt_number,
            claimed_at_millis: assignment.claimed_at_millis,
            state_changed_at_millis: assignment.state_changed_at_millis,
            closed_at_millis: assignment.closed_at_millis,
            closure_reason: assignment
                .closure_reason
                .map(|reason| format!("{reason:?}").to_lowercase()),
            launch_started_at_millis: value
                .launch_intent
                .as_ref()
                .map(|intent| intent.started_at_millis),
            acknowledgement_deadline_millis: value
                .launch_intent
                .map(|intent| intent.acknowledgement_deadline_millis),
            acknowledged_at_millis: value.acknowledged_at_millis,
            dispatch_hold: value.dispatch_hold.map(|hold| hold.reason),
            resources: value
                .resources
                .into_iter()
                .map(|resource| AgentAssignmentResourceResponse {
                    resource_kind: resource.resource_kind,
                    resource_id: resource.resource_id,
                    created_at_millis: resource.created_at_millis,
                    retired_at_millis: resource.retired_at_millis,
                })
                .collect(),
            recovery_blockers,
            capacity_reserved,
            operations,
            unknown_resource_resolution: value.unknown_resource_resolution.map(|resolution| {
                UnknownResourceResolutionResponse {
                    confirmed_at_millis: resolution.confirmed_at_millis,
                    authority: resolution.authority,
                    confirmation: resolution.confirmation,
                }
            }),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentInfoRequest {
    pub assignment_id: String,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct AcknowledgeAssignmentRequest {
    pub assignment_id: String,
    pub secret: String,
    pub correlation_id: String,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct AssignmentBootstrapFailureRequest {
    pub assignment_id: String,
    pub secret: String,
    pub correlation_id: String,
    pub message: String,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthenticatedAssignmentTaskStartRequest {
    pub assignment_id: String,
    pub secret: String,
    pub correlation_id: String,
    pub task_id: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StartAssignmentTaskRequest {
    pub assignment_id: String,
    pub capability_id: String,
    pub correlation_id: String,
    pub task_id: u64,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetireAgentAssignmentRequest {
    pub assignment_id: String,
}

/// Trusted operator operation, never an Assignment Capability operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveUnknownAgentResourcesRequest {
    pub assignment_id: String,
    #[serde(default)]
    pub confirm_old_agent_cannot_act: bool,
    #[serde(default)]
    pub confirm_unknown_resources_safe: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentDispatchReconcileResponse {
    pub acquired_lease: bool,
    pub reconciled_assignments: usize,
}
impl From<AgentDispatchReconcileResult> for AgentDispatchReconcileResponse {
    fn from(value: AgentDispatchReconcileResult) -> Self {
        Self {
            acquired_lease: value.acquired_lease,
            reconciled_assignments: value.reconciled_assignments,
        }
    }
}

// Legacy field name capability_id carries the actual secret, never an audit identity.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateAgentSubscriptionRequest {
    pub assignment_id: String,
    pub capability_id: String,
    pub correlation_id: String,
    pub target_record_ids: Vec<u64>,
    pub kind_filters: Vec<String>,
}
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManageAgentSubscriptionRequest {
    pub assignment_id: String,
    pub capability_id: String,
    pub correlation_id: String,
    pub subscription_id: u64,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecordUpdateSubscriptionResponse {
    pub id: u64,
    pub assignment_id: String,
    pub target_record_ids: Vec<u64>,
    pub kind_filters: Vec<String>,
    pub start_cursor: u64,
    pub lease_expires_at_millis: u64,
}
impl From<RecordUpdateSubscription> for RecordUpdateSubscriptionResponse {
    fn from(value: RecordUpdateSubscription) -> Self {
        Self {
            id: value.id.0,
            assignment_id: value.assignment_id,
            target_record_ids: value.target_record_ids,
            kind_filters: value
                .kind_filters
                .into_iter()
                .map(record_update_kind_filter_name)
                .collect(),
            start_cursor: value.start_cursor.0,
            lease_expires_at_millis: value.lease_expires_at_millis,
        }
    }
}
fn record_update_kind_filter_name(filter: RecordUpdateKindFilter) -> String {
    match filter {
        RecordUpdateKindFilter::Created => "created",
        RecordUpdateKindFilter::BodyEdited => "body-edited",
        RecordUpdateKindFilter::StatusTransition => "status-transition",
        RecordUpdateKindFilter::RelationshipAdded => "relationship-added",
        RecordUpdateKindFilter::RelationshipRemoved => "relationship-removed",
        RecordUpdateKindFilter::CommentAdded => "comment-added",
        RecordUpdateKindFilter::CommentDeleted => "comment-deleted",
    }
    .into()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SetProjectSetupHookRequest {
    pub project_slug: String,
    pub hook: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateManagedWorktreeRequest {
    pub record_id: u64,
    pub branch: String,
    pub base: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoveManagedWorktreeRequest {
    pub record_id: u64,
    pub force: bool,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverManagedWorktreeRequest {
    pub record_id: u64,
    pub confirm_no_active_agent: bool,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ListManagedWorktreesRequest {
    pub include_all: bool,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DoctorManagedWorktreesRequest {
    pub repair: bool,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManagedWorktreeResponse {
    pub record_id: u64,
    pub project_slug: String,
    pub path: String,
    pub state: String,
    pub facts: Vec<String>,
    pub blockers: Vec<String>,
    pub removable: bool,
}
impl From<ManagedWorktreeInfo> for ManagedWorktreeResponse {
    fn from(value: ManagedWorktreeInfo) -> Self {
        Self {
            record_id: value.claim.record_id,
            project_slug: value.claim.project_slug,
            path: value.claim.path.to_string_lossy().into_owned(),
            state: match value.claim.state {
                WorktreeClaimState::Reserving => "reserving",
                WorktreeClaimState::Active => "active",
                WorktreeClaimState::CleanupRequired => "cleanup-required",
            }
            .into(),
            facts: value.safety.facts,
            blockers: value.safety.blockers,
            removable: value.safety.removable,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ListManagedWorktreesResponse {
    pub worktrees: Vec<ManagedWorktreeResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegisterProjectRequest {
    pub folder: String,
    pub slug: String,
    pub workspace: Option<String>,
    pub cwd: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegisterProjectResponse {
    pub reference: String,
    pub workspace_id: String,
    pub slug: String,
    pub project_root: String,
    pub origin_url: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectHereRequest {
    pub folder: String,
    pub workspace: Option<String>,
    pub project: Option<String>,
}
pub type ProjectHereResponse = RegisterProjectResponse;
impl From<Project> for RegisterProjectResponse {
    fn from(project: Project) -> Self {
        Self {
            reference: project.reference,
            workspace_id: project.workspace_id,
            slug: project.slug,
            project_root: project.project_root.to_string_lossy().into_owned(),
            origin_url: project.origin_url,
        }
    }
}

/// Opens a Record. An empty `project_slugs` explicitly represents no Projects.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OpenRecordRequest {
    pub body: String,
    pub project_scope: ProjectScopeRequest,
    pub generated_by_ids: Vec<u64>,
}
/// Opens a Spec with its required single-Project scope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OpenSpecRequest {
    pub body: String,
    pub project_slug: String,
    pub generated_by_ids: Vec<u64>,
}
/// Opens a Wayfinder with zero or more Projects in its selected Workspace.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OpenWayfinderRequest {
    pub body: String,
    pub project_scope: ProjectScopeRequest,
    pub generated_by_ids: Vec<u64>,
}
/// Opens a standalone Task with an optional single-Project scope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OpenTaskRequest {
    pub body: String,
    pub project_slug: Option<String>,
    pub task_kind: Option<String>,
    pub generated_by_ids: Vec<u64>,
}
/// Opens a Task under a Spec, inheriting its scope atomically.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OpenTaskUnderSpecRequest {
    pub body: String,
    pub spec_id: u64,
    pub task_kind: Option<String>,
    pub generated_by_ids: Vec<u64>,
}
/// Opens a Task under a Wayfinder with an optional selected Project.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OpenTaskUnderWayfinderRequest {
    pub body: String,
    pub wayfinder_id: u64,
    pub project_slug: Option<String>,
    pub task_kind: Option<String>,
    pub generated_by_ids: Vec<u64>,
}
/// Opens a Grilling under a Wayfinder, inheriting its scope unless a subset is selected.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OpenGrillingUnderWayfinderRequest {
    pub body: String,
    pub wayfinder_id: u64,
    pub project_scope: Option<ProjectScopeRequest>,
    pub generated_by_ids: Vec<u64>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectScopeRequest {
    pub project_slugs: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecordIdRequest {
    pub id: u64,
}
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ReadyRecordsRequest {
    pub planning: bool,
    pub include_hitl: bool,
    pub blocked: bool,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReadyRecordResponse {
    pub record: RecordResponse,
    pub blocked_by: Vec<u64>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReadyRecordsResponse {
    pub records: Vec<ReadyRecordResponse>,
}

/// Optional Record discovery filters. Both filters combine with AND.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ListRecordsRequest {
    pub project_slug: Option<String>,
    pub status: Option<String>,
    pub record_type: Option<String>,
    pub task_kind: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ListRecordsResponse {
    pub records: Vec<RecordResponse>,
}
/// Record Update Log response ordered newest first.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ListRecordUpdatesResponse {
    pub updates: Vec<RecordUpdateResponse>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AddRecordCommentRequest {
    pub record_id: u64,
    pub body: String,
    pub author: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeleteRecordCommentRequest {
    pub record_id: u64,
    pub comment_id: u64,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ListRecordCommentsResponse {
    pub comments: Vec<RecordCommentResponse>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecordCommentResponse {
    pub id: u64,
    pub created_at_millis: u64,
    pub author: Option<String>,
    pub body: String,
}
impl From<RecordComment> for RecordCommentResponse {
    fn from(comment: RecordComment) -> Self {
        Self {
            id: comment.id.0,
            created_at_millis: comment.created_at_millis,
            author: comment.author,
            body: comment.body,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecordUpdateResponse {
    pub id: u64,
    pub created_at_millis: u64,
    pub kind: String,
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    pub related_record_id: Option<u64>,
    pub relationship_kind: Option<String>,
    pub comment_id: Option<u64>,
}
impl From<RecordUpdate> for RecordUpdateResponse {
    fn from(update: RecordUpdate) -> Self {
        let (kind, from_status, to_status, related_record_id, relationship_kind, comment_id) =
            match update.kind {
                RecordUpdateKind::Created => ("created", None, None, None, None, None),
                RecordUpdateKind::BodyEdited => ("body-edited", None, None, None, None, None),
                RecordUpdateKind::StatusTransition { from, to } => (
                    "status-transition",
                    Some(record_status_name(from)),
                    Some(record_status_name(to)),
                    None,
                    None,
                    None,
                ),
                RecordUpdateKind::RelationshipAdded {
                    related_record_id,
                    relationship_kind,
                } => (
                    "relationship-added",
                    None,
                    None,
                    Some(related_record_id),
                    relationship_kind.map(record_relationship_kind_name),
                    None,
                ),
                RecordUpdateKind::RelationshipRemoved {
                    related_record_id,
                    relationship_kind,
                } => (
                    "relationship-removed",
                    None,
                    None,
                    Some(related_record_id),
                    relationship_kind.map(record_relationship_kind_name),
                    None,
                ),
                RecordUpdateKind::CommentAdded { comment_id } => {
                    ("comment-added", None, None, None, None, Some(comment_id.0))
                }
                RecordUpdateKind::CommentDeleted { comment_id } => (
                    "comment-deleted",
                    None,
                    None,
                    None,
                    None,
                    Some(comment_id.0),
                ),
            };
        Self {
            id: update.id.0,
            created_at_millis: update.created_at_millis,
            kind: kind.into(),
            from_status,
            to_status,
            related_record_id,
            relationship_kind,
            comment_id,
        }
    }
}
fn record_relationship_kind_name(kind: RecordRelationshipKind) -> String {
    match kind {
        RecordRelationshipKind::ParentChild => "parent-child",
        RecordRelationshipKind::DependsOn => "depends-on",
        RecordRelationshipKind::GeneratedBy => "generated-by",
        RecordRelationshipKind::Initiative => "initiative",
    }
    .into()
}

fn task_kind_name(task_kind: TaskKind) -> String {
    match task_kind {
        TaskKind::Research => "research",
        TaskKind::Prototype => "prototype",
        TaskKind::Review => "review",
    }
    .into()
}

fn record_type_name(record_type: forge_app::RecordType) -> String {
    match record_type {
        forge_app::RecordType::Generic => "generic",
        forge_app::RecordType::Spec => "spec",
        forge_app::RecordType::Task => "task",
        forge_app::RecordType::Wayfinder => "wayfinder",
        forge_app::RecordType::Grilling => "grilling",
        forge_app::RecordType::Initiative => "initiative",
    }
    .into()
}
fn record_status_name(status: RecordStatus) -> String {
    match status {
        RecordStatus::Open => "open",
        RecordStatus::InProgress => "in_progress",
        RecordStatus::Done => "done",
    }
    .into()
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EditRecordRequest {
    pub id: u64,
    pub body: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DoneRecordRequest {
    pub id: u64,
    pub completion_resolution: Option<String>,
}
/// Links or unlinks a typed directional Record Relationship.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RelateRecordRequest {
    pub from_id: u64,
    pub to_id: u64,
    pub kind: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecordRelationshipResponse {
    pub kind: String,
    pub from_id: u64,
    pub to_id: u64,
    pub parent_id: u64,
    pub child_id: u64,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecordResponse {
    pub workspace_id: String,
    pub id: u64,
    pub record_type: String,
    pub task_kind: Option<String>,
    pub body: String,
    pub project_scope: ProjectScopeRequest,
    pub status: String,
    pub completion_resolution: Option<String>,
    pub relationships: Vec<RecordRelationshipResponse>,
}
impl From<Record> for RecordResponse {
    fn from(record: Record) -> Self {
        Self {
            workspace_id: record.workspace_id,
            id: record.id,
            record_type: record_type_name(record.record_type),
            task_kind: record.task_kind.map(task_kind_name),
            body: record.body,
            project_scope: ProjectScopeRequest {
                project_slugs: record.project_slugs,
            },
            status: record_status_name(record.status),
            completion_resolution: record.completion_resolution,
            relationships: record
                .relationships
                .into_iter()
                .map(|relationship| RecordRelationshipResponse {
                    kind: match relationship.kind {
                        RecordRelationshipKind::ParentChild => "parent-child",
                        RecordRelationshipKind::DependsOn => "depends-on",
                        RecordRelationshipKind::GeneratedBy => "generated-by",
                        RecordRelationshipKind::Initiative => "initiative",
                    }
                    .into(),
                    from_id: relationship.from_id,
                    to_id: relationship.to_id,
                    parent_id: relationship.parent_id,
                    child_id: relationship.child_id,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegistrationErrorResponse {
    pub code: String,
    pub message: String,
}
impl From<RegistrationError> for RegistrationErrorResponse {
    fn from(error: RegistrationError) -> Self {
        Self {
            code: error.code.into(),
            message: error.message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revision_identity(revision: u32) -> RevisionIdentity {
        RevisionIdentity {
            commit: "0123456789abcdef".into(),
            dirty: false,
            management_protocol_revision: revision,
            version: "1.2.3".into(),
        }
    }

    #[test]
    fn status_serialization_retains_legacy_fields_and_adds_management() {
        let response = SystemStatusResponse {
            name: "forge".into(),
            version: "1.2.3".into(),
            ready: true,
            management: ManagementStatus::new(revision_identity(1), None),
        };

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["name"], "forge");
        assert_eq!(json["version"], "1.2.3");
        assert_eq!(json["ready"], true);
        assert_eq!(json["management"]["management_protocol_revision"], 1);
        assert_eq!(
            json["management"]["revision_identity"]["commit"],
            "0123456789abcdef"
        );
        assert!(json["management"]["daemon_instance_id"].is_null());

        assert_eq!(
            serde_json::from_value::<SystemStatusResponse>(json).unwrap(),
            response
        );
    }

    #[test]
    fn managed_status_includes_its_daemon_instance_id() {
        let management = ManagementStatus::new(revision_identity(1), Some("instance-123".into()));

        assert_eq!(
            management.daemon_instance_id.as_deref(),
            Some("instance-123")
        );
    }

    #[test]
    fn validation_rejects_unsupported_management_protocol_revisions() {
        let error = ManagementStatus::new(revision_identity(2), None)
            .validate_supported_by(&[1])
            .unwrap_err();

        assert_eq!(error.reported, 2);
        assert_eq!(error.supported, vec![1]);
        assert_eq!(
            error.to_string(),
            "unsupported management protocol revision 2; supported revisions: [1]"
        );
    }
}
