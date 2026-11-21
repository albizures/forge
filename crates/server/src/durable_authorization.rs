//! Trusted durable grant delivery/renewal service. No operator RPC, Assignment
//! capability or production launch path exposes this service yet.
//! One `DaemonAuthority` belongs to one daemon lifetime, never retained on disk.
mod effect_admission;
pub mod effects;
mod inspection_process;
pub mod operator;
pub mod quiesced_inspection;
pub mod recovery;
pub mod recovery_inspection;
pub mod recovery_worktree;
pub mod retirement;
mod runtime_directory;
pub mod waiting_recovery_inspection;
use forge_app::{
    RegistrationError,
    authorization_lease::{
        AuthorityHost as _,
        session::{Failure, Session, Tick},
    },
    durable_execution::Launch,
};
use forge_runner_host::{SpawnedOwner, control::ControlPipe, supervision::boottime_millis};
use forge_store::SqliteProjectRepository;
use std::{
    path::PathBuf,
    process::Child,
    sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError},
    time::Duration,
};

/// A fresh cryptographic epoch fences previous daemon lifetimes. This is not a
/// credential, heartbeat or proof that old supervised processes have exited.
pub struct DaemonAuthority {
    epoch: String,
    inspection: Option<operator::InspectionComponents>,
}

impl DaemonAuthority {
    /// Create once in the daemon's trusted composition root when durable launch
    /// is wired. Never accept a persisted or caller-supplied epoch as a shortcut.
    /// # Errors
    /// Fails closed if the operating system cannot generate fresh identity.
    pub fn new() -> Result<Self, RegistrationError> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| {
            RegistrationError::new(
                "assignment-epoch-unavailable",
                "fresh daemon authorization identity is unavailable",
            )
        })?;
        Ok(Self {
            inspection: None,
            epoch: bytes
                .iter()
                .fold(String::with_capacity(64), |mut epoch, byte| {
                    use std::fmt::Write;
                    write!(epoch, "{byte:02x}").expect("writing to a String cannot fail");
                    epoch
                }),
        })
    }

    /// Select explicit trusted matched inspection components for retained stores.
    /// No PATH discovery, model authentication or execution permission is added.
    /// # Errors
    /// Rejects relative component paths.
    pub fn with_inspection(
        mut self,
        components: operator::InspectionComponents,
    ) -> Result<Self, RegistrationError> {
        components.validate()?;
        self.inspection = Some(components);
        Ok(self)
    }

    /// Prepare protected runtime storage from the retained immutable store ID
    /// and registered Workspace root. This cannot issue execution permission.
    /// # Errors
    /// Rejects missing/mismatched reservations and unsafe filesystem boundaries.
    pub fn prepare_directory(
        &self,
        repository: &SqliteProjectRepository,
        workspace: &str,
        launch: &Launch,
    ) -> Result<PathBuf, RegistrationError> {
        runtime_directory::mapped(repository, workspace, launch, true)
    }

    /// Bind immutable Forge state to a directly spawned, ready supervisor. Its
    /// directory is derived from Forge's canonical runtime mapping, never an RPC
    /// path. A retained owner file or lease alone cannot construct this service.
    /// # Errors
    /// This synchronous path is fresh-only. Existing storage must use
    /// `bind_inspected`; no retained runtime can bypass the effect fence.
    /// Rejects malformed bindings, unsafe storage or unverified live ownership.
    pub fn bind<'a>(
        &self,
        repository: &SqliteProjectRepository,
        workspace: String,
        launch: Launch,
        generation: u64,
        supervisor: &'a mut Child,
    ) -> Result<Authorization<'a>, RegistrationError> {
        let directory = runtime_directory::mapped(repository, &workspace, &launch, false)?;
        if let Err(error) =
            effect_admission::require_fresh(repository, &workspace, &launch, &directory)
        {
            forge_app::authorization_lease::session::SessionRepository::retain_failure(
                repository,
                &workspace,
                &launch,
                Failure::AuthorityUnavailable,
            )?;
            return Err(error);
        }
        self.bind_owned(repository, workspace, launch, generation, supervisor)
    }

    /// Bind a directly spawned, lock-owning supervisor through the committed
    /// interrupted-effect gate BEFORE its first grant. The worker must wait for
    /// that private grant before writable Harness opening, model or tool work.
    /// Existing runtime storage is inspected with explicit matched components;
    /// committed effects are retained transactionally before admission rejects.
    /// No audit waives unsettled runtime intent. This is the controller's effect
    /// precondition, not implementation of authorized writable recovery (T10).
    /// # Errors
    /// Inspection, ownership or retention failure fences this session and retains
    /// uncertainty. No permission is delivered, and no worktree/receipt is repaired.
    pub async fn bind_inspected<'a>(
        &self,
        repository: &SqliteProjectRepository,
        workspace: String,
        launch: Launch,
        generation: u64,
        supervisor: &'a mut Child,
    ) -> Result<Authorization<'a>, RegistrationError> {
        let directory = runtime_directory::mapped(repository, &workspace, &launch, false)?;
        let mut authorization = self.bind_owned(
            repository,
            workspace.clone(),
            launch.clone(),
            generation,
            supervisor,
        )?;
        let inspection = async {
            let before = authorization.host.observe()?;
            if effect_admission::has_database(&directory)? {
                let components = self
                    .inspection
                    .as_ref()
                    .ok_or_else(effect_admission::denied)?;
                effects::inspect_before_grant(repository, &workspace, &launch, components).await?;
            } else {
                effect_admission::require_fresh(repository, &workspace, &launch, &directory)?;
            }
            let after = authorization.host.observe()?;
            if before.owner != after.owner || after.boottime_millis < before.boottime_millis {
                return Err(effect_admission::denied());
            }
            Ok(())
        }
        .await;
        if let Err(error) = inspection {
            authorization
                .session
                .withdraw(repository, Failure::AuthorityUnavailable)?;
            return Err(error);
        }
        Ok(authorization)
    }

    fn bind_owned<'a>(
        &self,
        repository: &SqliteProjectRepository,
        workspace: String,
        launch: Launch,
        generation: u64,
        supervisor: &'a mut Child,
    ) -> Result<Authorization<'a>, RegistrationError> {
        let directory = runtime_directory::mapped(repository, &workspace, &launch, false)?;
        let mut session = Session::new(workspace, launch, generation, self.epoch.clone())?;
        let pipe = match ControlPipe::take(supervisor) {
            Ok(pipe) => pipe,
            Err(error) => {
                session.withdraw(repository, Failure::DeliveryUncertain)?;
                return Err(error);
            }
        };
        let host = match SpawnedOwner::capture(supervisor, &directory) {
            Ok(host) => host,
            Err(error) => {
                session.withdraw(repository, Failure::AuthorityUnavailable)?;
                return Err(error);
            }
        };
        Ok(Authorization {
            session,
            host,
            pipe,
        })
    }
}

