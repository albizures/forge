//! Daemon same-attempt recovery driver, composed in production beside the
//! launcher (`super::launch`). New launches stay behind the release gate.
//! Registry entries contain directly spawned children, never adopted PIDs/grants.
use super::{
    DaemonAuthority, inspection_process, quiesced_inspection, retirement, runtime_directory,
    waiting_recovery_inspection,
};
use forge_app::{
    AgentObservation, RegistrationError,
    authorization_lease::session::{Failure, SessionRepository},
    durable_execution::{
        Launch,
        completion::{AnswerObservation, Repository as _},
        recovery::{Claim, Repository as _},
    },
};
use forge_rpc::durable_execution::{RunnerEvent, RunnerOutput};
use forge_runner_host::control::ControlPipe;
use forge_store::SqliteProjectRepository;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};

fn denied() -> RegistrationError {
    RegistrationError::new(
        "durable-recovery-unavailable",
        "recovery proof or owned execution is unavailable; preserve capacity and investigate",
    )
}

/// Explicit trusted matched components, not an operator request or PATH lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Components {
    pub host: PathBuf,
    /// Fixed leading host arguments. Empty for the packaged `forge-runner-host`;
    /// a test composition may re-exec its own supervisor entry instead.
    pub host_arguments: Vec<std::ffi::OsString>,
    pub node: PathBuf,
    pub runner: PathBuf,
    pub inspector: PathBuf,
    pub answer_inspector: PathBuf,
}
impl Components {
    /// # Errors
    /// Missing/relative components cannot start a recovery process.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        for path in [
            &self.host,
            &self.node,
            &self.runner,
            &self.inspector,
            &self.answer_inspector,
        ] {
            if !path.is_absolute()
                || !path.is_file()
                || std::fs::canonicalize(path).map_err(|_| denied())? != *path
            {
                return Err(denied());
            }
        }
        Ok(())
    }
}
struct Slot {
    launch: Launch,
    workspace: String,
    observation: Arc<Mutex<AgentObservation>>,
    stop: Sender<Sender<()>>,
    stopped: bool,
}

/// One registry per daemon lifetime, with fresh nonretained issuer authority for
/// each proven recovery episode. No public RPC exposes this service. Uncertain
/// starts stay registered and are never retried by a later scan; only exact
/// positive crash proof can begin a subsequent episode.
pub struct Controller {
    database: PathBuf,
    components: Components,
    /// When composed, each Launch recovers only with the set whose attested
    /// component hash equals its own; otherwise `components` (legacy single set).
    catalog: Option<Arc<super::launch::ComponentCatalog>>,
    authority: Arc<DaemonAuthority>,
    retirement: Arc<retirement::Controller>,
    slots: Mutex<HashMap<String, Slot>>,
}
impl Controller {
    /// # Errors
    /// Rejects missing matched components or unavailable fresh daemon identity.
    pub fn new(
        database: PathBuf,
        components: Components,
        retirement: Arc<retirement::Controller>,
    ) -> Result<Self, RegistrationError> {
        components.validate()?;
        Ok(Self {
            database,
            components,
            catalog: None,
            authority: Arc::new(DaemonAuthority::new()?),
            retirement,
            slots: Mutex::new(HashMap::new()),
        })
    }

    /// Select recovery components per Launch from the daemon's matched catalog.
    #[must_use]
    pub fn with_catalog(mut self, catalog: Arc<super::launch::ComponentCatalog>) -> Self {
        self.catalog = Some(catalog);
        self
    }

