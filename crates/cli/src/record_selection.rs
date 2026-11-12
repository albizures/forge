//! Client-side routing: capture cwd here, never at the daemon.
use forge_rpc::{
    AddRecordCommentRequest, DeleteRecordCommentRequest, DoneRecordRequest, EditRecordRequest,
    ListRecordCommentsResponse, ListRecordUpdatesResponse, ListRecordsRequest, ListRecordsResponse,
    OpenGrillingUnderWayfinderRequest, OpenRecordRequest, OpenSpecRequest, OpenTaskRequest,
    OpenTaskUnderSpecRequest, OpenTaskUnderWayfinderRequest, OpenWayfinderRequest,
    ReadyRecordsRequest, ReadyRecordsResponse, RecordApiClient, RecordCommentResponse,
    RecordIdRequest, RecordResponse, RecordSelection, RelateRecordRequest, SelectedRecordRequest,
};
use jsonrpsee::http_client::HttpClient;

#[derive(Clone, Debug)]
pub struct RecordEndpoint {
    pub url: String,
    pub workspace: Option<String>,
}
impl From<&str> for RecordEndpoint {
    fn from(url: &str) -> Self {
        Self {
            url: url.into(),
            workspace: None,
        }
    }
}
impl From<&String> for RecordEndpoint {
    fn from(url: &String) -> Self {
        Self::from(url.as_str())
    }
}
impl From<(&str, Option<&str>)> for RecordEndpoint {
    fn from((url, workspace): (&str, Option<&str>)) -> Self {
        Self {
            url: url.into(),
            workspace: workspace.map(str::to_owned),
        }
    }
}

