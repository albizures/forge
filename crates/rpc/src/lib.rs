//! Shared JSON-RPC wire contract ownership boundary.

use forge_app::{
    ApplicationStatus, Project, Record, RecordRelationshipKind, RecordStatus, RecordUpdate,
    RecordUpdateKind, RegistrationError,
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
#[rpc(client, server, namespace = "record")]
pub trait RecordApi {
    #[method(name = "open")]
    async fn open(&self, request: OpenRecordRequest) -> RpcResult<RecordResponse>;
    #[method(name = "show")]
    async fn show(&self, request: RecordIdRequest) -> RpcResult<RecordResponse>;
    #[method(name = "list")]
    async fn list(&self, request: ListRecordsRequest) -> RpcResult<ListRecordsResponse>;
    #[method(name = "edit")]
    async fn edit(&self, request: EditRecordRequest) -> RpcResult<RecordResponse>;
    #[method(name = "start")]
    async fn start(&self, request: RecordIdRequest) -> RpcResult<RecordResponse>;
    #[method(name = "done")]
    async fn done(&self, request: DoneRecordRequest) -> RpcResult<RecordResponse>;
    #[method(name = "relate")]
    async fn relate(&self, request: RelateRecordRequest) -> RpcResult<RecordResponse>;
    #[method(name = "unrelate")]
    async fn unrelate(&self, request: RelateRecordRequest) -> RpcResult<RecordResponse>;
    #[method(name = "updates")]
    async fn updates(&self, request: RecordIdRequest) -> RpcResult<ListRecordUpdatesResponse>;
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
pub struct RegisterProjectRequest {
    pub folder: String,
    pub slug: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegisterProjectResponse {
    pub slug: String,
    pub registered_folder: String,
    pub origin_url: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectHereRequest {
    pub folder: String,
}
pub type ProjectHereResponse = RegisterProjectResponse;
impl From<Project> for RegisterProjectResponse {
    fn from(project: Project) -> Self {
        Self {
            slug: project.slug,
            registered_folder: project.registered_folder.to_string_lossy().into_owned(),
            origin_url: project.origin_url,
        }
    }
}

/// Opens a Record. An empty `project_slugs` explicitly represents no Projects.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OpenRecordRequest {
    pub body: String,
    pub project_scope: ProjectScopeRequest,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectScopeRequest {
    pub project_slugs: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecordIdRequest {
    pub id: u64,
}
/// Optional Record discovery filters. Both filters combine with AND.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ListRecordsRequest {
    pub project_slug: Option<String>,
    pub status: Option<String>,
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
pub struct RecordUpdateResponse {
    pub id: u64,
    pub created_at_millis: u64,
    pub kind: String,
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    pub related_record_id: Option<u64>,
}
impl From<RecordUpdate> for RecordUpdateResponse {
    fn from(update: RecordUpdate) -> Self {
        let (kind, from_status, to_status, related_record_id) = match update.kind {
            RecordUpdateKind::Created => ("created", None, None, None),
            RecordUpdateKind::BodyEdited => ("body-edited", None, None, None),
            RecordUpdateKind::StatusTransition { from, to } => (
                "status-transition",
                Some(record_status_name(from)),
                Some(record_status_name(to)),
                None,
            ),
            RecordUpdateKind::RelationshipAdded { related_record_id } => {
                ("relationship-added", None, None, Some(related_record_id))
            }
            RecordUpdateKind::RelationshipRemoved { related_record_id } => {
                ("relationship-removed", None, None, Some(related_record_id))
            }
        };
        Self {
            id: update.id.0,
            created_at_millis: update.created_at_millis,
            kind: kind.into(),
            from_status,
            to_status,
            related_record_id,
        }
    }
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
/// Links or unlinks `child_id` to/from `parent_id` using the sole supported kind.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RelateRecordRequest {
    pub child_id: u64,
    pub parent_id: u64,
    pub kind: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecordRelationshipResponse {
    pub kind: String,
    pub parent_id: u64,
    pub child_id: u64,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecordResponse {
    pub id: u64,
    pub body: String,
    pub project_scope: ProjectScopeRequest,
    pub status: String,
    pub completion_resolution: Option<String>,
    pub relationships: Vec<RecordRelationshipResponse>,
}
impl From<Record> for RecordResponse {
    fn from(record: Record) -> Self {
        Self {
            id: record.id,
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
                    }
                    .into(),
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
