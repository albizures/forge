use super::{Daemon, rpc_error};
use forge_rpc::{
    WorkspaceApiServer, WorkspaceCreateRequest, WorkspaceIdRequest, WorkspaceRenameRequest,
    WorkspaceResponse, WorkspaceSelectRequest,
};
use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use std::path::Path;

#[async_trait]
impl WorkspaceApiServer for Daemon {
    async fn create(&self, request: WorkspaceCreateRequest) -> RpcResult<WorkspaceResponse> {
        self.application
            .create_workspace(&request.id, &request.name, Path::new(&request.root))
            .map(Into::into)
            .map_err(rpc_error)
    }
    async fn list(&self) -> RpcResult<Vec<WorkspaceResponse>> {
        self.application
            .workspaces()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(rpc_error)
    }
    async fn select(&self, request: WorkspaceSelectRequest) -> RpcResult<WorkspaceResponse> {
        self.application
            .select_workspace(
                request.workspace.as_deref(),
                request.project_workspace.as_deref(),
                Path::new(&request.cwd),
            )
            .map(Into::into)
            .map_err(rpc_error)
    }
    async fn rename(&self, request: WorkspaceRenameRequest) -> RpcResult<WorkspaceResponse> {
        self.application
            .rename_workspace(&request.id, &request.name)
            .map(Into::into)
            .map_err(rpc_error)
    }
    async fn remove(&self, request: WorkspaceIdRequest) -> RpcResult<()> {
        self.application
            .remove_workspace(&request.id)
            .map_err(rpc_error)
    }
}