    /// Start only after protected original-host proof and exclusive read-only
    /// runtime inspection. Live owners, missing hosts/proof and invalid bindings
    /// are uncertainty, not permission to reserve recovery or spawn replacement.
    /// # Errors
    /// Fails closed without Herdr fallback or changing any Assignment identity.
    pub fn recover(
        &self,
        workspace: &str,
        launch: &Launch,
    ) -> Result<AgentObservation, RegistrationError> {
        let mut slots = self.slots.lock().map_err(|_| denied())?;
        if let Some(slot) = slots.get(&launch.assignment_id) {
            if slot.launch != *launch || slot.workspace != workspace {
                return Err(denied());
            }
            return slot.observation.lock().map(|v| *v).map_err(|_| denied());
        }
        // The Launch's own matched components, or an explicit refusal before any
        // claim or spawn: retained work never runs under other components.
        let components = match &self.catalog {
            Some(catalog) => catalog.execution_for(launch)?.components,
            None => self.components.clone(),
        };
        let repository = SqliteProjectRepository::open(&self.database)?;
        let candidate = repository.recovery_candidate(workspace, launch)?;
        let claim = Claim::for_candidate(
            &candidate,
            format!("recover-{}", candidate.last_lease.sequence),
            self.authority.epoch.clone(),
        )?;
        let directory = runtime_directory::mapped(&repository, workspace, launch, false)?;
        // Do not fence ordinary grants while an original owner might still run.
        let guard = forge_runner_host::inspection::ExclusiveInspection::acquire(&directory)
            .map_err(|_| denied())?;
        let operation =
            forge_app::durable_execution::recovery::quiescence::Operation::for_candidate(
                &candidate,
                claim.clone(),
            )?;
        forge_runner_host::quiescence::waiting_receipt(&directory, &operation)
            .map_err(|_| denied())?;
        guard.verify().map_err(|_| denied())?;
        repository.claim_recovery(workspace, &claim)?;
        drop(guard);
        let observation = Arc::new(Mutex::new(AgentObservation::Unknown));
        let (stop, stops) = mpsc::channel();
        let (started, start) = mpsc::channel();
        let worker = Worker {
            database: self.database.clone(),
            components,
            retirement: self.retirement.clone(),
            workspace: workspace.into(),
            claim,
            observation: observation.clone(),
            stops,
            started: Some(started),
        };
        slots.insert(
            launch.assignment_id.clone(),
            Slot {
                launch: launch.clone(),
                workspace: workspace.into(),
                observation,
                stop,
                stopped: false,
            },
        );
        std::thread::spawn(move || worker.run());
        drop(slots);
        start
            .recv_timeout(Duration::from_secs(75))
            .map_err(|_| denied())??;
        Ok(AgentObservation::Working)
    }

    /// Stop the grant driver before transferring its original Child/private pipe
    /// to the retirement registry. Called only after application's revocation.
    /// # Errors
    /// Failed handoff retains uncertainty; it never adopts a process by identity.
    pub fn stop_for_retirement(
        &self,
        workspace: &str,
        launch: &Launch,
    ) -> Result<(), RegistrationError> {
        let mut slots = self.slots.lock().map_err(|_| denied())?;
        let Some(slot) = slots.get_mut(&launch.assignment_id) else {
            return Ok(());
        };
        if slot.launch != *launch || slot.workspace != workspace {
            return Err(denied());
        }
        if slot.stopped {
            return Ok(());
        }
        let (done, wait) = mpsc::channel();
        if slot.stop.send(done).is_err() {
            // The local driver is gone, not necessarily its execution. Only the
            // retirement adapter's separate exact proof can establish cleanup.
            slot.stopped = true;
            return Ok(());
        }
        wait.recv_timeout(Duration::from_secs(10))
            .map_err(|_| denied())?;
        slot.stopped = true;
        Ok(())
    }
}

struct Worker {
    database: PathBuf,
    components: Components,
    retirement: Arc<retirement::Controller>,
    workspace: String,
    claim: Claim,
    observation: Arc<Mutex<AgentObservation>>,
    stops: Receiver<Sender<()>>,
    started: Option<Sender<Result<(), RegistrationError>>>,
}
enum Exit {
    Retire(Sender<()>),
    Crashed(Box<Claim>),
}

