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
        _ => RegistrationError::new(
            "durable-runtime-proof-unmatched",
            "no live owner holds this runtime, but no host proof matches its recorded owner and Launch; capacity stays reserved. Investigate the runtime directory's receipts",
        ),
    }
}

/// Reap a directly spawned host on a dedicated thread, however long it takes to
/// prove its own quiescence and exit. Never signals it and never drops it live.
pub(super) fn reap_detached(mut child: Child) {
    drop(child.stdin.take());
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

/// A never-granted launch whose host left ownership evidence without proof.
fn ungranted_unproven(directory: &std::path::Path) -> RegistrationError {
    match unproven(directory) {
        error if error.code == "durable-runtime-proof-unmatched" => RegistrationError::new(
            "durable-launch-ungranted-unproven",
            "the failed launch's host never received a grant but has not proven that its worker and descendants are gone; capacity stays reserved. Investigate the runtime directory's watchdog diagnostics",
        ),
        error => error,
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
    /// A rejected handoff never drops a live host: the private pipe closes (the
    /// host fences and proves quiescence itself) and the Child is reaped (T26).
    pub fn register(
        &self,
        repository: &SqliteProjectRepository,
        workspace: &str,
        launch: Launch,
        mut child: Child,
        pipe: ControlPipe,
    ) -> Result<(), RegistrationError> {
        let owner = runtime_directory::mapped(repository, workspace, &launch, false)
            .and_then(|path| Ok(SpawnedOwner::capture(&mut child, &path)?.observe()?.owner));
        let owner = match owner {
            Ok(owner) => owner,
            Err(error) => {
                drop(pipe);
                reap_detached(child);
                return Err(error);
            }
        };
        let Ok(mut owners) = self.owners.lock() else {
            drop(pipe);
            reap_detached(child);
            return Err(uncertain());
        };
        if owners.contains_key(&launch.assignment_id) {
            drop(owners);
            drop(pipe);
            reap_detached(child);
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
        // receipt, and the last lease's owner needs crash, post-fence or
        // ungranted proof: for this lease, or for the earlier lease that host
        // last accepted when a later commit was never delivered (T18 F1).
        if forge_runner_host::fence::settled(directory).is_err()
            || forge_runner_host::quiescence::predecessor_proof(directory, lease).is_err()
        {
            return Ok(false);
        }
        inspect_effects(
            repository,
            workspace,
            &operation.launch,
            || self.inspection(&operation.launch),
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

    /// Host-written proof for the recorded operation without a live handoff:
    /// with a committed grant, the last lease owner's crash, fence or
    /// ungranted proof (`reconcile_proof`); without one, for an unowned host,
    /// its exact ungranted receipt (T26) — a host whose retirement request was
    /// lost with its daemon fenced on the closed pipe instead.
    fn reconcile_recorded(
        &self,
        repository: &SqliteProjectRepository,
        workspace: &str,
        directory: &std::path::Path,
        operation: &Operation,
        last: Option<&forge_app::authorization_lease::Lease>,
        unowned: bool,
    ) -> Result<bool, RegistrationError> {
        match last {
            Some(last) => self.reconcile_proof(repository, workspace, directory, operation, last),
            None if unowned => {
                reconcile_ungranted_owner(repository, workspace, directory, operation)
            }
            None => Ok(false),
        }
    }
}

/// A host that never accepted any grant, under the operation recorded for
/// it: its exact ungranted receipt, bound to the same owner, every fence
/// settled and no host retirement in flight.
fn reconcile_ungranted_owner(
    repository: &SqliteProjectRepository,
    workspace: &str,
    directory: &std::path::Path,
    operation: &Operation,
) -> Result<bool, RegistrationError> {
    let Ok(guard) = ExclusiveInspection::acquire(directory) else {
        return Ok(false);
    };
    match std::fs::symlink_metadata(directory.join("retirement-intent.json")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Ok(false),
    }
    if forge_runner_host::fence::ungranted_receipt_for(directory, &operation.owner).is_err()
        || forge_runner_host::fence::settled(directory).is_err()
    {
        return Ok(false);
    }
    guard.verify().map_err(|_| uncertain())?;
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

/// T26: retire a launch that never received a grant and that no live host
/// of this daemon still holds. Proof, strongest first:
/// - no launch intent was committed, so nothing was ever spawned;
/// - the original host's exact ungranted quiescence receipt;
/// - a sealed directory that no host ever owned (and never can).
///
/// Otherwise a specific code keeps the Assignment visibly blocked.
fn reconcile_ungranted(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
) -> Result<(), RegistrationError> {
    use forge_app::{
        AgentDispatchRepository as _,
        durable_execution::launch_failure::{Proof, Repository as _},
    };
    if repository
        .agent_launch_intent(&launch.assignment_id)?
        .is_none()
    {
        // The store proves no production launch spawned a host. Defense in
        // depth: any prepared storage must also seal as never owned, so a host
        // that exists anyway (any path outside the launch intent) blocks this.
        let sealed = match runtime_directory::mapped(repository, workspace, launch, false) {
            Ok(directory) => Some(
                forge_runner_host::seal::seal(&directory)
                    .map_err(|_| ungranted_unproven(&directory))?,
            ),
            Err(_) => None,
        };
        repository.reconcile_ungranted_launch(workspace, launch, Proof::NeverLaunched)?;
        drop(sealed);
        return Ok(());
    }
    let directory = runtime_directory::mapped(repository, workspace, launch, false)?;
    if let Ok(guard) = ExclusiveInspection::acquire(&directory)
        && std::fs::symlink_metadata(directory.join("owner.json")).is_ok()
    {
        let proven = forge_runner_host::fence::ungranted_receipt(&directory).is_ok()
            && forge_runner_host::fence::settled(&directory).is_ok();
        if !proven {
            // Release the inspection lock first: the diagnosis probes it.
            drop(guard);
            return Err(ungranted_unproven(&directory));
        }
        guard.verify().map_err(|_| uncertain())?;
        return repository.reconcile_ungranted_launch(
            workspace,
            launch,
            Proof::UngrantedQuiescence,
        );
    }
    match forge_runner_host::seal::seal(&directory) {
        Ok(guard) => {
            repository.reconcile_ungranted_launch(workspace, launch, Proof::NeverOwned)?;
            drop(guard);
            Ok(())
        }
        Err(_) => Err(ungranted_unproven(&directory)),
    }
}

/// Committed interrupted-effect inspection before reconciliation. A launch
/// whose admission was never sent and that has no runtime store has nothing to
/// inspect: the same fresh-storage check gates its first grant (T26).
fn inspect_effects(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    components: impl FnOnce() -> Result<super::operator::InspectionComponents, RegistrationError>,
    directory: &std::path::Path,
    guard: &ExclusiveInspection,
) -> Result<(), RegistrationError> {
    if !super::effect_admission::has_database(directory)?
        && repository
            .runtime(workspace, &launch.assignment_id)?
            .is_some_and(|runtime| {
                runtime.admission == forge_app::durable_execution::Admission::Pending
            })
    {
        return super::effect_admission::require_fresh(repository, workspace, launch, directory);
    }
    super::effects::inspect_retirement(
        repository,
        workspace,
        launch,
        &components()?,
        directory,
        guard,
    )
}

/// Journal a new retirement operation for the directly spawned host this
/// daemon holds, or else for the owner of the last committed grant.
fn begin(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    owned: Option<&Arc<Mutex<Owned>>>,
    last: Option<&forge_app::authorization_lease::Lease>,
) -> Result<Operation, RegistrationError> {
    let identity = if let Some(owned) = owned {
        let owned = owned.lock().map_err(|_| uncertain())?;
        if owned.fenced || owned.launch != *launch {
            return Err(uncertain());
        }
        owned.owner.clone()
    } else {
        last.ok_or_else(uncertain)?.owner.clone()
    };
    let operation = Operation {
        operation_id: format!("{}-retire", launch.operation_id),
        launch: launch.clone(),
        owner: identity,
    };
    repository.begin_retirement(workspace, &operation)?;
    Ok(operation)
}

impl Controller {
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
        let owned = self
            .owners
            .lock()
            .map_err(|_| uncertain())?
            .get(&launch.assignment_id)
            .cloned();
        let last = repository.revoked_lease(workspace, launch)?;
        // T26: a launch that never received a grant and that no live host of
        // this daemon still holds has narrower proof than host retirement.
        if existing.is_none() && owned.is_none() && last.is_none() {
            return reconcile_ungranted(repository, workspace, launch);
        }
        let directory = runtime_directory::mapped(repository, workspace, launch, false)?;
        let operation = match existing {
            Some(existing) => existing.operation,
            None => begin(repository, workspace, launch, owned.as_ref(), last.as_ref())?,
        };
        if self.reconcile_recorded(
            repository,
            workspace,
            &directory,
            &operation,
            last.as_ref(),
            owned.is_none(),
        )? {
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
        inspect_effects(
            repository,
            workspace,
            launch,
            || self.inspection(launch),
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
                super::launch::DriverState::Crashed
                | super::launch::DriverState::Ended
                | super::launch::DriverState::Reaped => AgentObservation::Unknown,
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
                Some(super::launch::DriverState::Crashed | super::launch::DriverState::Reaped) => {
                    launcher.release_finished(&context.assignment.id);
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
            let repository = SqliteProjectRepository::open(&self.database)?;
            let workspace = repository.record_workspace(context.assignment.record_id)?;
            let Some(runtime) = repository.runtime(&workspace, &context.assignment.id)? else {
                // A launch claim without a reserved runtime (written before
                // T26 made claim and reservation one transaction): once closed
                // it can never reserve or spawn, so the claim is discharged.
                use forge_app::durable_execution::launch_failure::Repository as _;
                return repository.reconcile_unreserved_claim(&workspace, &context.assignment.id);
            };
            let launch = runtime.launch;
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
