use forge_app::Workspace;
use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceResponse {
    pub id: String,
    pub name: String,
    pub root: String,
}
impl From<Workspace> for WorkspaceResponse {
    fn from(value: Workspace) -> Self {
        Self {
            id: value.id,
            name: value.name,
            root: value.root.to_string_lossy().into_owned(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceCreateRequest {
    pub id: String,
    pub name: String,
    pub root: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceRenameRequest {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceIdRequest {
    pub id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceSelectRequest {
    pub workspace: Option<String>,
    /// Workspace qualifier supplied by a Project reference, not cwd inference.
    pub project_workspace: Option<String>,
    pub cwd: String,
}
#[rpc(client, server, namespace = "workspace")]
pub trait WorkspaceApi {
    #[method(name = "create")]
    async fn create(&self, request: WorkspaceCreateRequest) -> RpcResult<WorkspaceResponse>;
    #[method(name = "list")]
    async fn list(&self) -> RpcResult<Vec<WorkspaceResponse>>;
    #[method(name = "select")]
    async fn select(&self, request: WorkspaceSelectRequest) -> RpcResult<WorkspaceResponse>;
    #[method(name = "rename")]
    async fn rename(&self, request: WorkspaceRenameRequest) -> RpcResult<WorkspaceResponse>;
    #[method(name = "remove")]
    async fn remove(&self, request: WorkspaceIdRequest) -> RpcResult<()>;
}