impl Worker {
    fn set(&self, observation: AgentObservation) {
        if let Ok(mut value) = self.observation.lock() {
            *value = observation;
        }
    }
    fn run(mut self) {
        let result = (|| -> Result<(), RegistrationError> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| denied())?;
            let repository = SqliteProjectRepository::open(&self.database)?;
            loop {
                let Some(next) = runtime.block_on(self.drive(&repository))? else {
                    return Ok(());
                };
                self.set(AgentObservation::Unknown);
                repository.claim_recovery(&self.workspace, &next)?;
                self.claim = next;
            }
        })();
        if result.is_err() {
            self.set(AgentObservation::Blocked);
            if let Some(started) = self.started.take() {
                let _ = started.send(Err(denied()));
            }
            // Failure to retain is logged without private payloads; capacity's
            // original runtime/claim reservation remains even if storage is down.
            if SqliteProjectRepository::open(&self.database)
                .and_then(|r| {
                    r.retain_failure(
                        &self.workspace,
                        &self.claim.launch,
                        Failure::AuthorityUnavailable,
                    )
                })
                .is_err()
            {
                tracing::warn!(
                    code = "durable-recovery-retention-unavailable",
                    "recovery fenced; retain original resources"
                );
            }
        }
    }

    async fn drive(
        &mut self,
        repository: &SqliteProjectRepository,
    ) -> Result<Option<Claim>, RegistrationError> {
        let c = &self.components;
        let proof = quiesced_inspection::inspect(
            repository,
            &self.workspace,
            &self.claim,
            &c.node,
            &c.inspector,
        )
        .await?;
        let path = PathBuf::from(&proof.candidate().execution_directory);
        proof.verify(repository, &self.workspace)?;
        drop(proof);
        let directory =
            runtime_directory::mapped(repository, &self.workspace, &self.claim.launch, false)?;
        let mut child = Command::new(&c.host)
            .args(&c.host_arguments)
            .args([&directory, &path, &c.node, &c.runner])
            .env_clear()
            .current_dir(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| denied())?;
        let output = events(child.stdout.take().ok_or_else(denied)?);
        let owned = output
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| denied())?;
        let Ok(RunnerEvent::Owned { compatibility, .. }) = owned else {
            return Err(denied());
        };
        compatibility
            .validate(&self.claim.launch.component_hash)
            .map_err(|_| denied())?;
        let mut pipe = ControlPipe::take(&mut child)?;
        let handoff = waiting_recovery_inspection::inspect(
            repository,
            &self.workspace,
            &self.claim,
            &mut child,
            &c.node,
            &c.inspector,
        )
        .await?
        .inspect_progress(repository, &self.workspace, &c.node, &c.inspector)
        .await?
        .prepare(repository)
        .await?;
        let admission = handoff.prepared().candidate().admission_id.clone();
        let next = handoff.prepared().grant().lease.issued_boottime_millis
            + forge_app::authorization_lease::RENEW_AFTER_MILLIS;
        let mut active = handoff.start(repository, &mut pipe).await?;
        self.set(AgentObservation::Working);
        if let Some(started) = self.started.take() {
            started.send(Ok(())).map_err(|_| denied())?;
        }
        let exit = self
            .run_active(
                repository,
                &mut active,
                &mut pipe,
                &output,
                &admission,
                next,
            )
            .await?;
        drop(active);
        match exit {
            Exit::Retire(done) => {
                self.retirement.register(
                    repository,
                    &self.workspace,
                    self.claim.launch.clone(),
                    child,
                    pipe,
                )?;
                done.send(()).map_err(|_| denied())?;
                Ok(None)
            }
            Exit::Crashed(next) => {
                if child.try_wait().map_err(|_| denied())?.is_none() {
                    return Err(denied());
                }
                Ok(Some(*next))
            }
        }
    }

    fn inspect_crash(
        &self,
        repository: &SqliteProjectRepository,
    ) -> Result<Claim, RegistrationError> {
        let candidate = repository.recovery_candidate(&self.workspace, &self.claim.launch)?;
        let next = Claim::for_candidate(
            &candidate,
            format!("recover-{}", candidate.last_lease.sequence),
            DaemonAuthority::new()?.epoch,
        )?;
        let directory =
            runtime_directory::mapped(repository, &self.workspace, &self.claim.launch, false)?;
        let guard = forge_runner_host::inspection::ExclusiveInspection::acquire(&directory)
            .map_err(|_| denied())?;
        let operation =
            forge_app::durable_execution::recovery::quiescence::Operation::for_candidate(
                &candidate,
                next.clone(),
            )?;
        forge_runner_host::quiescence::waiting_receipt(&directory, &operation)
            .map_err(|_| denied())?;
        guard.verify().map_err(|_| denied())?;
        Ok(next)
    }

    async fn run_active(
        &self,
        repository: &SqliteProjectRepository,
        active: &mut waiting_recovery_inspection::progress::handoff::Active<'_>,
        pipe: &mut ControlPipe,
        output: &Receiver<Result<RunnerEvent, ()>>,
        admission: &str,
        mut next: u64,
    ) -> Result<Exit, RegistrationError> {
        let mut renew = true;
        let mut resumed = false;
        let mut sent = std::collections::HashSet::new();
        let mut last_poll = std::time::Instant::now()
            .checked_sub(FOLLOWUP_POLL)
            .unwrap_or_else(std::time::Instant::now);
        loop {
            if renew && resumed && last_poll.elapsed() >= FOLLOWUP_POLL {
                last_poll = std::time::Instant::now();
                send_followups(
                    repository,
                    &self.workspace,
                    &self.claim.launch.assignment_id,
                    pipe,
                    &mut sent,
                );
            }
            match self.stops.try_recv() {
                Ok(done) => {
                    active.stop(repository)?;
                    return Ok(Exit::Retire(done));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    active.stop(repository)?;
                    return Err(denied());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            match output.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(RunnerEvent::Resumed { .. })) => {
                    resumed = true;
                    last_poll = last_poll.checked_sub(FOLLOWUP_POLL).unwrap_or(last_poll);
                }
                Ok(Ok(RunnerEvent::FollowupAccepted {
                    operation_id,
                    admission_id,
                    ..
                })) => {
                    record_followup_event(
                        repository,
                        &self.workspace,
                        &self.claim.launch.assignment_id,
                        &operation_id,
                        &admission_id,
                    );
                }
                Ok(Ok(RunnerEvent::Settled { .. })) => {
                    let result = inspect_answer(
                        repository,
                        &self.workspace,
                        &self.claim.launch,
                        admission,
                        &self.components,
                    )
                    .await;
                    if matches!(result, Ok(AnswerObservation::OutcomeAccepted)) {
                        self.set(AgentObservation::Idle);
                    } else {
                        active.stop(repository)?;
                        renew = false;
                        self.set(AgentObservation::Blocked);
                    }
                }
                // A runtime failure or any output outside protocol 1 is terminal
                // for renewal. Nothing is inferred, replayed or released.
                Ok(Ok(RunnerEvent::RuntimeFailed {}) | Err(())) => {
                    active.stop(repository)?;
                    renew = false;
                    self.set(AgentObservation::Blocked);
                }
                // A failed follow-up stays `unknown`; nothing is fabricated.
                Ok(Ok(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if !renew {
                        return Err(denied());
                    }
                    return self
                        .inspect_crash(repository)
                        .map(|next| Exit::Crashed(Box::new(next)));
                }
            }
            let now = forge_runner_host::supervision::boottime_millis().map_err(|_| denied())?;
            if renew && now >= next {
                if let Ok(tick) = active.tick(repository, pipe) {
                    next = tick.next_renewal_millis();
                } else {
                    renew = false;
                    self.set(AgentObservation::Blocked);
                }
            }
        }
    }
}

