//! Shared JSON-RPC wire contract ownership boundary.

use forge_app::{
    ApplicationStatus, Project, Record, RecordRelationshipKind, RecordStatus, RegistrationError,
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
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SystemStatusResponse {
    pub name: String,
    pub version: String,
    pub ready: bool,
}
impl From<ApplicationStatus> for SystemStatusResponse {
    fn from(status: ApplicationStatus) -> Self {
        Self {
            name: status.name.to_owned(),
            version: status.version.to_owned(),
            ready: status.ready,
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
            status: match record.status {
                RecordStatus::Open => "open",
                RecordStatus::InProgress => "in_progress",
                RecordStatus::Done => "done",
            }
            .into(),
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
