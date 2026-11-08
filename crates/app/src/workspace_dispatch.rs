//! Workspace-owned dispatch selections, separate from daemon transport and Project policy.
use crate::{
    AgentAdapterError, AgentAssignment, AgentDispatchAdapterFactory, AgentDispatchRepository,
    AgentProfile, Application, DispatchWorkspace, DispatcherConfigEntry, RecordType,
    RegistrationError,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchBlocker {
    pub code: String,
    pub message: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceDispatchReadiness {
    pub configuration: WorkspaceDispatchConfiguration,
    pub configured: bool,
    pub dispatchable: bool,
    pub blockers: Vec<DispatchBlocker>,
}
impl WorkspaceDispatchReadiness {
    pub(crate) fn require_dispatchable(&self) -> Result<(), RegistrationError> {
        if self.dispatchable {
            return Ok(());
        }
        Err(RegistrationError::new(
            "workspace-dispatch-blocked",
            self.blockers
                .iter()
                .map(|b| format!("{}: {}", b.code, b.message))
                .collect::<Vec<_>>()
                .join("; "),
        ))
    }
}

fn readiness<F: AgentDispatchAdapterFactory + ?Sized>(
    repository: &dyn AgentDispatchRepository,
    factory: &F,
    workspace: &str,
) -> Result<WorkspaceDispatchReadiness, RegistrationError> {
    let configuration = repository.workspace_dispatch_configuration(workspace)?;
    let mut blockers = Vec::new();
    for (kind, profile) in [
        ("task", &configuration.task_profile),
        ("grilling", &configuration.grilling_profile),
    ] {
        match profile {
            None => blockers.push(DispatchBlocker {
                code: format!("{kind}-profile-missing"),
                message: format!("set {kind}-profile to a valid revision in Workspace {workspace}"),
            }),
            Some(profile) if factory.validate_profile(profile).is_err() => {
                blockers.push(DispatchBlocker {
                    code: format!("{kind}-profile-invalid"),
                    message: format!(
                        "select a supported {kind} profile revision in Workspace {workspace}"
                    ),
                });
            }
            _ => (),
        }
    }
    if configuration.binding.is_none() {
        blockers.push(DispatchBlocker {
            code: "binding-missing".into(),
            message: "bind an existing Herdr Workspace by its exact identity".into(),
        });
    }
    let configured = blockers.is_empty();
    // Provider diagnostics are independent of configured completeness, never a prerequisite
    // for inspecting/changing local configuration or planning Records.
    let provider_profile = configuration
        .task_profile
        .as_ref()
        .filter(|p| factory.validate_profile(p).is_ok())
        .cloned()
        .unwrap_or(repository.agent_profile(RecordType::Task, 1)?);
    match factory.adapter_for(&provider_profile).and_then(|adapter| {
        adapter.probe()?;
        Ok(adapter)
    }) {
        Err(_) => blockers.push(DispatchBlocker {
            code: "provider-unreachable".into(),
            message: "configure the daemon Herdr socket and restore provider availability".into(),
        }),
        Ok(adapter) => {
            if let Some(binding) = &configuration.binding {
                match adapter.verify_workspace(&binding.id) {
                    Ok(found) if found.id == binding.id => {
                        if !configuration.provider_verified {
                            blockers.push(DispatchBlocker {
                                code: "binding-unverified".into(),
                                message: "socket changed; set workspace to the bound identity again to verify it at this endpoint".into(),
                            });
                        }
                    }
                    Err(AgentAdapterError::NotFound(_)) => blockers.push(DispatchBlocker {
                        code: "binding-missing-provider-identity".into(),
                        message: "the bound Herdr identity is missing; restore it or safely select another existing Workspace".into(),
                    }),
                    _ => blockers.push(DispatchBlocker {
                        code: "binding-unverifiable".into(),
                        message: "Herdr could not attest the exact bound identity; investigate the provider, not its labels".into(),
                    }),
                }
            }
        }
    }
    Ok(WorkspaceDispatchReadiness {
        configuration,
        configured,
        dispatchable: blockers.is_empty(),
        blockers,
    })
}

pub(crate) fn provisioning_available<F: AgentDispatchAdapterFactory + ?Sized>(
    repository: &dyn AgentDispatchRepository,
    factory: &F,
    assignment: &AgentAssignment,
) -> bool {
    (|| -> Result<bool, RegistrationError> {
        let owner = repository.show_record(assignment.record_id)?.workspace_id;
        let ready = readiness(repository, factory, &owner)?;
        if !ready.dispatchable
            || ready.configuration.binding
                != repository.assignment_dispatch_workspace(&assignment.id)?
        {
            return Ok(false);
        }
        // The attempt retains its original revision even if the operator changes selections.
        let Ok(adapter) = factory.adapter_for(&assignment.profile) else {
            return Ok(false);
        };
        let Some(binding) = repository.assignment_dispatch_workspace(&assignment.id)? else {
            return Ok(false);
        };
        Ok(adapter.probe().is_ok()
            && adapter
                .verify_workspace(&binding.id)
                .is_ok_and(|found| found.id == binding.id))
    })()
    .unwrap_or(false)
}

impl Application {
    /// Returns the effective selected immutable profile, without contacting the provider.
    #[allow(clippy::missing_errors_doc)]
    pub fn selected_agent_profile(
        &self,
        workspace: &str,
        kind: RecordType,
    ) -> Result<AgentProfile, RegistrationError> {
        let config = self
            .dispatch()?
            .workspace_dispatch_configuration(workspace)?;
        let profile = match kind {
            RecordType::Task => config.task_profile,
            RecordType::Grilling => config.grilling_profile,
            _ => {
                return Err(RegistrationError::new(
                    "invalid-profile-kind",
                    "select task or grilling",
                ));
            }
        };
        profile.ok_or_else(|| {
            RegistrationError::new(
                "profile-selection-required",
                "select a valid profile revision in this Workspace first",
            )
        })
    }
    /// Inspects completeness separately from current provider dispatchability.
    #[allow(clippy::missing_errors_doc)]
    pub fn workspace_dispatch_readiness(
        &self,
        workspace: &str,
    ) -> Result<WorkspaceDispatchReadiness, RegistrationError> {
        readiness(self.dispatch()?, self.adapter_factory()?, workspace)
    }

    /// Workspace configuration inspection is entirely local and works during provider outages.
    #[allow(clippy::missing_errors_doc)]
    pub fn workspace_dispatch_config_get(
        &self,
        workspace: &str,
        key: Option<&str>,
        project: Option<&str>,
    ) -> Result<Vec<DispatcherConfigEntry>, RegistrationError> {
        let config = self
            .dispatch()?
            .workspace_dispatch_configuration(workspace)?;
        let keys = key.map_or_else(
            || {
                let mut keys = vec![
                    "herdr-socket",
                    "workspace",
                    "task-profile",
                    "grilling-profile",
                    "acknowledgement-timeout-ms",
                ];
                if project.is_some() {
                    keys.push("project-limit");
                }
                keys
            },
            |key| vec![key],
        );
        keys.into_iter()
            .map(|key| {
                let (value, owner) = match key {
                    "herdr-socket" => (
                        self.dispatch()?
                            .herdr_socket_path()?
                            .map(|p| p.to_string_lossy().into_owned()),
                        "daemon".to_owned(),
                    ),
                    "workspace" => (
                        config.binding.as_ref().map(|b| b.id.clone()),
                        format!("workspace:{workspace}"),
                    ),
                    "task-profile" => (
                        config.task_profile.as_ref().map(|p| p.revision.to_string()),
                        format!("workspace:{workspace}"),
                    ),
                    "grilling-profile" => (
                        config
                            .grilling_profile
                            .as_ref()
                            .map(|p| p.revision.to_string()),
                        format!("workspace:{workspace}"),
                    ),
                    "acknowledgement-timeout-ms" => (
                        config
                            .task_profile
                            .as_ref()
                            .map(|p| p.acknowledgement_timeout_millis.to_string()),
                        "selected-task-profile-revision".into(),
                    ),
                    "project-limit" => {
                        let project = Self::dispatcher_config_project(project)?;
                        self.require_dispatch_project_owner(workspace, project)?;
                        (
                            Some(
                                self.dispatch()?
                                    .effective_project_task_concurrency_limit(project)?
                                    .to_string(),
                            ),
                            format!("project:{project}"),
                        )
                    }
                    _ => {
                        return Err(RegistrationError::new(
                            "unknown-dispatcher-config",
                            "unknown Dispatcher configuration key",
                        ));
                    }
                };
                let overridden = match key {
                    "acknowledgement-timeout-ms" => config
                        .task_profile
                        .as_ref()
                        .is_some_and(|p| p.acknowledgement_timeout_overridden),
                    "project-limit" => self
                        .dispatch()?
                        .project_task_concurrency_limit(Self::dispatcher_config_project(project)?)?
                        .is_some(),
                    _ => value.is_some(),
                };
                Ok(DispatcherConfigEntry {
                    key: key.into(),
                    ownership: owner,
                    provenance: if overridden { "override" } else { "default" }.into(),
                    value,
                })
            })
            .collect()
    }
    fn require_dispatch_project_owner(
        &self,
        workspace: &str,
        project: &str,
    ) -> Result<(), RegistrationError> {
        if self.dispatch()?.project(project)?.workspace_id != workspace {
            return Err(RegistrationError::new(
                "workspace-selection-conflict",
                "Project belongs to a different Workspace",
            ));
        }
        Ok(())
    }
    #[allow(clippy::missing_errors_doc)]
    pub fn workspace_dispatch_config_set(
        &self,
        workspace: &str,
        key: &str,
        value: &str,
        project: Option<&str>,
    ) -> Result<(), RegistrationError> {
        self.dispatch()?
            .workspace_dispatch_configuration(workspace)?;
        match key {
            "task-profile" | "grilling-profile" => {
                let revision = value
                    .parse::<u32>()
                    .ok()
                    .filter(|r| *r > 0)
                    .ok_or_else(|| {
                        RegistrationError::new(
                            "invalid-profile-revision",
                            "profile selection must be a positive revision",
                        )
                    })?;
                let kind = if key == "task-profile" {
                    RecordType::Task
                } else {
                    RecordType::Grilling
                };
                let profile = self.dispatch()?.agent_profile(kind, revision)?;
                self.adapter_factory()?
                    .validate_profile(&profile)
                    .map_err(|_| {
                        RegistrationError::new(
                            "invalid-profile-selection",
                            "selected profile is unsupported by this provider",
                        )
                    })?;
                self.dispatch()?
                    .select_workspace_profile(workspace, kind, Some(revision))
            }
            "workspace" => {
                let _guard = self.provider_configuration.lock().map_err(|_| {
                    RegistrationError::new(
                        "configuration-lock",
                        "provider configuration lock is poisoned",
                    )
                })?;
                if value.is_empty() || value.trim() != value {
                    return Err(RegistrationError::new(
                        "invalid-dispatch-workspace",
                        "supply an exact nonblank provider identity",
                    ));
                }
                let config = self
                    .dispatch()?
                    .workspace_dispatch_configuration(workspace)?;
                let profile = self.dispatch()?.agent_profile(RecordType::Task, 1)?;
                let adapter = self.adapter_factory()?.adapter_for(&profile).map_err(|_| {
                    RegistrationError::new("workspace-unavailable", "Herdr endpoint is unavailable")
                })?;
                let binding = adapter.verify_workspace(value).map_err(|_| {
                    RegistrationError::new(
                        "workspace-unavailable",
                        "could not verify the exact existing Herdr identity",
                    )
                })?;
                if binding.id != value {
                    return Err(RegistrationError::new(
                        "workspace-identity-mismatch",
                        "Herdr returned a different identity",
                    ));
                }
                self.dispatch()?.retain_verified_workspace_binding(
                    workspace,
                    Some(&binding),
                    config.provider_generation,
                )
            }
            "acknowledgement-timeout-ms" => {
                let timeout = value.parse::<u64>().map_err(|_| {
                    RegistrationError::new(
                        "invalid-acknowledgement-timeout",
                        "timeout must be a positive integer",
                    )
                })?;
                self.dispatch()?
                    .configure_workspace_acknowledgement_timeout(workspace, Some(timeout))
                    .map(|_| ())
            }
            "project-limit" => {
                let project = Self::dispatcher_config_project(project)?;
                self.require_dispatch_project_owner(workspace, project)?;
                let limit = value.parse::<u16>().map_err(|_| {
                    RegistrationError::new(
                        "invalid-project-limit",
                        "project-limit must be an integer from 1 through 1024",
                    )
                })?;
                self.set_project_task_concurrency_limit(project, Some(limit))
            }
            "herdr-socket" => self.set_herdr_socket_path(Some(std::path::Path::new(value))),
            _ => Err(RegistrationError::new(
                "unknown-dispatcher-config",
                "unknown Dispatcher configuration key",
            )),
        }
    }
    #[allow(clippy::missing_errors_doc)]
    pub fn workspace_dispatch_config_unset(
        &self,
        workspace: &str,
        key: &str,
        project: Option<&str>,
    ) -> Result<(), RegistrationError> {
        let config = self
            .dispatch()?
            .workspace_dispatch_configuration(workspace)?;
        match key {
            "task-profile" => {
                self.dispatch()?
                    .select_workspace_profile(workspace, RecordType::Task, None)
            }
            "grilling-profile" => {
                self.dispatch()?
                    .select_workspace_profile(workspace, RecordType::Grilling, None)
            }
            "workspace" => self.dispatch()?.retain_verified_workspace_binding(
                workspace,
                None,
                config.provider_generation,
            ),
            "acknowledgement-timeout-ms" => self
                .dispatch()?
                .configure_workspace_acknowledgement_timeout(workspace, None)
                .map(|_| ()),
            "project-limit" => {
                let project = Self::dispatcher_config_project(project)?;
                self.require_dispatch_project_owner(workspace, project)?;
                self.set_project_task_concurrency_limit(project, None)
            }
            "herdr-socket" => self.set_herdr_socket_path(None),
            _ => Err(RegistrationError::new(
                "unknown-dispatcher-config",
                "unknown Dispatcher configuration key",
            )),
        }
    }
}

/// Durable configuration facts, not a promise that the provider is currently reachable.
/// Callers must probe the provider and verify the bound identity before provisioning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceDispatchConfiguration {
    pub workspace_id: String,
    pub task_profile: Option<AgentProfile>,
    pub grilling_profile: Option<AgentProfile>,
    pub binding: Option<DispatchWorkspace>,
    /// Changes whenever the daemon-wide provider endpoint changes.
    pub provider_generation: u64,
    /// The binding was verified against this endpoint generation. Availability is not cached.
    pub provider_verified: bool,
}

/// Persistence boundary for Workspace-owned dispatch configuration.
/// Profile revisions are immutable; selections affect only future Assignments.
#[allow(clippy::missing_errors_doc)]
pub trait WorkspaceDispatchRepository: Send + Sync {
    fn workspace_dispatch_configuration(
        &self,
        workspace: &str,
    ) -> Result<WorkspaceDispatchConfiguration, RegistrationError>;

    fn select_workspace_profile(
        &self,
        workspace: &str,
        record_type: RecordType,
        revision: Option<u32>,
    ) -> Result<(), RegistrationError>;

    /// Clones the selected Task profile into a new immutable revision and selects it only here.
    /// None restores the profile timeout default, not a separate Workspace override.
    fn configure_workspace_acknowledgement_timeout(
        &self,
        workspace: &str,
        timeout_millis: Option<u64>,
    ) -> Result<AgentProfile, RegistrationError>;

    /// The caller must verify the exact provider identity at the observed endpoint generation.
    /// Generation comparison prevents retaining verification after a concurrent socket change.
    /// Passing None removes the binding without contacting the provider.
    fn retain_verified_workspace_binding(
        &self,
        workspace: &str,
        binding: Option<&DispatchWorkspace>,
        provider_generation: u64,
    ) -> Result<(), RegistrationError>;
}