/// Send each recorded, undelivered follow-up once per supervised session. The
/// `unknown` state is committed before the write, so a crash or a lost event
/// never reads as delivered; the runtime deduplicates any later resend.
pub(super) fn send_followups(
    repository: &SqliteProjectRepository,
    workspace: &str,
    assignment: &str,
    pipe: &mut ControlPipe,
    sent: &mut std::collections::HashSet<String>,
) {
    use forge_app::durable_execution::followup::{Delivery, Repository as _};
    let Ok(pending) = repository.undelivered_followups(workspace, assignment) else {
        return;
    };
    for request in pending {
        if sent.contains(&request.operation_id) {
            continue;
        }
        if repository
            .mark_followup(
                workspace,
                assignment,
                &request.operation_id,
                &Delivery::Unknown,
            )
            .is_err()
        {
            return;
        }
        sent.insert(request.operation_id.clone());
        if pipe.deliver_followup(&request).is_err() {
            return;
        }
    }
}

/// Record the runner's typed acceptance; a failed write stays `unknown`.
pub(super) fn record_followup_event(
    repository: &SqliteProjectRepository,
    workspace: &str,
    assignment: &str,
    operation: &str,
    admission: &str,
) {
    use forge_app::durable_execution::followup::{Delivery, Repository as _};
    let _ = repository.mark_followup(
        workspace,
        assignment,
        operation,
        &Delivery::Delivered {
            admission_id: admission.into(),
        },
    );
}

pub(super) const FOLLOWUP_POLL: Duration = Duration::from_millis(500);

/// Strictly typed runner events. A line outside protocol 1 is reported once as
/// `Err(())` and ends the stream: it is never skipped or guessed at.
fn events(source: impl Read + Send + 'static) -> Receiver<Result<RunnerEvent, ()>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(source);
        loop {
            let mut line = Vec::new();
            if !Read::by_ref(&mut reader)
                .take(65_537)
                .read_until(b'\n', &mut line)
                .is_ok_and(|n| n > 0 && n <= 65_536 && line.last() == Some(&b'\n'))
            {
                break;
            }
            let event = match RunnerOutput::parse(&line) {
                Ok(RunnerOutput::Event(event)) => Ok(event),
                _ => Err(()),
            };
            let malformed = event.is_err();
            if tx.send(event).is_err() || malformed {
                break;
            }
        }
    });
    rx
}

pub(super) async fn inspect_answer(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    admission: &str,
    components: &Components,
) -> Result<AnswerObservation, RegistrationError> {
    use forge_rpc::durable_execution::{
        RUNNER_PROTOCOL_VERSION,
        answer_inspection::{Answer, MAX_RESPONSE_BYTES, Request, Response},
    };
    let directory = runtime_directory::mapped(repository, workspace, launch, false)?;
    let request = serde_json::to_vec(&Request {
        protocol: RUNNER_PROTOCOL_VERSION,
        launch: launch.clone(),
        admission_id: admission.into(),
    })
    .map_err(|_| denied())?;
    let bytes = inspection_process::read(
        &components.node,
        &components.answer_inspector,
        &directory,
        &request,
        Duration::from_secs(30),
        MAX_RESPONSE_BYTES,
    )
    .await
    .map_err(|_| denied())?;
    let value: Response = serde_json::from_slice(&bytes).map_err(|_| denied())?;
    value.validate(launch)?;
    if value.answer != Answer::Answered {
        return Err(denied());
    }
    repository.observe_answer(workspace, launch)
}
