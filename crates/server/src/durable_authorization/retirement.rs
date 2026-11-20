//! Original-backend operator retirement. Production launch stays disabled. The
//! registry contains only directly spawned children/private pipes, never adopted PIDs.
use super::runtime_directory;
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, RegistrationError,
    authorization_lease::{AuthorityHost, Owner},
    durable_execution::{
        Launch, Repository as _,
        retirement::{Operation, Repository as _},
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
    inspection: Option<super::operator::InspectionComponents>,
}
impl Controller {
    /// Configure explicitly selected matched read-only components. No credential
    /// or writable runner is needed; missing inspection blocks new reconciliation.
    /// # Errors
    /// Rejects relative component paths.
    pub fn with_inspection(
        mut self,
        components: super::operator::InspectionComponents,
    ) -> Result<Self, RegistrationError> {
        components.validate()?;
        self.inspection = Some(components);
        Ok(self)
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
        let operation = if let Some(existing) = existing {
            existing.operation
        } else {
            let owned = owned.as_ref().ok_or_else(uncertain)?;
            let owned = owned.lock().map_err(|_| uncertain())?;
            if owned.fenced || owned.launch != *launch {
                return Err(uncertain());
            }
            let operation = Operation {
                operation_id: format!("{}-retire", launch.operation_id),
                launch: launch.clone(),
                owner: owned.owner.clone(),
            };
            repository.begin_retirement(workspace, &operation)?;
            operation
        };
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
        let guard = ExclusiveInspection::acquire(&directory).map_err(|_| uncertain())?;
        let receipt = forge_runner_host::retirement::receipt(&directory, &operation)
            .map_err(|_| uncertain())?;
        guard.verify().map_err(|_| uncertain())?;
        let components = self.inspection.as_ref().ok_or_else(uncertain)?;
        super::effects::inspect_retirement(
            repository, workspace, launch, components, &directory, &guard,
        )?;
        guard.verify().map_err(|_| uncertain())?;
        repository.reconcile_retirement(workspace, &receipt)?;
        Ok(())
    }
}

/// Cleanup-only durable routing alongside the original Herdr/Grilling factory.
/// No profile source/model credentials or matched writable runner are needed for
/// retained receipt inspection. New durable launches remain explicitly disabled.
pub struct Factory {
    pub database: PathBuf,
    pub controller: Arc<Controller>,
    pub original: Box<dyn AgentDispatchAdapterFactory>,
}
impl AgentDispatchAdapterFactory for Factory {
    fn validate_profile(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        if profile.provider == "pi-durable" {
            return Err(AgentAdapterError::InvalidRequest(
                "durable production dispatch remains disabled".into(),
            ));
        }
        self.original.validate_profile(profile)
    }
    fn validate_preflight(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        if profile.provider == "pi-durable" {
            return Err(disabled());
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
            }));
        }
        self.original.adapter_for(profile)
    }
}
struct Adapter {
    database: PathBuf,
    controller: Arc<Controller>,
}
fn disabled() -> AgentAdapterError {
    AgentAdapterError::InvalidRequest(
        "durable production dispatch and recovery remain disabled".into(),
    )
}
impl AgentDispatchAdapter for Adapter {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        Err(disabled())
    }
    fn provision(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        Err(disabled())
    }
    fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        Err(disabled())
    }
    fn recover(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        Err(disabled())
    }
    fn retire(
        &self,
        context: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        let run = || {
            let repository = SqliteProjectRepository::open(&self.database)?;
            let workspace = repository.record_workspace(context.assignment.record_id)?;
            let runtime = repository
                .runtime(&workspace, &context.assignment.id)?
                .ok_or_else(uncertain)?;
            self.controller
                .retire(&repository, &workspace, &runtime.launch)
        };
        run().map_err(|error| AgentAdapterError::InvalidRequest(error.code.into()))
    }
}
