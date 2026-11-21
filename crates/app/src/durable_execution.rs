//! Forge-owned durable admission. Runtime completion is never a Check outcome.
use crate::RegistrationError;
use serde::{Deserialize, Serialize};

pub mod bundle;
pub mod completion;
pub mod effect_resolution;
pub mod effects;
pub mod followup;
pub mod launch_control;
pub mod purge;
mod reconciliation;
pub mod recovery;
pub mod retirement;
pub mod status;

/// Immutable identities only: no capability, provider credential or prompt content.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub assignment_id: String,
    pub operation_id: String,
    pub attempt_id: String,
    pub profile_revision: u32,
    pub runtime_id: String,
    pub incarnation_id: String,
    pub conversation_id: String,
    pub store_id: String,
    pub forge_revision: String,
    pub component_hash: String,
    pub runtime_schema: u32,
}

pub const RUNTIME_SCHEMA_VERSION: u32 = 2;

impl Launch {
    /// Validate the initial pinned runtime identity, never manufacture missing values.
    /// # Errors
    /// Rejects empty identities and unsupported runtime stores before reserving intent.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        if [
            &self.assignment_id,
            &self.operation_id,
            &self.attempt_id,
            &self.runtime_id,
            &self.incarnation_id,
            &self.conversation_id,
            &self.store_id,
            &self.forge_revision,
            &self.component_hash,
        ]
        .iter()
        .any(|s| s.trim().is_empty())
            || self.profile_revision == 0
        {
            return Err(RegistrationError::new(
                "durable-identity-invalid",
                "durable execution requires explicit immutable identities",
            ));
        }
        if self.runtime_schema != RUNTIME_SCHEMA_VERSION {
            return Err(RegistrationError::new(
                "runner-schema-incompatible",
                "expected runtime schema 2; no automatic conversion",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Admission {
    Pending,
    Unknown,
    Accepted { admission_id: String },
    Rejected { code: String },
}

impl Admission {
    /// Keep terminal results immutable; uncertainty can only resolve after send intent.
    /// # Errors
    /// Rejects conflicting results and invalid or empty adapter responses.
    pub fn reconcile(&self, result: &Self) -> Result<Self, RegistrationError> {
        if matches!(self, Self::Accepted { .. } | Self::Rejected { .. }) {
            return if self == result {
                Ok(self.clone())
            } else {
                Err(RegistrationError::new(
                    "durable-result-conflict",
                    "durable admission result conflicts with the accepted result",
                ))
            };
        }
        let invalid = self != &Self::Unknown
            || matches!(result, Self::Pending)
            || matches!(result, Self::Accepted { admission_id } if admission_id.trim().is_empty())
            || matches!(result, Self::Rejected { code } if code.trim().is_empty());
        if invalid {
            return Err(RegistrationError::new(
                "durable-result-invalid",
                "durable admission result is not a valid reconciliation",
            ));
        }
        Ok(result.clone())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub backend: Backend,
    pub launch: Launch,
    pub admission: Admission,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    PiDurable,
}

/// Trusted application/store boundary, not an agent credential surface.
/// Every mutation authorizes the Workspace and immutable binding before replay.
pub trait Repository: Send + Sync {
    /// # Errors
    /// Rejects unauthorized scope, conflicting identity and storage failures.
    fn reserve(&self, workspace: &str, launch: &Launch) -> Result<Runtime, RegistrationError>;
    /// Atomically marks intent unknown *before* a potentially effective external call.
    /// Returns true only to the single initial sender. Unknown calls require observation.
    /// # Errors
    /// Rejects stale/revoked bindings, missing intent, conflicts and storage failures.
    fn begin_send(&self, workspace: &str, launch: &Launch) -> Result<bool, RegistrationError>;
    /// # Errors
    /// Rejects unauthorized scope, conflicting results and storage failures.
    fn record_result(
        &self,
        workspace: &str,
        launch: &Launch,
        result: &Admission,
    ) -> Result<Runtime, RegistrationError>;
    /// Read persisted identity without starting execution.
    /// # Errors
    /// Rejects unauthorized scope and unreadable storage.
    fn runtime(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<Option<Runtime>, RegistrationError>;
}

/// Version negotiation and host ownership precede admission in the eventual adapter.
/// An observation must never open writable storage or start execution.
pub trait Transport {
    /// # Errors
    /// Returns compatibility, ownership or transport failures, leaving local intent unknown.
    fn admit(&self, launch: &Launch) -> Result<Admission, RegistrationError>;
    /// # Errors
    /// Returns read-only observation failures without inferring admission or absence.
    fn observe_admission(&self, launch: &Launch) -> Result<Admission, RegistrationError>;
}

/// Persist intent, cross the external boundary, then revalidate on result commit.
/// Transport errors deliberately leave durable Unknown, including a dropped reply.
/// # Errors
/// Returns scope, binding, storage or transport failures without releasing capacity.
pub fn admit(
    repository: &dyn Repository,
    transport: &dyn Transport,
    workspace: &str,
    launch: &Launch,
) -> Result<Runtime, RegistrationError> {
    let runtime = repository.reserve(workspace, launch)?;
    if matches!(
        runtime.admission,
        Admission::Accepted { .. } | Admission::Rejected { .. }
    ) {
        return Ok(runtime);
    }
    let result =
        if runtime.admission == Admission::Pending && repository.begin_send(workspace, launch)? {
            transport.admit(launch)?
        } else {
            transport.observe_admission(launch)?
        };
    repository.record_result(workspace, launch, &result)
}
