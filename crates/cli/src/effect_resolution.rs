//! Bounded operator RPC with explicit client Workspace selection and no agent secret.
use crate::RecordEndpoint;
use forge_rpc::effect_resolution::ResolutionReceipt;
use forge_rpc::{
    AgentDispatchApiClient, RecordSelection, ResolveInterruptedEffectRequest, SelectedRecordRequest,
};
use jsonrpsee::http_client::HttpClientBuilder;
use std::time::Duration;

/// Submit an evidence-based operator assertion; transport failure is unknown,
/// never permission to replay the command. Retry the exact operation/evidence.
/// # Errors
/// Returns routing, transport, ownership, confirmation or persistence failures.
pub async fn request_resolve_interrupted_effect(
    endpoint: impl Into<RecordEndpoint>,
    request: ResolveInterruptedEffectRequest,
) -> Result<ResolutionReceipt, String> {
    let endpoint = endpoint.into();
    let cwd = std::env::current_dir().map_err(|_| "could not resolve client cwd")?;
    // Inspector deadline is 30s plus bounded reap. Leave time for the durable
    // commit/reply; neither an HTTP timeout nor this deadline proves absence.
    let client = HttpClientBuilder::default()
        .request_timeout(Duration::from_secs(45))
        .build(&endpoint.url)
        .map_err(|_| "invalid Forge daemon endpoint")?;
    client.assignment_resolve_effect(SelectedRecordRequest {
        selection: RecordSelection { workspace: endpoint.workspace, cwd: Some(cwd.to_string_lossy().into_owned()) },
        record: request,
    }).await.map_err(|error| format!("could not resolve interrupted effect through Forge daemon: {error}; inspect retained audit and retry only the exact operation, never the interrupted command"))
}
