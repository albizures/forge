//! Original-backend durable routing and operator retirement. The registry
//! contains only directly spawned children/private pipes, never adopted PIDs.
use super::runtime_directory;
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, RegistrationError,
    authorization_lease::{AuthorityHost, Owner},
    durable_execution::{
        Launch, Repository as _,
        retirement::{Operation, Receipt, Repository as _},
    },
};
use forge_runner_host::{SpawnedOwner, control::ControlPipe, inspection::ExclusiveInspection};
use forge_store::SqliteProjectRepository;
use std::{
    collections::HashMap,
    path::PathBuf,
    process::Child,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn uncertain() -> RegistrationError {
    RegistrationError::new(
        "durable-retirement-unverified",
        "owned execution cleanup is uncertain; retain resources and capacity",
    )
}
/// A specific retained-uncertainty code for a runtime this daemon does not own
/// and that has no reconcilable proof. Descriptive only; it releases nothing.
fn unproven(directory: &std::path::Path) -> RegistrationError {
    use forge_runner_host::fence::Diagnosis;
    match Diagnosis::of(directory) {
        Diagnosis::Unproven => RegistrationError::new(
            "durable-runtime-fence-unproven",
            "the original host fenced this runtime but has not proven that no descendant survives; capacity stays reserved. Investigate the runtime directory's watchdog diagnostics",
        ),
        _ if ExclusiveInspection::acquire(directory).is_err() => RegistrationError::new(
            "durable-runtime-owner-active",
            "an original runner host still holds this runtime's ownership lock; retirement completes after its lease expires and it proves quiescence",
        ),
        _ => uncertain(),
    }
}

struct Owned {
    child: Child,
    pipe: ControlPipe,
    owner: Owner,
    launch: Launch,
    sent: bool,
    fenced: bool,
}
#[derive(Default)]
pub struct Controller {
    owners: Mutex<HashMap<String, Arc<Mutex<Owned>>>>,
    /// Matched read-only components, selected per Launch by its component hash.
    catalog: Arc<super::launch::ComponentCatalog>,
}
impl Controller {
    /// Use the daemon's matched component catalog for reconciliation inspection.
    #[must_use]
    pub fn with_catalog(catalog: Arc<super::launch::ComponentCatalog>) -> Self {
        Self {
            owners: Mutex::default(),
            catalog,
        }
    }

    /// Configure explicitly selected matched read-only components. No credential
    /// or writable runner is needed; missing inspection blocks new reconciliation.
    /// # Errors
    /// Rejects relative component paths.
    pub fn with_inspection(
        mut self,
        components: super::operator::InspectionComponents,
    ) -> Result<Self, RegistrationError> {
        components.validate()?;
        Arc::make_mut(&mut self.catalog).push_inspection(components);
        Ok(self)
    }

    /// The matched inspection components for this Launch, or an explicit
    /// `durable-components-unavailable` refusal; never another Revision's code.
    fn inspection(
        &self,
        launch: &Launch,
    ) -> Result<super::operator::InspectionComponents, RegistrationError> {
        self.catalog.inspection_for(launch)?.ok_or_else(uncertain)
    }

    /// Transfer a directly spawned, ready supervisor after stopping its grant
    /// driver. The pipe is the original channel, retained from Authorization.
    /// # Errors
    /// Rejects unsafe ownership, duplicate registration or foreign runtime mapping.
    pub fn register(
        &self,
        repository: &SqliteProjectRepository,
        workspace: &str,
        launch: Launch,
        mut child: Child,
        pipe: ControlPipe,
    ) -> Result<(), RegistrationError> {
        let path = runtime_directory::mapped(repository, workspace, &launch, false)?;
        let owner = SpawnedOwner::capture(&mut child, &path)?.observe()?.owner;
        let mut owners = self.owners.lock().map_err(|_| uncertain())?;
        if owners.contains_key(&launch.assignment_id) {
            return Err(uncertain());
        }
        owners.insert(
            launch.assignment_id.clone(),
            Arc::new(Mutex::new(Owned {
                child,
                pipe,
                owner,
                launch,
                sent: false,
                fenced: false,
            })),
        );
        Ok(())
    }

    fn reconcile_proof(
        &self,
        repository: &SqliteProjectRepository,
        workspace: &str,
        directory: &std::path::Path,
        operation: &Operation,
        lease: &forge_app::authorization_lease::Lease,
    ) -> Result<bool, RegistrationError> {
        if lease.owner != operation.owner {
            return Err(uncertain());
        }
        let Ok(guard) = ExclusiveInspection::acquire(directory) else {
            return Ok(false);
        };
        match std::fs::symlink_metadata(directory.join("retirement-intent.json")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Ok(false),
        }
        // Every watchdog fence must be settled by its own host's quiescence
        // receipt, and this exact last lease needs crash or post-fence proof.
        if forge_runner_host::fence::settled(directory).is_err()
            || (forge_runner_host::crash::receipt(directory, lease).is_err()
                && forge_runner_host::fence::receipt(directory, lease).is_err())
        {
            return Ok(false);
        }
        let components = self.inspection(&operation.launch)?;
        super::effects::inspect_retirement(
            repository,
            workspace,
            &operation.launch,
            &components,
            directory,
            &guard,
        )?;
        guard.verify().map_err(|_| uncertain())?;
        // This is the trusted app reconciliation result of the original crash
        // proof, not a repaired/manufactured host retirement receipt on disk.
        repository.reconcile_retirement(
            workspace,
            &Receipt {
                version: 1,
                operation: operation.clone(),
                children_absent: true,
            },
        )?;
        Ok(true)
    }

    /// Called only after application's durable revocation. Retained receipt
    /// reconciliation after restart performs no process adoption or signalling.
    /// # Errors
    /// Missing/stale ownership, failed cancellation, surviving children, unsafe
    /// storage and failed commits all preserve the original pending operation.
    pub fn retire(
        &self,
        repository: &SqliteProjectRepository,
        workspace: &str,
        launch: &Launch,
    ) -> Result<(), RegistrationError> {
        let existing = repository.retirement(workspace, launch)?;
        if existing
            .as_ref()
            .is_some_and(|r| r.reconciled_at_millis.is_some())
        {
            return Ok(());
        }
        let directory = runtime_directory::mapped(repository, workspace, launch, false)?;
        let owned = self
            .owners
            .lock()
            .map_err(|_| uncertain())?
            .get(&launch.assignment_id)
            .cloned();
        let last = repository.revoked_lease(workspace, launch)?;
        let operation = if let Some(existing) = existing {
            existing.operation
        } else {
            let identity = if let Some(owned) = &owned {
                let owned = owned.lock().map_err(|_| uncertain())?;
                if owned.fenced || owned.launch != *launch {
                    return Err(uncertain());
                }
                owned.owner.clone()
            } else {
                last.as_ref().ok_or_else(uncertain)?.owner.clone()
            };
            let operation = Operation {
                operation_id: format!("{}-retire", launch.operation_id),
                launch: launch.clone(),
                owner: identity,
            };
            repository.begin_retirement(workspace, &operation)?;
            operation
        };
        if let Some(last) = last
            && self.reconcile_proof(repository, workspace, &directory, &operation, &last)?
        {
            return Ok(());
        }
        if let Some(owned) = &owned {
            let mut owned = owned.lock().map_err(|_| uncertain())?;
            if owned.fenced || owned.launch != *launch || owned.owner != operation.owner {
                return Err(uncertain());
            }
            if !owned.sent {
                let observation = SpawnedOwner::capture(&mut owned.child, &directory)
                    .and_then(|mut host| host.observe());
                if !observation.is_ok_and(|o| o.owner == owned.owner) {
                    owned.fenced = true;
                    return Err(uncertain());
                }
                // Mark before delivery: a partial write is never concatenated or
                // replayed. Restart can only inspect the original host receipt.
                owned.sent = true;
                if owned.pipe.retire(&operation).is_err() {
                    owned.fenced = true;
                    return Err(uncertain());
                }
            }
            let deadline = Instant::now() + Duration::from_secs(9);
            loop {
                match owned.child.try_wait() {
                    Ok(Some(status)) if status.success() => break,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    _ => return Err(uncertain()),
                }
            }
        }
        // A dead/absent host is not proof. Require protected exact receipt and an
        // exclusive lock continuously through the transactional result commit.
        let unowned = owned.is_none();
        let guard = ExclusiveInspection::acquire(&directory).map_err(|_| {
            if unowned {
                unproven(&directory)
            } else {
                uncertain()
            }
        })?;
        let receipt =
            forge_runner_host::retirement::receipt(&directory, &operation).map_err(|_| {
                if unowned {
                    unproven(&directory)
                } else {
                    uncertain()
                }
            })?;
        guard.verify().map_err(|_| uncertain())?;
        let components = self.inspection(launch)?;
        super::effects::inspect_retirement(
            repository,
            workspace,
            launch,
            &components,
            &directory,
            &guard,
        )?;
        guard.verify().map_err(|_| uncertain())?;
        repository.reconcile_retirement(workspace, &receipt)?;
        Ok(())
    }
}

/// Durable routing alongside the original Herdr/Grilling factory. New launches
/// require an explicitly composed matched launcher; whether one is *admitted* is
/// the release gate's decision, made before this factory is consulted. Historical
/// observation, recovery and retirement need no profile source or credentials.
pub struct Factory {
    pub database: PathBuf,
    pub controller: Arc<Controller>,
    /// Production launch composition; `None` keeps new durable launches refused.
    pub launcher: Option<Arc<super::launch::Launcher>>,
    /// Same-attempt recovery from exact crash proof; `None` refuses recovery.
    pub recovery: Option<Arc<super::recovery::Controller>>,
    pub original: Box<dyn AgentDispatchAdapterFactory>,
}
impl AgentDispatchAdapterFactory for Factory {
    fn validate_profile(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        if profile.provider == "pi-durable" {
            let launcher = self.launcher.as_ref().ok_or_else(disabled)?;
            return launcher
                .validate_profile(profile)
                .map_err(|error| super::launch::adapter_error(&error));
        }
        self.original.validate_profile(profile)
    }
    fn validate_preflight(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        if profile.provider == "pi-durable" {
            return self.validate_profile(profile);
        }
        self.original.validate_preflight(profile)
    }

    fn adapter_for(
        &self,
        profile: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        if profile.provider == "pi-durable" {
            return Ok(Box::new(Adapter {
                database: self.database.clone(),
                controller: self.controller.clone(),
                launcher: self.launcher.clone(),
                recovery: self.recovery.clone(),
            }));
        }
        self.original.adapter_for(profile)
    }
}
struct Adapter {
    database: PathBuf,
    controller: Arc<Controller>,
    launcher: Option<Arc<super::launch::Launcher>>,
    recovery: Option<Arc<super::recovery::Controller>>,
}
fn disabled() -> AgentAdapterError {
    AgentAdapterError::InvalidRequest(
        "durable production dispatch and recovery remain disabled".into(),
    )
}
impl Adapter {
    fn runtime(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<(SqliteProjectRepository, String, Launch), RegistrationError> {
        let repository = SqliteProjectRepository::open(&self.database)?;
        let workspace = repository.record_workspace(context.assignment.record_id)?;
        let runtime = repository
            .runtime(&workspace, &context.assignment.id)?
            .ok_or_else(uncertain)?;
        Ok((repository, workspace, runtime.launch))
    }
}
impl AgentDispatchAdapter for Adapter {
    /// Matched launch components are configured and pass launch preflight.
    /// This is not launch eligibility (the release gate decides that).
    fn probe(&self) -> Result<(), AgentAdapterError> {
        self.launcher
            .as_ref()
            .ok_or_else(disabled)?
            .preflight()
            .map_err(|error| super::launch::adapter_error(&error))
    }
    fn validate_provision(&self, context: &AgentDispatchContext) -> Result<(), AgentAdapterError> {
        let launcher = self.launcher.as_ref().ok_or_else(disabled)?;
        launcher
            .validate_provision(context)
            .map_err(|error| super::launch::adapter_error(&error))
    }
    fn provision(
        &self,
        context: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        let launcher = self.launcher.as_ref().ok_or_else(disabled)?;
        launcher
            .provision(context)
            .map_err(|error| super::launch::adapter_error(&error))
    }
    /// Only this daemon's directly supervised driver can report progress. A
    /// retained runtime without one is uncertainty, never absence or completion.
    fn observe(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentObservation, AgentAdapterError> {
        if let Some(launcher) = &self.launcher
            && let Some(state) = launcher.state(&context.assignment.id)
        {
            return Ok(match state {
                super::launch::DriverState::Live(observation) => observation,
                super::launch::DriverState::Crashed | super::launch::DriverState::Ended => {
                    AgentObservation::Unknown
                }
            });
        }
        self.runtime(context)
            .map(|_| AgentObservation::Unknown)
            .map_err(|_| {
                AgentAdapterError::Transient(
                    "durable runtime facts unavailable; retain original resources".into(),
                )
            })
    }
    fn recover(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentObservation, AgentAdapterError> {
        // A live first-launch driver owns its runtime; recovery never competes.
        if let Some(launcher) = &self.launcher {
            match launcher.state(&context.assignment.id) {
                Some(super::launch::DriverState::Live(observation)) => return Ok(observation),
                Some(super::launch::DriverState::Ended) => {
                    return Err(AgentAdapterError::Transient(
                        "durable launch fenced; retain original resources".into(),
                    ));
                }
                Some(super::launch::DriverState::Crashed) => {
                    launcher.release_crashed(&context.assignment.id);
                }
                None => {}
            }
        }
        let controller = self.recovery.as_ref().ok_or_else(disabled)?;
        let run = || {
            let (_, workspace, launch) = self.runtime(context)?;
            controller.recover(&workspace, &launch)
        };
        run().map_err(|error: RegistrationError| {
            AgentAdapterError::Transient(if error.code == "durable-components-unavailable" {
                format!("{}: {}", error.code, error.message)
            } else {
                "durable recovery unavailable; retain original resources".into()
            })
        })
    }
    fn retire(
        &self,
        context: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        let run = || {
            let (repository, workspace, launch) = self.runtime(context)?;
            if let Some(launcher) = &self.launcher {
                launcher.stop_for_retirement(&workspace, &launch)?;
            }
            if let Some(recovery) = &self.recovery {
                recovery.stop_for_retirement(&workspace, &launch)?;
            }
            self.controller.retire(&repository, &workspace, &launch)
        };
        run().map_err(|error| AgentAdapterError::InvalidRequest(error.code.into()))
    }
}
