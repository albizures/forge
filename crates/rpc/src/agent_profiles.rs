//! Redacted operator view; never serialize the persisted launch policy on this surface.
use forge_app::{
    AgentProfile,
    agent_profiles::{DirectResource, ResolvedLaunchPolicy},
};
use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfileDocumentRequest {
    pub workspace: String,
    pub document: String,
    pub record_type: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentProfileResourceResponse {
    pub name: String,
    pub path: String,
    pub sha256: String,
}
impl From<&DirectResource> for AgentProfileResourceResponse {
    fn from(value: &DirectResource) -> Self {
        Self {
            name: value.name.clone(),
            path: value.path.to_string_lossy().into_owned(),
            sha256: value.sha256.clone(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentProfilePolicyResponse {
    pub version: u32,
    pub runtime: String,
    pub model: String,
    pub tools: Vec<String>,
    pub skills: Vec<AgentProfileResourceResponse>,
    pub extensions: Vec<AgentProfileResourceResponse>,
    pub mcp_servers: Vec<AgentProfileResourceResponse>,
}
impl From<&ResolvedLaunchPolicy> for AgentProfilePolicyResponse {
    fn from(value: &ResolvedLaunchPolicy) -> Self {
        Self {
            version: value.version,
            runtime: value.runtime.clone(),
            model: value.model.clone(),
            tools: value.tools.clone(),
            skills: value.skills.iter().map(Into::into).collect(),
            extensions: value.extensions.iter().map(Into::into).collect(),
            mcp_servers: value
                .mcp_servers
                .iter()
                .map(|entry| AgentProfileResourceResponse {
                    name: entry.name.clone(),
                    path: entry.path.to_string_lossy().into_owned(),
                    sha256: entry.sha256.clone(),
                })
                .collect(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentProfileLifecycleResponse {
    pub retry_limit: u32,
    pub retry_backoff_millis: u64,
    pub startup_timeout_millis: u64,
    pub acknowledgement_timeout_millis: u64,
    pub unknown_state_grace_millis: u64,
}
impl From<&AgentProfile> for AgentProfileLifecycleResponse {
    fn from(value: &AgentProfile) -> Self {
        Self {
            retry_limit: value.retry_limit,
            retry_backoff_millis: value.retry_backoff_millis,
            startup_timeout_millis: value.startup_timeout_millis,
            acknowledgement_timeout_millis: value.acknowledgement_timeout_millis,
            unknown_state_grace_millis: value.unknown_state_grace_millis,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentProfileDocumentResponse {
    pub schema_version: u32,
    pub workspace: String,
    pub record_type: String,
    /// Only registration creates a revision. It does not select it.
    pub revision: Option<u32>,
    pub policy: AgentProfilePolicyResponse,
    pub lifecycle: AgentProfileLifecycleResponse,
    pub fingerprint_coverage: String,
    pub limitations: Vec<String>,
}
impl AgentProfileDocumentResponse {
    #[must_use]
    pub fn new(
        workspace: String,
        record_type: String,
        revision: Option<u32>,
        policy: &ResolvedLaunchPolicy,
        lifecycle: &AgentProfile,
    ) -> Self {
        Self {
            schema_version: 1, workspace, record_type, revision, policy: policy.into(), lifecycle: lifecycle.into(),
            fingerprint_coverage: "SHA-256 of selected SKILL.md files, extension entry files and persisted MCP launch configuration only; companion behavior is persisted, not revalidated.".into(),
            limitations: vec![
                "No recursive dependency pinning or atomic validation-to-runtime-load filesystem snapshot.".into(),
                "External services, models and credentials are not frozen; registration does not select or enable dispatch.".into(),
                "No OS/same-user isolation. Selected extensions are trusted unsandboxed code. MCP configuration and behavior are omitted from this view.".into(),
                "Pi flags configure launch but do not attest runtime identity or closed enforcement; native reload and ambient user settings are not guarded.".into(),
                "Selected MCP servers are unsupported at launch. Empty lists and the tool allowlist configure Pi flags, not a sandbox for trusted extensions.".into(),
            ],
        }
    }
}
#[rpc(client, server, namespace = "agent_profile")]
pub trait AgentProfileApi {
    #[method(name = "preview")]
    async fn preview(
        &self,
        request: AgentProfileDocumentRequest,
    ) -> RpcResult<AgentProfileDocumentResponse>;
    #[method(name = "register")]
    async fn register(
        &self,
        request: AgentProfileDocumentRequest,
    ) -> RpcResult<AgentProfileDocumentResponse>;
}
