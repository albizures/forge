//! Production durable Check launch (T23).
//!
//! `provision` is short and non-blocking: it atomically claims the single launch,
//! commits the immutable runtime identity (admission intent) and launch intent,
//! prepares protected storage and the runner's protected input, and only then
//! spawns the matched `forge-runner-host`. A daemon-owned driver thread then binds
//! the directly spawned host through the interrupted-effect gate, delivers and
//! renews the authorization lease, and admits through the idempotent
//! [`durable_execution::admit`] path over the private supervisor pipe.
//!
//! Nothing here enables launch: the release gate decides eligibility before the
//! application ever calls `provision`. Ownership is exclusive by construction:
//! one atomic provision claim per Assignment, no respawn of a claimed launch,
//! the host's kernel flock, and no adoption of processes by identity.
use super::{Authorization, DaemonAuthority, recovery, retirement, runtime_directory};
use forge_app::{
    AgentAdapterError, AgentDispatchContext, AgentDispatchRepository as _, AgentObservation,
    AgentProfile, RegistrationError,
    assignment::BoundIdentity,
    authorization_lease::session::Failure,
    durable_execution::{
        self, Admission, Launch, Repository as _, Transport, completion::AnswerObservation,
    },
};
use forge_rpc::durable_execution::{
    CHECK_RUNTIME_VERSION, CheckCapability, CheckRuntimeInput, DURABLE_RUNTIME_SCHEMA,
    RUNNER_PROTOCOL_VERSION, RunnerCompatibility, RunnerEvent, RunnerOperation, RunnerOutput,
    RunnerRequest,
};
use forge_runner_host::{control::ControlPipe, supervision::boottime_millis};
use forge_store::SqliteProjectRepository;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
    },
    time::{Duration, Instant},
};

const OWNED_TIMEOUT: Duration = Duration::from_secs(30);
const ADMISSION_TIMEOUT: Duration = Duration::from_secs(120);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(50);

fn uncertain() -> RegistrationError {
    RegistrationError::new(
        "durable-launch-uncertain",
        "durable launch outcome is uncertain; retain resources and capacity",
    )
}

fn invalid(message: &str) -> RegistrationError {
    RegistrationError::new("durable-launch-invalid", message.to_owned())
}

/// The Forge Revision recorded on every new Launch: this build's commit, marked
/// when it was built from a modified checkout. Never a caller-selected value.
#[must_use]
pub fn current_forge_revision() -> String {
    let build = forge_build_info::current();
    if build.dirty {
        format!("{}-dirty", build.commit)
    } else {
        build.commit.to_owned()
    }
}

/// Ask the matched runner sources for their attested component hash with the
/// matched Node, never `PATH`. `contract.ts` in the matched source directory is
/// the single definition the runner validates every request and Launch against.
/// # Errors
/// Missing components or an unexpected answer fail closed.
pub fn attest_component_hash(node: &Path, sources: &Path) -> Result<String, RegistrationError> {
    let contract = sources.join("contract.ts");
    let module = serde_json::to_string(&contract).map_err(|_| uncertain())?;
    let output = Command::new(node)
        .env_clear()
        .args(["--input-type=module", "-e"])
        .arg(format!(
            "import {{componentHash}} from {module}; process.stdout.write(componentHash());"
        ))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| invalid("matched runner components are unavailable"))?;
    let hash = String::from_utf8(output.stdout).unwrap_or_default();
    if !output.status.success()
        || hash.len() != 64
        || !hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(invalid("matched runner component hash is unavailable"));
    }
    Ok(hash)
}

/// Trusted daemon composition inputs for new durable launches.
#[derive(Clone)]
pub struct Settings {
    pub database: PathBuf,
    /// Matched host, Node, runner entry and read-only inspectors.
    pub components: recovery::Components,
    /// Attested hash of exactly these runner components.
    pub component_hash: String,
    /// The Forge Revision recorded on each Launch.
    pub forge_revision: String,
    /// Explicit owner-only model credential file (a path, never key bytes).
    pub model_key_file: Option<PathBuf>,
}