pub(super) struct RecordClient {
    client: HttpClient,
    selection: RecordSelection,
}
pub(super) fn record_client(endpoint: impl Into<RecordEndpoint>) -> Result<RecordClient, String> {
    let endpoint = endpoint.into();
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    Ok(RecordClient {
        client: super::client(&endpoint.url)?,
        selection: RecordSelection {
            workspace: endpoint.workspace,
            cwd: Some(cwd.to_string_lossy().into_owned()),
        },
    })
}
macro_rules! requests {
    ($($method:ident($request:ty) -> $response:ty;)+) => {
        impl RecordClient {
            $(pub(super) async fn $method(&self, record: $request) -> Result<$response, jsonrpsee::core::client::Error> {
                RecordApiClient::$method(&self.client, SelectedRecordRequest { selection: self.selection.clone(), record }).await
            })+
        }
    }
}
macro_rules! worktree_requests {
    ($($local:ident => $method:ident($request:ty) -> $response:ty;)+) => {
        impl RecordClient {
            $(pub(super) async fn $local(&self, record: $request) -> Result<$response, jsonrpsee::core::client::Error> {
                forge_rpc::WorktreeApiClient::$method(&self.client, SelectedRecordRequest { selection: self.selection.clone(), record }).await
            })+
        }
    }
}
worktree_requests! {
    worktree_setup_hook => setup_hook_set(forge_rpc::SetProjectSetupHookRequest) -> ();
    worktree_create => create(forge_rpc::CreateManagedWorktreeRequest) -> forge_rpc::ManagedWorktreeResponse;
    worktree_recover => recover(forge_rpc::RecoverManagedWorktreeRequest) -> forge_rpc::ManagedWorktreeResponse;
    worktree_info => info(RecordIdRequest) -> forge_rpc::ManagedWorktreeResponse;
    worktree_list => list(forge_rpc::ListManagedWorktreesRequest) -> forge_rpc::ListManagedWorktreesResponse;
    worktree_remove => remove(forge_rpc::RemoveManagedWorktreeRequest) -> ();
    worktree_doctor => doctor(forge_rpc::DoctorManagedWorktreesRequest) -> forge_rpc::ListManagedWorktreesResponse;
}
macro_rules! dispatch_requests {
    ($($method:ident($request:ty) -> $response:ty;)+) => {
        impl RecordClient {
            $(pub(super) async fn $method(&self, record: $request) -> Result<$response, jsonrpsee::core::client::Error> {
                forge_rpc::AgentDispatchApiClient::$method(&self.client, SelectedRecordRequest { selection: self.selection.clone(), record }).await
            })+
        }
    }
}
dispatch_requests! {
    profile(forge_rpc::AgentDispatchProfileRequest) -> forge_rpc::AgentDispatchProfileResponse;
    config_get(forge_rpc::DispatcherConfigGetRequest) -> forge_rpc::DispatcherConfigResponse;
    config_set(forge_rpc::DispatcherConfigSetRequest) -> forge_rpc::DispatcherConfigResponse;
    config_unset(forge_rpc::DispatcherConfigUnsetRequest) -> forge_rpc::DispatcherConfigResponse;
    dispatch_status(forge_rpc::AgentDispatchStatusRequest) -> forge_rpc::AgentDispatchStatusResponse;
    dispatch_record(forge_rpc::DispatchRecordRequest) -> forge_rpc::DispatchRecordResponse;
}
requests! {
    project_check_template_show(forge_rpc::CheckTemplateShowRequest) -> forge_rpc::TemplatePage;
    project_check_template_set(forge_rpc::CheckTemplateSetRequest) -> forge_rpc::CheckTemplate;
    record_finding_list(forge_rpc::RecordFindingListRequest) -> forge_rpc::FindingList;
    record_finding_show(forge_rpc::RecordFindingShowRequest) -> forge_rpc::FindingPage;
    record_finding_raise(forge_rpc::RecordFindingRaiseRequest) -> forge_rpc::Finding;
    record_finding_resolve(forge_rpc::RecordFindingResolveRequest) -> forge_rpc::Finding;
    record_finding_mutate(forge_rpc::RecordFindingMutationRequest) -> forge_rpc::Finding;
    record_check_list(forge_rpc::RecordCheckListRequest) -> Vec<forge_rpc::RecordCheck>;
    record_check_invalidate(forge_rpc::RecordCheckInvalidateRequest) -> forge_rpc::RecordCheck;
    record_check_show(forge_rpc::RecordCheckShowRequest) -> forge_rpc::CheckPage;
    record_check_assess(forge_rpc::RecordCheckAssessRequest) -> forge_rpc::RecordCheck;
    record_work_state(forge_rpc::RecordWorkStateRequest) -> forge_rpc::WorkState;
    record_attempt_begin(forge_rpc::RecordAttemptBeginRequest) -> forge_rpc::Attempt;
    record_outcome_submit(forge_rpc::RecordOutcomeSubmitRequest) -> forge_rpc::AcceptedOutcome;
    record_work_invalidate(forge_rpc::RecordWorkInvalidateRequest) -> forge_rpc::WorkState;
    open_initiative(OpenRecordRequest) -> RecordResponse;
    ready(ReadyRecordsRequest) -> ReadyRecordsResponse;
    open(OpenRecordRequest) -> RecordResponse;
    open_spec(OpenSpecRequest) -> RecordResponse;
    open_task(OpenTaskRequest) -> RecordResponse;
    open_wayfinder(OpenWayfinderRequest) -> RecordResponse;
    open_task_under_spec(OpenTaskUnderSpecRequest) -> RecordResponse;
    open_task_under_wayfinder(OpenTaskUnderWayfinderRequest) -> RecordResponse;
    open_grilling_under_wayfinder(OpenGrillingUnderWayfinderRequest) -> RecordResponse;
    list(ListRecordsRequest) -> ListRecordsResponse;
    show(RecordIdRequest) -> RecordResponse;
    edit(EditRecordRequest) -> RecordResponse;
    start(RecordIdRequest) -> RecordResponse;
    done(DoneRecordRequest) -> RecordResponse;
    relate(RelateRecordRequest) -> RecordResponse;
    unrelate(RelateRecordRequest) -> RecordResponse;
    comment(AddRecordCommentRequest) -> RecordCommentResponse;
    comments(RecordIdRequest) -> ListRecordCommentsResponse;
    updates(RecordIdRequest) -> ListRecordUpdatesResponse;
    comment_delete(DeleteRecordCommentRequest) -> ();
}
