//! Trusted dependency wiring for the existing protected Assignment bootstrap.
use forge_app::AgentDispatchAdapterFactory;
use std::path::PathBuf;

impl crate::Daemon {
    /// Set the same protected capability root used by launch delivery in a trusted
    /// embedded server composition. This is not an operator/Assignment RPC option.
    #[must_use]
    pub fn with_assignment_capability_root(mut self, root: PathBuf) -> Self {
        self.capability_secret_root = Some(root);
        self
    }
}

/// Wrap an explicitly selected provider with server-owned capability delivery.
/// This does not select/enable a backend, start execution or bypass provider
/// preflight. The endpoint and component paths belong to the trusted daemon
/// composition root, never an Assignment request or model input.
#[must_use]
pub fn adapter_factory(
    provider: Box<dyn AgentDispatchAdapterFactory>,
    daemon_url: String,
    capability_secret_root: PathBuf,
    cli_path: PathBuf,
) -> Box<dyn AgentDispatchAdapterFactory> {
    Box::new(super::ServerAgentDispatchAdapterFactory {
        provider,
        daemon_url,
        capability_secret_root,
        cli_override: Some(cli_path),
    })
}