/// Observable state of one daemon-owned launch driver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverState {
    /// The driver is supervising; the value is its latest observation.
    Live(AgentObservation),
    /// The runner stream ended while renewing: only exact recovery proof may continue.
    Crashed,
    /// The driver ended after a fence or handoff; no local process is supervised.
    Ended,
}

struct Slot {
    launch: Launch,
    workspace: String,
    state: Arc<Mutex<DriverState>>,
    stop: Sender<Sender<()>>,
    stopped: bool,
}

/// One launcher per daemon lifetime. Its registry holds only children this
/// daemon spawned directly; a restarted daemon starts empty and never adopts.
pub struct Launcher {
    settings: Settings,
    authority: Arc<DaemonAuthority>,
    retirement: Arc<retirement::Controller>,
    slots: Mutex<HashMap<String, Slot>>,
}

impl Launcher {
    /// # Errors
    /// Rejects missing matched components, an invalid component hash or an
    /// unavailable fresh daemon identity.
    pub fn new(
        settings: Settings,
        authority: Arc<DaemonAuthority>,
        retirement: Arc<retirement::Controller>,
    ) -> Result<Self, RegistrationError> {
        settings.components.validate()?;
        if settings.component_hash.len() != 64 || settings.forge_revision.trim().is_empty() {
            return Err(invalid("matched component identity is required"));
        }
        Ok(Self {
            settings,
            authority,
            retirement,
            slots: Mutex::new(HashMap::new()),
        })
    }

    /// Validate the explicit profile and host authentication for a new launch.
    /// The cwd is re-validated against the bound Managed Worktree at provision.
    /// # Errors
    /// Rejects incompatible profiles and missing or unprotected credentials.
    pub fn validate_profile(&self, profile: &AgentProfile) -> Result<(), RegistrationError> {
        crate::durable_profiles::prepare(
            profile,
            Path::new("/"),
            self.settings.model_key_file.as_deref(),
        )
        .map(|_| ())
    }

