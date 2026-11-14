//! Authenticated Assignment wire contract. Requests deliberately do not implement Debug.
use crate::{RecordCommentResponse, RecordResponse, RecordUpdateResponse, WorkspaceResponse};
use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentStackRequest {
    pub assignment_id: String,
    pub secret: String,
    /// None reads the own Task's stack; Some verifies its publication.
    pub url: Option<String>,
}

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

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentCheckListRequest {
    pub assignment_id: String,
    pub secret: String,
    pub compare_revision: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentCheckShowRequest {
    pub assignment_id: String,
    pub secret: String,
    pub check: u64,
    pub after: Option<u64>,
    pub limit: u32,
    pub compare_revision: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentCheckAssessRequest {
    pub assignment_id: String,
    pub secret: String,
    pub check: u64,
    pub expected_version: u64,
    pub verdict: crate::Verdict,
    pub message: String,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentCheckInvalidateRequest {
    pub assignment_id: String,
    pub secret: String,
    pub check: u64,
    pub expected_version: u64,
    pub assessment_id: u64,
    pub message: String,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentFindingListRequest {
    pub assignment_id: String,
    pub secret: String,
    pub after: Option<u64>,
    pub limit: u32,
    pub compare_revision: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentFindingShowRequest {
    pub assignment_id: String,
    pub secret: String,
    pub finding: u64,
    pub after: Option<u64>,
    pub limit: u32,
    pub compare_revision: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentFindingRaiseRequest {
    pub assignment_id: String,
    pub secret: String,
    pub severity: crate::Severity,
    pub blocking: Option<bool>,
    pub check_id: Option<u64>,
    pub assessment_id: Option<u64>,
    pub message: String,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentFindingMutationRequest {
    pub assignment_id: String,
    pub secret: String,
    pub finding: u64,
    pub expected_version: u64,
    #[serde(deserialize_with = "strict_finding_change")]
    pub change: crate::FindingMutation,
    pub message: String,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
}

// Serde's internally tagged unit variants otherwise ignore extra fields, even with
// deny_unknown_fields. Keep capability payloads strict for every action shape.
fn strict_finding_change<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<crate::FindingMutation, D::Error> {
    #[derive(Deserialize)]
    #[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
    enum Change {
        Respond {
            response: crate::FindingResponse,
        },
        Resolve {},
        Dismiss {},
        Reopen {},
        Amend {
            severity: Option<crate::Severity>,
            blocking: Option<bool>,
        },
        Note {},
    }
    Ok(match Change::deserialize(deserializer)? {
        Change::Respond { response } => crate::FindingMutation::Respond { response },
        Change::Resolve {} => crate::FindingMutation::Resolve,
        Change::Dismiss {} => crate::FindingMutation::Dismiss,
        Change::Reopen {} => crate::FindingMutation::Reopen,
        Change::Amend { severity, blocking } => {
            crate::FindingMutation::Amend { severity, blocking }
        }
        Change::Note {} => crate::FindingMutation::Note,
    })
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentOutcomeRequest {
    pub assignment_id: String,
    pub secret: String,
    pub outcome: forge_app::outcomes::OutcomeInput,
}

#[rpc(client, server, namespace = "assignment")]
pub trait AssignmentApi {
    #[method(name = "stack")]
    async fn stack(
        &self,
        request: AssignmentStackRequest,
    ) -> RpcResult<Option<forge_app::stacks::TaskStack>>;
    #[method(name = "outcomeSubmit")]
    async fn outcome_submit(
        &self,
        request: AssignmentOutcomeRequest,
    ) -> RpcResult<forge_app::outcomes::AcceptedOutcome>;
    #[method(name = "findingList")]
    async fn finding_list(
        &self,
        request: AssignmentFindingListRequest,
    ) -> RpcResult<crate::FindingList>;
    #[method(name = "findingShow")]
    async fn finding_show(
        &self,
        request: AssignmentFindingShowRequest,
    ) -> RpcResult<crate::FindingPage>;
    #[method(name = "findingRaise")]
    async fn finding_raise(
        &self,
        request: AssignmentFindingRaiseRequest,
    ) -> RpcResult<crate::Finding>;
    #[method(name = "findingMutate")]
    async fn finding_mutate(
        &self,
        request: AssignmentFindingMutationRequest,
    ) -> RpcResult<crate::Finding>;
    #[method(name = "checkList")]
    async fn check_list(
        &self,
        request: AssignmentCheckListRequest,
    ) -> RpcResult<Vec<crate::RecordCheck>>;
    #[method(name = "checkShow")]
    async fn check_show(&self, request: AssignmentCheckShowRequest) -> RpcResult<crate::CheckPage>;
    #[method(name = "checkAssess")]
    async fn check_assess(
        &self,
        request: AssignmentCheckAssessRequest,
    ) -> RpcResult<crate::RecordCheck>;
    #[method(name = "checkInvalidate")]
    async fn check_invalidate(
        &self,
        request: AssignmentCheckInvalidateRequest,
    ) -> RpcResult<crate::RecordCheck>;
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
