//! Shared JSON-RPC wire contract ownership boundary.

use forge_app::ApplicationStatus;
use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use serde::{Deserialize, Serialize};

/// The additive JSON-RPC method name for application readiness.
pub const SYSTEM_STATUS_METHOD: &str = "system.status";

/// The typed RPC contract shared by daemon clients and the daemon adapter.
#[rpc(client, server, namespace = "system")]
pub trait SystemApi {
    /// Returns the application readiness boundary without transport-owned data.
    #[method(name = "status")]
    async fn status(&self) -> RpcResult<SystemStatusResponse>;
}

/// The serializable response returned by [`SYSTEM_STATUS_METHOD`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SystemStatusResponse {
    /// The application name.
    pub name: String,
    /// The application version.
    pub version: String,
    /// Whether the daemon/application is ready to serve requests.
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

#[cfg(test)]
mod tests {
    use forge_app::Application;

    use super::{SYSTEM_STATUS_METHOD, SystemStatusResponse};

    #[test]
    fn system_status_response_preserves_the_application_result() {
        let response = SystemStatusResponse::from(Application::new().status());

        assert_eq!(SYSTEM_STATUS_METHOD, "system.status");
        assert_eq!(response.name, "forge");
        assert_eq!(response.version, "0.1.0");
        assert!(response.ready);
    }
}