    /// # Errors
    /// Rejects missing components and unavailable authentication or worktree.
    pub fn validate_provision(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<(), RegistrationError> {
        self.settings.components.validate()?;
        let cwd = context
            .agent_workspace
            .as_ref()
            .ok_or_else(|| invalid("durable launch requires the bound Managed Worktree"))?;
        crate::durable_profiles::prepare(
            &context.assignment.profile,
            cwd,
            self.settings.model_key_file.as_deref(),
        )
        .map(|_| ())
    }

    /// The local driver state, if this daemon lifetime launched the Assignment.
    #[must_use]
    pub fn state(&self, assignment: &str) -> Option<DriverState> {
        let slots = self.slots.lock().ok()?;
        let slot = slots.get(assignment)?;
        slot.state.lock().ok().map(|state| *state)
    }

    fn mint(prefix: &str) -> Result<String, RegistrationError> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| uncertain())?;
        Ok(bytes.iter().fold(format!("{prefix}-"), |mut value, byte| {
            use std::fmt::Write;
            write!(value, "{byte:02x}").expect("writing to a String cannot fail");
            value
        }))
    }

    /// Commit intent, prepare protected storage and spawn the matched host, then
    /// hand it to a daemon-owned driver. Returns once the host is spawned.
    /// # Errors
    /// A duplicate or closed claim never spawns. Any failure after the launch
    /// intent leaves it retained as uncertainty for explicit reconciliation.
    #[allow(clippy::too_many_lines)] // One ordered intent-before-effect sequence.
    pub fn provision(&self, context: &AgentDispatchContext) -> Result<(), RegistrationError> {
        let id = context.assignment.id.clone();
        if self
            .slots
            .lock()
            .map_err(|_| uncertain())?
            .contains_key(&id)
        {
            // Same daemon lifetime: the driver already owns this launch.
            return Ok(());
        }
        let delivery = context
            .capability_delivery
            .as_ref()
            .ok_or_else(|| invalid("durable launch requires protected capability delivery"))?;
        let cwd = context
            .agent_workspace
            .clone()
            .ok_or_else(|| invalid("durable launch requires the bound Managed Worktree"))?;
        let prepared = crate::durable_profiles::prepare(
            &context.assignment.profile,
            &cwd,
            self.settings.model_key_file.as_deref(),
        )?;
        let repository = SqliteProjectRepository::open(&self.settings.database)?;
        let workspace = repository.record_workspace(context.assignment.record_id)?;
        let attempt = repository
            .assignment_check_attempt(&id)?
            .ok_or_else(|| invalid("durable launch requires a Check-bound attempt"))?;
        // Atomic single-launch claim: an earlier attempt (this or a previous daemon
        // lifetime, or a closed Assignment) is never respawned.
        if !repository.begin_agent_provision(&id)? {
            return Err(RegistrationError::new(
                "durable-launch-claimed",
                "durable launch was already claimed or the Assignment is closed; observe, recover or retire it",
            ));
        }
        let launch = Launch {
            assignment_id: id.clone(),
            operation_id: Self::mint("admit")?,
            attempt_id: attempt.id.clone(),
            profile_revision: context.assignment.profile.revision,
            runtime_id: Self::mint("runtime")?,
            incarnation_id: Self::mint("incarnation")?,
            conversation_id: Self::mint("conversation")?,
            store_id: Self::mint("store")?,
            forge_revision: self.settings.forge_revision.clone(),
            component_hash: self.settings.component_hash.clone(),
            runtime_schema: DURABLE_RUNTIME_SCHEMA,
        };
        launch.validate()?;
        // Admission intent precedes every external effect.
        let reserved = repository.reserve(&workspace, &launch)?;
        if reserved.launch != launch || reserved.admission != Admission::Pending {
            return Err(uncertain());
        }
        let directory = runtime_directory::mapped(&repository, &workspace, &launch, true)?;
        if directory.starts_with(&cwd) || cwd.starts_with(&directory) {
            return Err(invalid(
                "runtime storage must be separate from the Managed Worktree",
            ));
        }
        let bound = BoundIdentity {
            assignment_id: id.clone(),
            record_id: context.assignment.record_id,
            profile_revision: launch.profile_revision,
            profile_record_type: "task".into(),
            provider: context.assignment.profile.provider.clone(),
            agent_kind: context.assignment.profile.agent_kind.clone(),
            model: context.assignment.profile.model.clone(),
            attempt_number: context.assignment.attempt_number,
            check_attempt: Some(attempt.clone()),
            execution_directory: cwd.clone(),
            provider_workspace: None,
        };
        forge_runner_host::check_runtime::write(
            &directory,
            &CheckRuntimeInput {
                version: CHECK_RUNTIME_VERSION,
                launch: launch.clone(),
                generation: attempt.generation,
                bound,
                configuration: prepared.configuration,
                credential_file: prepared.credential_file,
                capability: CheckCapability {
                    assignment_id: id.clone(),
                    daemon_url: delivery.endpoint.clone(),
                    secret_file: delivery.secret_file.clone(),
                    forge_cli: delivery.cli_path.clone(),
                },
            },
        )?;
        if !repository.begin_agent_launch(&id, now_millis()?)? {
            return Err(uncertain());
        }
        let c = &self.settings.components;
        let child = Command::new(&c.host)
            .args(&c.host_arguments)
            .args([&directory, &cwd, &c.node, &c.runner])
            .env_clear()
            .current_dir(&directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| uncertain())?;
        let state = Arc::new(Mutex::new(DriverState::Live(AgentObservation::Working)));
        let (stop, stops) = mpsc::channel();
        let driver = Driver {
            settings: self.settings.clone(),
            authority: self.authority.clone(),
            retirement: self.retirement.clone(),
            workspace: workspace.clone(),
            launch: launch.clone(),
            generation: attempt.generation,
            state: state.clone(),
            stops,
        };
        self.slots.lock().map_err(|_| uncertain())?.insert(
            id,
            Slot {
                launch,
                workspace,
                state,
                stop,
                stopped: false,
            },
        );
        std::thread::spawn(move || driver.run(child));
        Ok(())
    }

    /// Stop the driver and hand its directly spawned host and private pipe to the
    /// retirement registry. Called only after the application's revocation.
    /// # Errors
    /// A foreign launch or a handoff timeout retains uncertainty.
    pub fn stop_for_retirement(
        &self,
        workspace: &str,
        launch: &Launch,
    ) -> Result<(), RegistrationError> {
        let mut slots = self.slots.lock().map_err(|_| uncertain())?;
        let Some(slot) = slots.get_mut(&launch.assignment_id) else {
            return Ok(());
        };
        if slot.launch != *launch || slot.workspace != workspace {
            return Err(uncertain());
        }
        if slot.stopped {
            return Ok(());
        }
        let (done, wait) = mpsc::channel();
        if slot.stop.send(done).is_ok() {
            wait.recv_timeout(STOP_TIMEOUT).map_err(|_| uncertain())?;
        }
        // A gone driver is not process absence; retirement needs its own proof.
        slot.stopped = true;
        Ok(())
    }

    /// Forget a crashed driver so exact recovery proof can take over.
    pub fn release_crashed(&self, assignment: &str) {
        if let Ok(mut slots) = self.slots.lock()
            && slots
                .get(assignment)
                .is_some_and(|slot| slot.state.lock().is_ok_and(|s| *s == DriverState::Crashed))
        {
            slots.remove(assignment);
        }
    }
}

