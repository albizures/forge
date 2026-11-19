//! New-launch durable profile/authentication composition. Not a dispatch adapter.
use forge_app::{AgentProfile, RegistrationError, durable_profiles::ExecutionConfiguration};
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

impl crate::Daemon {
    /// Configure only the explicit host credential-file path, never key bytes.
    /// Validation gates new durable preview/registration, not historical reads.
    #[must_use]
    pub fn with_durable_model_key_file(mut self, path: PathBuf) -> Self {
        self.durable_model_key_file = Some(path);
        self
    }

    pub(crate) fn preflight_profile_auth(
        &self,
        policy: &forge_app::agent_profiles::ResolvedLaunchPolicy,
    ) -> Result<(), RegistrationError> {
        if policy.runtime == "pi-durable" {
            let path = self.durable_model_key_file.as_deref().ok_or_else(|| {
                RegistrationError::new(
                    "durable-model-auth-unavailable",
                    "configure an explicit owner-only OpenAI API-key file",
                )
            })?;
            forge_runner_host::model_auth::preflight(path)?;
        }
        Ok(())
    }
}

/// Trusted host setup input. Never persist this with a transcript or expose it
/// as an agent-controlled setting. A prepared value neither issues a capability
/// nor starts a model request, tool, runtime store or Assignment.
pub struct PreparedProfile {
    pub configuration: ExecutionConfiguration,
    pub credential_file: PathBuf,
}

impl std::fmt::Debug for PreparedProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedProfile")
            .field("configuration", &self.configuration)
            .finish_non_exhaustive()
    }
}

/// Resolve an explicit registered Task revision and deliberately selected host
/// credential source. The trusted launch boundary must call this before issuing
/// capabilities, then revalidate mutable inputs at launch. Historical operations
/// must not call this. No ambient credential discovery or Herdr fallback exists.
/// # Errors
/// Rejects incompatible/mismatched policy, unavailable context/cwd and missing or
/// unprotected model credentials with redacted diagnostics.
pub fn prepare(
    profile: &AgentProfile,
    worktree: &Path,
    credential_file: Option<&Path>,
) -> Result<PreparedProfile, RegistrationError> {
    let configuration = profile
        .durable_execution_configuration(worktree)
        .map_err(|reason| RegistrationError::new("invalid-durable-profile", reason))?;
    let credential_file = credential_file.ok_or_else(|| {
        RegistrationError::new(
            "durable-model-auth-unavailable",
            "configure an explicit owner-only OpenAI API-key file",
        )
    })?;
    forge_runner_host::model_auth::preflight(credential_file)?;
    Ok(PreparedProfile {
        configuration,
        credential_file: credential_file.to_path_buf(),
    })
}
