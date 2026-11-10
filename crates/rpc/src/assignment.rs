//! Authenticated Assignment wire contract. Requests deliberately do not implement Debug.
use crate::{RecordCommentResponse, RecordResponse, RecordUpdateResponse, WorkspaceResponse};
use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentCredentials {
    pub assignment_id: String,
    pub secret: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentReadRequest {
    pub assignment_id: String,
    pub secret: String,
    pub target: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentPageRequest {
    pub assignment_id: String,
    pub secret: String,
    pub target: u64,
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentCommentRequest {
    pub assignment_id: String,
    pub secret: String,
    pub body: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentDoneRequest {
    pub assignment_id: String,
    pub secret: String,
    pub resolution: Option<String>,
    pub human: Option<forge_app::assignment::HumanConfirmation>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentContextResponse {
    pub schema_version: u32,
    pub bound: forge_app::assignment::BoundIdentity,
    pub record: AssignmentLiveRecord,
    pub workspace: WorkspaceResponse,
    pub projects: Vec<AssignmentProject>,
    pub trusted_constraints: Vec<String>,
    pub related_records: Vec<RelatedRecordSummary>,
    pub freshness: String,
}

/// Project identity/root allowlist; never include credential-bearing remote URLs.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentProject {
    pub reference: String,
    pub workspace_id: String,
    pub slug: String,
    pub root: std::path::PathBuf,
}
impl From<forge_app::Project> for AssignmentProject {
    fn from(value: forge_app::Project) -> Self {
        Self {
            reference: value.reference,
            workspace_id: value.workspace_id,
            slug: value.slug,
            root: value.project_root,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AssignmentLiveRecord {
    pub title: String,
    #[serde(flatten)]
    pub record: RecordResponse,
}
impl From<forge_app::Record> for AssignmentLiveRecord {
    fn from(value: forge_app::Record) -> Self {
        Self {
            title: value.body.lines().next().unwrap_or_default().into(),
            record: value.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedRecordSummary {
    pub id: u64,
    pub record_type: String,
    pub title: String,
    pub status: String,
}

impl From<forge_app::assignment::Context> for AssignmentContextResponse {
    fn from(value: forge_app::assignment::Context) -> Self {
        Self {
            schema_version: value.schema_version,
            bound: value.bound,
            record: value.record.into(),
            workspace: value.workspace.into(),
            projects: value.projects.into_iter().map(Into::into).collect(),
            trusted_constraints: value.trusted_constraints,
            related_records: value
                .related_records
                .into_iter()
                .map(|record| RelatedRecordSummary {
                    id: record.id,
                    record_type: crate::record_type_name(record.record_type),
                    title: record.body.lines().next().unwrap_or_default().into(),
                    status: crate::record_status_name(record.status),
                })
                .collect(),
            freshness: value.freshness,
        }
    }
}

// Behavior-bearing responses intentionally have no Debug implementation.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentBootstrapResponse {
    pub schema_version: u32,
    pub context: AssignmentContextResponse,
    pub behavior: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentRecordResponse {
    pub schema_version: u32,
    pub record: AssignmentLiveRecord,
    pub freshness: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentCommentResponse {
    pub schema_version: u32,
    pub comment: RecordCommentResponse,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentPageResponse<T> {
    pub schema_version: u32,
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub freshness: u64,
}

#[rpc(client, server, namespace = "assignment")]
pub trait AssignmentApi {
    #[method(name = "bootstrap")]
    async fn bootstrap(
        &self,
        request: AssignmentCredentials,
    ) -> RpcResult<AssignmentBootstrapResponse>;
    #[method(name = "context")]
    async fn context(&self, request: AssignmentCredentials)
    -> RpcResult<AssignmentContextResponse>;
    #[method(name = "record")]
    async fn record(&self, request: AssignmentReadRequest) -> RpcResult<AssignmentRecordResponse>;
    #[method(name = "comments")]
    async fn comments(
        &self,
        request: AssignmentPageRequest,
    ) -> RpcResult<AssignmentPageResponse<RecordCommentResponse>>;
    #[method(name = "history")]
    async fn history(
        &self,
        request: AssignmentPageRequest,
    ) -> RpcResult<AssignmentPageResponse<RecordUpdateResponse>>;
    #[method(name = "comment")]
    async fn comment(
        &self,
        request: AssignmentCommentRequest,
    ) -> RpcResult<AssignmentCommentResponse>;
    #[method(name = "start")]
    async fn start(&self, request: AssignmentCredentials) -> RpcResult<AssignmentRecordResponse>;
    #[method(name = "done")]
    async fn done(&self, request: AssignmentDoneRequest) -> RpcResult<AssignmentRecordResponse>;
}