fn now_millis() -> Result<u64, RegistrationError> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| uncertain())?;
    u64::try_from(elapsed.as_millis()).map_err(|_| uncertain())
}

/// Strict protocol-1 line reader. A line outside the contract is reported once as
/// `Err(())` and ends the stream; nothing is skipped or guessed at.
fn outputs(source: impl Read + Send + 'static) -> Receiver<Result<RunnerOutput, ()>> {
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
            let output = RunnerOutput::parse(&line).map_err(|_| ());
            let malformed = output.is_err();
            if tx.send(output).is_err() || malformed {
                break;
            }
        }
    });
    rx
}

struct Driver {
    settings: Settings,
    authority: Arc<DaemonAuthority>,
    retirement: Arc<retirement::Controller>,
    workspace: String,
    launch: Launch,
    generation: u64,
    state: Arc<Mutex<DriverState>>,
    stops: Receiver<Sender<()>>,
}

enum Exit {
    Stop(Option<Sender<()>>),
    Crashed,
    Ended,
}

impl Driver {
    fn set(&self, state: DriverState) {
        if let Ok(mut value) = self.state.lock() {
            *value = state;
        }
    }

    fn retain(&self, repository: &SqliteProjectRepository, failure: Failure) {
        use forge_app::authorization_lease::session::SessionRepository as _;
        if repository
            .retain_failure(&self.workspace, &self.launch, failure)
            .is_err()
        {
            tracing::warn!(
                assignment_id = %self.launch.assignment_id,
                code = "durable-launch-retention-unavailable",
                "durable launch fenced; retain original resources"
            );
        }
    }

