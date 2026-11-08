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
requests! {
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
