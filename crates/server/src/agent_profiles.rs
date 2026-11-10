use super::{Daemon, rpc_error};
use forge_app::{RecordType, RegistrationError};
use forge_rpc::{AgentProfileApiServer, AgentProfileDocumentRequest, AgentProfileDocumentResponse};
use jsonrpsee::core::{RpcResult, async_trait};
use std::path::Path;

fn kind(value: &str) -> RpcResult<RecordType> {
    match value {
        "task" => Ok(RecordType::Task),
        "grilling" => Ok(RecordType::Grilling),
        _ => Err(rpc_error(RegistrationError::new(
            "invalid-profile-kind",
            "select task or grilling",
        ))),
    }
}
impl Daemon {
    /// Override the operator catalog root (e.g. an isolated fixture or service account).
    #[must_use]
    pub fn with_profile_operator_root(mut self, root: std::path::PathBuf) -> Self {
        self.profile_operator_root = Some(root);
        self
    }
    fn profile_operator_root(&self) -> RpcResult<&Path> {
        self.profile_operator_root
            .as_deref()
            .filter(|root| root.is_absolute())
            .ok_or_else(|| {
                rpc_error(RegistrationError::new(
                    "operator-catalog-unavailable",
                    "operator home catalog root must be absolute and available",
                ))
            })
    }
}
#[async_trait]
impl AgentProfileApiServer for Daemon {
    async fn preview(
        &self,
        request: AgentProfileDocumentRequest,
    ) -> RpcResult<AgentProfileDocumentResponse> {
        let (policy, lifecycle) = self
            .application
            .preview_agent_profile(
                &request.workspace,
                self.profile_operator_root()?,
                Path::new(&request.document),
                kind(&request.record_type)?,
            )
            .map_err(rpc_error)?;
        Ok(AgentProfileDocumentResponse::new(
            request.workspace,
            request.record_type,
            None,
            &policy,
            &lifecycle,
        ))
    }
    async fn register(
        &self,
        request: AgentProfileDocumentRequest,
    ) -> RpcResult<AgentProfileDocumentResponse> {
        let profile = self
            .application
            .register_agent_profile(
                &request.workspace,
                self.profile_operator_root()?,
                Path::new(&request.document),
                kind(&request.record_type)?,
            )
            .map_err(rpc_error)?;
        // A registered revision always has a launch policy. Do not serialize its credentials/behavior.
        let policy = profile.launch_policy.as_ref().ok_or_else(|| {
            rpc_error(RegistrationError::new(
                "profile-policy-missing",
                "registered launch policy unavailable",
            ))
        })?;
        Ok(AgentProfileDocumentResponse::new(
            request.workspace,
            request.record_type,
            Some(profile.revision),
            policy,
            &profile,
        ))
    }
}