    fn run(self, mut child: Child) {
        let Ok(repository) = SqliteProjectRepository::open(&self.settings.database) else {
            self.set(DriverState::Ended);
            tracing::warn!(
                code = "durable-launch-store-unavailable",
                "durable launch fenced"
            );
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            self.retain(&repository, Failure::AuthorityUnavailable);
            self.set(DriverState::Ended);
            return;
        };
        let output = outputs(stdout);
        // The host's `owned` evidence must name exactly the matched components.
        let owned = matches!(
            output.recv_timeout(OWNED_TIMEOUT),
            Ok(Ok(RunnerOutput::Event(RunnerEvent::Owned { ref compatibility, .. })))
                if compatibility.validate(&self.launch.component_hash).is_ok()
        );
        if !owned {
            self.retain(&repository, Failure::AuthorityUnavailable);
            self.set(DriverState::Ended);
            return;
        }
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            self.retain(&repository, Failure::AuthorityUnavailable);
            self.set(DriverState::Ended);
            return;
        };
        let bound = runtime.block_on(self.authority.bind_inspected(
            &repository,
            self.workspace.clone(),
            self.launch.clone(),
            self.generation,
            &mut child,
        ));
        let (exit, pipe) = match bound {
            // `bind_inspected` already retained its failure and fenced the session.
            Err(_) => {
                self.set(DriverState::Ended);
                return;
            }
            Ok(authorization) => {
                let pipe = authorization.retirement_control();
                let mut live = Live {
                    driver: &self,
                    repository: &repository,
                    runtime: &runtime,
                    authorization,
                    pipe,
                    output,
                    pending: VecDeque::new(),
                    next_renewal: 0,
                    renew: true,
                    admission: None,
                    disconnected: false,
                };
                let exit = live.supervise();
                let pipe = live.pipe.clone();
                drop(live);
                (exit, pipe)
            }
        };
        match exit {
            Exit::Stop(done) => {
                // Same directly spawned Child and original private pipe; no adoption.
                if self
                    .retirement
                    .register(
                        &repository,
                        &self.workspace,
                        self.launch.clone(),
                        child,
                        pipe,
                    )
                    .is_err()
                {
                    tracing::warn!(
                        assignment_id = %self.launch.assignment_id,
                        code = "durable-launch-handoff-uncertain",
                        "retirement handoff failed; retain original resources"
                    );
                }
                self.set(DriverState::Ended);
                if let Some(done) = done {
                    let _ = done.send(());
                }
            }
            Exit::Crashed => {
                // Issuer loss is not a stop: never fabricate a stop/hold here. Reap
                // the host after it records its crash receipt; recovery requires
                // that exact proof, never this observation.
                reap(&mut child);
                self.set(DriverState::Crashed);
            }
            Exit::Ended => {
                reap(&mut child);
                self.set(DriverState::Ended);
            }
        }
    }
}

fn reap(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => std::thread::sleep(POLL),
        }
    }
}

/// The supervised session state shared by the admission transport and the
/// supervision loop. Single-threaded: the transport borrows it through a cell.
struct Live<'d, 'c> {
    driver: &'d Driver,
    repository: &'d SqliteProjectRepository,
    runtime: &'d tokio::runtime::Runtime,
    authorization: Authorization<'c>,
    pipe: ControlPipe,
    output: Receiver<Result<RunnerOutput, ()>>,
    pending: VecDeque<RunnerOutput>,
    next_renewal: u64,
    renew: bool,
    admission: Option<String>,
    disconnected: bool,
}

impl Live<'_, '_> {
    fn fence(&mut self, failure: Option<Failure>) {
        if self.renew {
            let result = match failure {
                Some(failure) => self.authorization.withdraw(self.repository, failure),
                None => self.authorization.stop(self.repository),
            };
            if result.is_err() {
                self.driver
                    .retain(self.repository, Failure::AuthorityUnavailable);
            }
        }
        self.renew = false;
        self.driver
            .set(DriverState::Live(AgentObservation::Blocked));
    }

    fn renew_if_due(&mut self) -> Result<(), RegistrationError> {
        if !self.renew {
            return Ok(());
        }
        let now = boottime_millis().map_err(|_| uncertain())?;
        if now < self.next_renewal {
            return Ok(());
        }
        match self.authorization.tick(self.repository) {
            Ok(tick) => {
                self.next_renewal = tick.next_renewal_millis();
                Ok(())
            }
            Err(error) => {
                // `tick` already fenced and retained its failure.
                self.renew = false;
                self.driver
                    .set(DriverState::Live(AgentObservation::Blocked));
                Err(error)
            }
        }
    }