/// The server owns both trusted evidence and the directly spawned host's private
/// input writer. No grant, owner, clock or epoch arrives from an agent. Dropping
/// this handle is not retirement or capacity release: the independent watchdog
/// and persisted runtime reservations remain necessary.
pub struct Authorization<'a> {
    session: Session,
    host: SpawnedOwner<'a>,
    pipe: ControlPipe,
}

impl Authorization<'_> {
    /// Retain the same private control channel when handing the directly spawned
    /// supervisor to the retirement controller. This grants no execution authority.
    #[must_use]
    pub fn retirement_control(&self) -> ControlPipe {
        self.pipe.clone()
    }

    /// Commit current Forge authority before bounded pipe delivery, or wait until
    /// the app-owned renewal deadline. A write is not worker acknowledgement.
    /// # Errors
    /// Any failed observation/commit/delivery terminally fences renewal and
    /// attempts atomic durable uncertainty retention. A later tick retries only
    /// retention; it cannot issue or replay an uncertain control.
    pub fn tick(
        &mut self,
        repository: &SqliteProjectRepository,
    ) -> Result<Tick, RegistrationError> {
        self.session
            .tick(repository, &mut self.host, &mut self.pipe)
    }

    /// Blocking driver for a dedicated daemon worker, never an async executor
    /// thread or database transaction. Wake on app-owned absolute renewal times;
    /// stop/disconnected controller retains uncertainty without reviving work.
    /// Production launch must explicitly start and retain this driver.
    /// # Errors
    /// Returns on fenced issuance/delivery or failed diagnostic persistence.
    pub fn run(
        &mut self,
        repository: &SqliteProjectRepository,
        stop: &Receiver<()>,
    ) -> Result<(), RegistrationError> {
        loop {
            match stop.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => return self.session.stop(repository),
                Err(TryRecvError::Empty) => {}
            }
            let tick = self.tick(repository)?;
            let Ok(now) = boottime_millis() else {
                self.session
                    .withdraw(repository, Failure::AuthorityUnavailable)?;
                return Err(RegistrationError::new(
                    "authorization-clock-unavailable",
                    "renewal driver stopped without a trustworthy clock; runtime capacity remains retained",
                ));
            };
            let wait = Duration::from_millis(tick.next_renewal_millis().saturating_sub(now));
            match stop.recv_timeout(wait) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                    return self.session.stop(repository);
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_lifetimes_have_distinct_non_retained_epochs() {
        let first = DaemonAuthority::new().unwrap();
        let second = DaemonAuthority::new().unwrap();
        assert_eq!(first.epoch.len(), 64);
        assert!(first.epoch.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first.epoch, second.epoch);
    }
}
