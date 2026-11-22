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

    /// Attach the operator service over the embedded server's retained store, as
    /// production `serve` does. Grants no launch eligibility.
    ///
    /// # Errors
    /// Returns store-open failures.
    pub fn with_durable_operator_store(
        self,
        store: &std::path::Path,
    ) -> Result<Self, forge_app::RegistrationError> {
        self.with_durable_operator_catalog(
            store,
            crate::durable_authorization::launch::ComponentCatalog::default(),
        )
    }

    /// As [`Self::with_durable_operator_store`], with the daemon's matched
    /// component catalog, so transcripts and effect resolution of retained
    /// Launches use their original components, as `serve` composes them.
    ///
    /// # Errors
    /// Returns store-open failures.
    pub fn with_durable_operator_catalog(
        mut self,
        store: &std::path::Path,
        catalog: crate::durable_authorization::launch::ComponentCatalog,
    ) -> Result<Self, forge_app::RegistrationError> {
        self.durable_effect_resolution = Some(std::sync::Arc::new(
            crate::durable_authorization::operator::OperatorService::new(store, catalog)?,
        ));
        Ok(self)
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