    /// Next runner output, renewing the lease while waiting.
    fn next(&mut self, wait: Duration) -> Option<Result<RunnerOutput, ()>> {
        if let Some(value) = self.pending.pop_front() {
            return Some(Ok(value));
        }
        let deadline = Instant::now() + wait;
        loop {
            // A failed renewal fences itself and retains its cause.
            let _ = self.renew_if_due();
            match self.output.recv_timeout(POLL.min(wait)) {
                Ok(value) => return Some(value),
                Err(RecvTimeoutError::Disconnected) => {
                    self.disconnected = true;
                    return None;
                }
                Err(RecvTimeoutError::Timeout) if Instant::now() >= deadline => return None,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn request(&self, operation: RunnerOperation) -> RunnerRequest {
        RunnerRequest {
            compatibility: RunnerCompatibility {
                protocol: RUNNER_PROTOCOL_VERSION,
                runtime_schema: DURABLE_RUNTIME_SCHEMA,
                component_hash: self.driver.launch.component_hash.clone(),
            },
            assignment_id: self.driver.launch.assignment_id.clone(),
            runtime_id: self.driver.launch.runtime_id.clone(),
            operation_id: self.driver.launch.operation_id.clone(),
            operation,
        }
    }

    /// Send one request and wait for its correlated response; runner events in
    /// between are kept, in order, for the supervision loop.
    fn exchange(&mut self, request: &RunnerRequest) -> Result<Admission, RegistrationError> {
        self.pipe.request(request)?;
        let deadline = Instant::now() + ADMISSION_TIMEOUT;
        let mut kept = VecDeque::new();
        let result = loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            match self.next(wait) {
                Some(Ok(RunnerOutput::Response(response))) => {
                    break response.validate(request).map(|()| response.admission);
                }
                Some(Ok(event @ RunnerOutput::Event(_))) => kept.push_back(event),
                // A refusal, malformed line, closed stream or timeout leaves the
                // durable admission Unknown: nothing is inferred.
                Some(Ok(RunnerOutput::Refusal(_)) | Err(())) | None => break Err(uncertain()),
            }
        };
        kept.extend(self.pending.drain(..));
        self.pending = kept;
        result
    }

    fn supervise(&mut self) -> Exit {
        // The first grant must reach the host before any admission request.
        if self.renew_if_due().is_err() {
            return self.wait_fenced();
        }
        let repository = self.repository;
        let workspace = self.driver.workspace.clone();
        let launch = self.driver.launch.clone();
        let transport = PipeTransport(RefCell::new(&mut *self));
        let admitted = durable_execution::admit(repository, &transport, &workspace, &launch);
        match admitted.map(|runtime| runtime.admission) {
            Ok(Admission::Accepted { admission_id }) => {
                // As for any provider (docs/dispatch.md): the one-shot provisioning
                // marker is reconciled only after the launch succeeded and every
                // resource identity is retained. The retained `durable-runtime`
                // identity and its capacity remain until verified retirement; the
                // marker row itself stays and still prevents another launch.
                if self
                    .repository
                    .retire_agent_assignment_resource(
                        &self.driver.launch.assignment_id,
                        "provision-intent",
                        "once",
                    )
                    .is_err()
                {
                    tracing::warn!(
                        assignment_id = %self.driver.launch.assignment_id,
                        code = "durable-provision-intent-retained",
                        "launch succeeded but its provisioning marker stays retained"
                    );
                }
                self.admission = Some(admission_id);
            }
            // Rejected before intent (failed bootstrap, profile or authentication),
            // or Unknown: no execution continues under this grant.
            Ok(_) | Err(_) => self.fence(Some(Failure::AuthorityUnavailable)),
        }
        self.run_loop()
    }

    fn wait_fenced(&mut self) -> Exit {
        self.renew = false;
        self.run_loop()
    }

    fn run_loop(&mut self) -> Exit {
        let mut sent = HashSet::new();
        let mut last_poll = Instant::now()
            .checked_sub(recovery::FOLLOWUP_POLL)
            .unwrap_or_else(Instant::now);
        loop {
            match self.driver.stops.try_recv() {
                Ok(done) => {
                    self.fence(None);
                    return Exit::Stop(Some(done));
                }
                Err(TryRecvError::Disconnected) => {
                    self.fence(None);
                    return Exit::Stop(None);
                }
                Err(TryRecvError::Empty) => {}
            }
            if self.renew
                && let Some(_admission) = &self.admission
                && last_poll.elapsed() >= recovery::FOLLOWUP_POLL
            {
                last_poll = Instant::now();
                recovery::send_followups(
                    self.repository,
                    &self.driver.workspace,
                    &self.driver.launch.assignment_id,
                    &mut self.pipe,
                    &mut sent,
                );
            }
            match self.next(POLL) {
                Some(Ok(RunnerOutput::Event(RunnerEvent::Settled { .. }))) => self.settled(),
                Some(Ok(RunnerOutput::Event(RunnerEvent::FollowupAccepted {
                    operation_id,
                    admission_id,
                    ..
                }))) => recovery::record_followup_event(
                    self.repository,
                    &self.driver.workspace,
                    &self.driver.launch.assignment_id,
                    &operation_id,
                    &admission_id,
                ),
                // A runtime failure, any unexpected reply or output outside
                // protocol 1 is terminal for renewal. Nothing is inferred.
                Some(
                    Ok(
                        RunnerOutput::Event(RunnerEvent::RuntimeFailed {})
                        | RunnerOutput::Response(_)
                        | RunnerOutput::Refusal(_),
                    )
                    | Err(()),
                ) => self.fence(Some(Failure::AuthorityUnavailable)),
                None if self.disconnected => {
                    return if self.renew {
                        Exit::Crashed
                    } else {
                        Exit::Ended
                    };
                }
                Some(Ok(RunnerOutput::Event(_))) | None => {}
            }
        }
    }

    fn settled(&mut self) {
        let Some(admission) = self.admission.clone() else {
            self.fence(Some(Failure::AuthorityUnavailable));
            return;
        };
        let answer = self.runtime.block_on(recovery::inspect_answer(
            self.repository,
            &self.driver.workspace,
            &self.driver.launch,
            &admission,
            &self.driver.settings.components,
        ));
        match answer {
            // Keep renewing: recorded follow-ups remain deliverable.
            Ok(AnswerObservation::OutcomeAccepted) => {
                self.driver.set(DriverState::Live(AgentObservation::Idle));
            }
            // A missing Outcome stays an intervention state, never a completion.
            // The lease stays current (Forge may still refuse it) so the operator's
            // recorded follow-up can reach the same conversation.
            Ok(AnswerObservation::MissingOutcome) => {
                self.driver
                    .set(DriverState::Live(AgentObservation::Blocked));
            }
            Err(_) => self.fence(None),
        }
    }
}

/// Production admission transport over the directly spawned host's private pipe
/// (T22 protocol 1). Success of a write is never admission; only the runner's
/// correlated response is, and a lost reply leaves the durable result Unknown.
struct PipeTransport<'l, 'd, 'c>(RefCell<&'l mut Live<'d, 'c>>);

impl Transport for PipeTransport<'_, '_, '_> {
    fn admit(&self, launch: &Launch) -> Result<Admission, RegistrationError> {
        let mut live = self.0.borrow_mut();
        if *launch != live.driver.launch {
            return Err(uncertain());
        }
        let request = live.request(RunnerOperation::Admit {
            launch: launch.clone(),
        });
        live.exchange(&request)
    }

