//! Operator adaptation only. Runtime/tool execution and recovery stay disabled.
use super::effects::resolve_exclusive;
use forge_app::{
    RegistrationError,
    durable_execution::{
        Repository as _,
        effect_resolution::{Repository as _, ResolutionReceipt},
    },
};
use forge_rpc::ResolveInterruptedEffectRequest;
use forge_store::SqliteProjectRepository;
use std::path::{Path, PathBuf};

/// Trusted daemon configuration, never RPC-selected paths or PATH discovery.
/// The inspector independently verifies its component hash against the immutable
/// retained runtime. Historical inspection needs no model key/profile source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectionComponents {
    pub node: PathBuf,
    pub inspector: PathBuf,
}

impl InspectionComponents {
    /// Select both explicitly configured components or neither; never fill a
    /// partial selection from PATH or the current checkout.
    /// # Errors
    /// Rejects partial or relative configuration.
    pub fn select(
        node: Option<PathBuf>,
        inspector: Option<PathBuf>,
    ) -> Result<Option<Self>, RegistrationError> {
        match (node, inspector) {
            (None, None) => Ok(None),
            (Some(node), Some(inspector)) => {
                let components = Self { node, inspector };
                components.validate()?;
                Ok(Some(components))
            }
            _ => Err(RegistrationError::new(
                "durable-effect-inspector-invalid",
                "configure both FORGE_DURABLE_NODE and FORGE_DURABLE_EFFECT_INSPECTOR, or neither",
            )),
        }
    }

    /// # Errors
    /// Rejects partial or relative selection. Missing/incompatible files fail
    /// closed at request time, without resolving an effect or enabling recovery.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        if !self.node.is_absolute() || !self.inspector.is_absolute() {
            return Err(RegistrationError::new(
                "durable-effect-inspector-invalid",
                "select explicit absolute Node and matched effect-inspector paths on the daemon host",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "operator_tests.rs"]
mod tests;

pub(crate) struct OperatorService {
    repository: SqliteProjectRepository,
    components: Option<InspectionComponents>,
}

impl OperatorService {
    pub(crate) fn new(
        store: &Path,
        components: Option<InspectionComponents>,
    ) -> Result<Self, RegistrationError> {
        if let Some(components) = &components {
            components.validate()?;
        }
        Ok(Self {
            repository: SqliteProjectRepository::open(store)?,
            components,
        })
    }

    /// Persisted durable facts only; scope is authorized before any read.
    pub(crate) fn status(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<Option<forge_app::durable_execution::status::Status>, RegistrationError> {
        forge_app::durable_execution::status::Repository::durable_status(
            &self.repository,
            workspace,
            assignment,
        )
    }

    pub(crate) fn followups(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<Vec<forge_app::durable_execution::followup::Receipt>, RegistrationError> {
        forge_app::durable_execution::followup::Repository::followups(
            &self.repository,
            workspace,
            assignment,
        )
    }

    /// Record only. No runner, model, tool call, lease or storage open occurs here.
    pub(crate) fn follow_up(
        &self,
        workspace: &str,
        request: &forge_app::durable_execution::followup::Request,
    ) -> Result<forge_app::durable_execution::followup::Receipt, RegistrationError> {
        forge_app::durable_execution::followup::Repository::record_followup(
            &self.repository,
            workspace,
            request,
        )
    }

    /// Delete retired runtime history after Forge authorizes it. Needs no runtime
    /// components, profile or credential and never starts a runner.
    pub(crate) fn purge_runtime(
        &self,
        workspace: &str,
        request: &forge_app::durable_execution::purge::Request,
    ) -> Result<forge_app::durable_execution::purge::Record, RegistrationError> {
        request.validate()?;
        // Scope is authorized by the repository before any binding is read.
        forge_app::durable_execution::purge::Repository::purge_record(
            &self.repository,
            workspace,
            &request.assignment_id,
        )?;
        let runtime = self
            .repository
            .runtime(workspace, &request.assignment_id)?
            .ok_or_else(|| {
                RegistrationError::new(
                    "durable-intent-missing",
                    "Assignment has no retained durable runtime",
                )
            })?;
        super::purge::run(
            &self.repository,
            workspace,
            &runtime.launch,
            &request.operation_id,
        )
    }

    pub(crate) fn purge_record(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<Option<forge_app::durable_execution::purge::Record>, RegistrationError> {
        forge_app::durable_execution::purge::Repository::purge_record(
            &self.repository,
            workspace,
            assignment,
        )
    }

    /// Read one committed page. Needs trusted matched components but never the
    /// host lock, so it works beside a live writer and for stopped runners.
    pub(crate) async fn transcript(
        &self,
        workspace: &str,
        request: &forge_rpc::AssignmentTranscriptRequest,
    ) -> Result<forge_rpc::AssignmentTranscriptResponse, RegistrationError> {
        request.validate()?;
        let runtime = self
            .repository
            .runtime(workspace, &request.assignment_id)?
            .ok_or_else(|| {
                RegistrationError::new(
                    "durable-intent-missing",
                    "Assignment has no retained durable runtime",
                )
            })?;
        let components = self.components.as_ref().ok_or_else(|| {
            RegistrationError::new(
                "durable-transcript-unconfigured",
                "configure trusted matched inspection components on the daemon to read transcripts",
            )
        })?;
        super::transcript::page(
            &self.repository,
            workspace,
            &runtime.launch,
            &components.node,
            &components.inspector,
            request,
        )
        .await
    }

    pub(crate) async fn resolve(
        &self,
        workspace: &str,
        request: &ResolveInterruptedEffectRequest,
    ) -> Result<ResolutionReceipt, RegistrationError> {
        // Canonical immutable Launch comes from owning storage, never a caller's
        // desired profile, path or component. Scope is authorized before reads/replay.
        let runtime = self
            .repository
            .runtime(workspace, &request.assignment_id)?
            .ok_or_else(|| {
                RegistrationError::new(
                    "durable-intent-missing",
                    "Assignment has no retained durable runtime",
                )
            })?;
        // Returning an accepted receipt is a read, not a new safety assertion.
        // Lost-reply retries must survive absent runtime components/storage or a
        // current lock owner. Fresh acceptance still requires exclusive inspection.
        if let Some(receipt) = self.repository.accepted_effect_resolution(
            workspace,
            &runtime.launch,
            &request.resolution,
        )? {
            return Ok(receipt);
        }
        let components = self.components.as_ref().ok_or_else(|| RegistrationError::new(
            "durable-effect-inspector-unconfigured", "configure trusted matched effect inspection components on the daemon; preserve uncertainty and do not replay",
        ))?;
        resolve_exclusive(
            &self.repository,
            workspace,
            &runtime.launch,
            &components.node,
            &components.inspector,
            &request.resolution,
        )
        .await
    }
}
