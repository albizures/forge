//! Shared JSON-RPC wire contract ownership boundary.

use forge_app::{ApplicationStatus, Project, RegistrationError};
use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use serde::{Deserialize, Serialize};

pub const SYSTEM_STATUS_METHOD: &str = "system.status";
pub const PROJECT_REGISTER_METHOD: &str = "project.register";
pub const PROJECT_HERE_METHOD: &str = "project.here";

#[rpc(client, server, namespace = "system")]
pub trait SystemApi {
    #[method(name = "status")]
    async fn status(&self) -> RpcResult<SystemStatusResponse>;
}

#[rpc(client, server, namespace = "project")]
pub trait ProjectApi {
    /// Registers the supplied folder under its immutable slug.
    #[method(name = "register")]
    async fn register(&self, request: RegisterProjectRequest)
    -> RpcResult<RegisterProjectResponse>;

    /// Resolves the Project containing the supplied current folder.
    #[method(name = "here")]
    async fn here(&self, request: ProjectHereRequest) -> RpcResult<ProjectHereResponse>;
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

/// The client process current folder to resolve.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectHereRequest {
    pub folder: String,
}

/// The retained registration identity for the current Project.
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