    fn observe_admission(&self, launch: &Launch) -> Result<Admission, RegistrationError> {
        let mut live = self.0.borrow_mut();
        if *launch != live.driver.launch {
            return Err(uncertain());
        }
        let request = live.request(RunnerOperation::Observe {
            admission_operation_id: launch.operation_id.clone(),
        });
        live.exchange(&request)
    }
}

/// Map a launcher failure to the adapter port without leaking private details.
pub(super) fn adapter_error(error: &RegistrationError) -> AgentAdapterError {
    match error.code {
        "invalid-durable-profile" | "durable-model-auth-unavailable" | "durable-launch-invalid" => {
            AgentAdapterError::InvalidRequest(error.code.into())
        }
        "durable-launch-claimed" => AgentAdapterError::Conflict(error.code.into()),
        _ => AgentAdapterError::Transient(error.code.into()),
    }
}

/// Explicitly selected matched execution components (never `PATH` or the checkout).
#[derive(Clone, Eq, PartialEq)]
pub struct Execution {
    /// Host, Node, runner entry and the recovery/answer inspectors.
    pub components: recovery::Components,
    /// The matched interrupted-effect inspector; its directory holds the attested sources.
    pub effect_inspector: PathBuf,
}

impl std::fmt::Debug for Execution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Execution").finish_non_exhaustive()
    }
}

impl Execution {
    /// Select a packaged layout: the runner entry and every inspector are siblings
    /// in one matched source directory. Host and entry are both set or neither;
    /// a launch selection also requires the matched Node and effect inspector.
    /// # Errors
    /// Rejects partial or relative selection.
    pub fn select(
        node: Option<PathBuf>,
        effect_inspector: Option<PathBuf>,
        host: Option<PathBuf>,
        entry: Option<PathBuf>,
    ) -> Result<Option<Self>, RegistrationError> {
        let (host, entry) = match (host, entry) {
            (None, None) => return Ok(None),
            (Some(host), Some(entry)) => (host, entry),
            _ => {
                return Err(invalid(
                    "configure both FORGE_DURABLE_RUNNER_HOST and FORGE_DURABLE_RUNNER_ENTRY, or neither",
                ));
            }
        };
        let (Some(node), Some(effect_inspector)) = (node, effect_inspector) else {
            return Err(invalid(
                "a durable runner selection also requires the matched Node and effect inspector",
            ));
        };
        let sources = entry
            .parent()
            .ok_or_else(|| invalid("runner entry has no component directory"))?
            .to_path_buf();
        if !host.is_absolute() || !entry.is_absolute() || !node.is_absolute() {
            return Err(invalid(
                "select explicit absolute durable runner components",
            ));
        }
        Ok(Some(Self {
            components: recovery::Components {
                host,
                host_arguments: vec![],
                node,
                runner: entry,
                inspector: sources.join("inspect-recovery.ts"),
                answer_inspector: sources.join("inspect-answer.ts"),
            },
            effect_inspector,
        }))
    }
}

/// Compose the durable provider factory exactly as the daemon does. Without an
/// execution selection, new durable launches and recovery stay refused while
/// historical retirement keeps working. The caller wraps the result in the
/// release gate; this function grants no launch eligibility.
/// # Errors
/// Rejects invalid components, failed component attestation or unavailable identity.
pub fn compose(
    database: &Path,
    inspection: Option<&super::operator::InspectionComponents>,
    execution: Option<&Execution>,
    model_key_file: Option<PathBuf>,
    original: Box<dyn forge_app::AgentDispatchAdapterFactory>,
) -> Result<retirement::Factory, RegistrationError> {
    let mut controller = retirement::Controller::default();
    if let Some(inspection) = inspection {
        controller = controller.with_inspection(inspection.clone())?;
    }
    let (launcher, recovery) = match execution {
        None => (None, None),
        Some(execution) => {
            let effects = super::operator::InspectionComponents {
                node: execution.components.node.clone(),
                inspector: execution.effect_inspector.clone(),
            };
            if inspection.is_none() {
                controller = controller.with_inspection(effects.clone())?;
            }
            let controller = Arc::new(controller);
            let sources = execution
                .effect_inspector
                .parent()
                .ok_or_else(|| invalid("effect inspector has no component directory"))?;
            let component_hash = attest_component_hash(&execution.components.node, sources)?;
            let launcher = Launcher::new(
                Settings {
                    database: database.to_path_buf(),
                    components: execution.components.clone(),
                    component_hash,
                    forge_revision: current_forge_revision(),
                    model_key_file,
                },
                Arc::new(DaemonAuthority::new()?.with_inspection(effects)?),
                controller.clone(),
            )?;
            let recovery = recovery::Controller::new(
                database.to_path_buf(),
                execution.components.clone(),
                controller.clone(),
            )?;
            return Ok(retirement::Factory {
                database: database.to_path_buf(),
                controller,
                launcher: Some(Arc::new(launcher)),
                recovery: Some(Arc::new(recovery)),
                original,
            });
        }
    };
    Ok(retirement::Factory {
        database: database.to_path_buf(),
        controller: Arc::new(controller),
        launcher,
        recovery,
        original,
    })
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
