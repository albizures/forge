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
        self, Admission, Launch, Repository as _, Transport,
        completion::AnswerObservation,
        launch_failure::{self, Repository as _},
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

/// A launch failure point that tests can force (T26). Injection only ever
/// makes a launch fail at that point; it cannot enable or advance a launch.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Fault {
    /// Writing the runner's protected input fails.
    RuntimeInput,
    /// The launch intent is refused.
    LaunchIntent,
    /// Spawning the matched host fails (before any process exists).
    Spawn,
    /// The host's `owned` evidence is treated as invalid.
    Owned,
    /// Binding the spawned host fails.
    Bind,
    /// The first grant fails.
    Grant,
    /// The driver stops making progress after `owned`, before binding: a hung
    /// daemon, so a test can kill it while the host waits for its first grant.
    Stall,
}

/// Forced failure points; empty in every production composition.
#[derive(Clone, Default)]
struct Faults(Arc<Mutex<HashSet<Fault>>>);

impl Faults {
    fn inject(&self, fault: Fault) -> Result<(), RegistrationError> {
        if self.0.lock().map_or(true, |faults| faults.contains(&fault)) {
            return Err(RegistrationError::new(
                "durable-launch-fault",
                format!("durable launch failed at {fault:?}"),
            ));
        }
        Ok(())
    }
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

fn incompatible(message: &str) -> RegistrationError {
    RegistrationError::new("durable-components-incompatible", message.to_owned())
}

/// Identity of an executable file at composition, so launch preflight notices a
/// component replaced or modified afterwards without rehashing it every time.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Fingerprint {
    device: u64,
    inode: u64,
    length: u64,
    modified: (i64, i64),
}

impl Fingerprint {
    fn of(path: &Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).ok()?;
        metadata.is_file().then(|| Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
        })
    }
}

/// The matched Node must be exactly the version this Forge build pins.
fn require_pinned_node(node: &Path) -> Result<(), RegistrationError> {
    let output = Command::new(node)
        .env_clear()
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| incompatible("the matched Node cannot run"))?;
    let expected = format!(
        "v{}",
        forge_app::durable_execution::bundle::PINNED_NODE_VERSION
    );
    if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != expected {
        return Err(incompatible(&format!(
            "the matched Node is not the pinned {expected}; reinstall this Revision's runner bundle"
        )));
    }
    Ok(())
}

/// The matched host must implement this daemon's runner protocol and runtime
/// schema and come from this daemon's own Forge Revision.
fn require_compatible_host(
    components: &recovery::Components,
    forge_revision: &str,
) -> Result<(), RegistrationError> {
    let output = Command::new(&components.host)
        .env_clear()
        .args(&components.host_arguments)
        .arg(forge_runner_host::entry::COMPATIBILITY_FLAG)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| incompatible("the matched runner host cannot run"))?;
    let reported: forge_runner_host::entry::HostCompatibility =
        serde_json::from_slice(&output.stdout)
            .ok()
            .filter(|_| output.status.success())
            .ok_or_else(|| {
                incompatible("the matched runner host does not report its compatibility")
            })?;
    if reported.protocol != RUNNER_PROTOCOL_VERSION
        || reported.runtime_schema != DURABLE_RUNTIME_SCHEMA
    {
        return Err(incompatible(
            "the matched runner host implements a different runner protocol or runtime schema",
        ));
    }
    if reported.forge_revision != forge_revision {
        return Err(incompatible(
            "the matched runner host was built from a different Forge Revision than this daemon",
        ));
    }
    Ok(())
}

/// Composition-time identity of the launch components, checked again by
/// [`Launcher::preflight`] before every new launch.
struct Attestation {
    host: Fingerprint,
    node: Fingerprint,
}

fn unavailable_components(launch: &Launch) -> RegistrationError {
    RegistrationError::new(
        "durable-components-unavailable",
        format!(
            "the matched runner components that launched this Assignment (component hash {}) are not available to this daemon; register the original Revision's runner bundle with Forgeup (`forgeup runner resolve --component-hash {}`) and restart the daemon. Retained work never runs under other components",
            launch.component_hash, launch.component_hash
        ),
    )
}

#[derive(Clone)]
struct Entry {
    inspection: super::operator::InspectionComponents,
    execution: Option<Execution>,
    hash: std::sync::OnceLock<Option<String>>,
}

impl Entry {
    /// The attested component hash of these sources, computed once on demand
    /// with the entry's own matched Node. `None` when it cannot be attested.
    fn hash(&self) -> Option<&str> {
        self.hash
            .get_or_init(|| {
                let sources = self.inspection.inspector.parent()?;
                attest_component_hash(&self.inspection.node, sources).ok()
            })
            .as_deref()
    }
}

/// Every set of matched components this daemon was configured with: this
/// Revision's own, and those of other installed Revisions. An existing Launch
/// is served only by the set whose attested component hash equals the Launch's
/// `component_hash`. When no attested set matches, the operation refuses with
/// `durable-components-unavailable` instead of switching code. The only
/// exception is an inspection-only set that cannot be attested at all (a legacy
/// manual selection): it is used as before, and the inspector itself rejects
/// a Launch whose hash it does not match.
#[derive(Clone, Default)]
pub struct ComponentCatalog {
    entries: Vec<Entry>,
}

impl ComponentCatalog {
    /// Build from the daemon's configuration. Attestation is deferred until a
    /// retained Launch needs a set.
    #[must_use]
    pub fn new(
        inspection: Option<&super::operator::InspectionComponents>,
        execution: Option<&Execution>,
        retained: &[Execution],
    ) -> Self {
        let mut catalog = Self::default();
        if let Some(execution) = execution {
            catalog.push_execution(execution.clone());
        }
        if let Some(inspection) = inspection {
            catalog.push_inspection(inspection.clone());
        }
        for execution in retained {
            catalog.push_execution(execution.clone());
        }
        catalog
    }

    fn push_execution(&mut self, execution: Execution) {
        self.entries.push(Entry {
            inspection: execution.inspection(),
            execution: Some(execution),
            hash: std::sync::OnceLock::new(),
        });
    }

    /// Add an inspection-only set unless an existing set already uses it.
    pub fn push_inspection(&mut self, inspection: super::operator::InspectionComponents) {
        if self
            .entries
            .iter()
            .all(|entry| entry.inspection != inspection)
        {
            self.entries.push(Entry {
                inspection,
                execution: None,
                hash: std::sync::OnceLock::new(),
            });
        }
    }

    /// Record an already attested hash for the set using exactly these components.
    fn seed(&self, execution: &Execution, hash: &str) {
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.execution.as_ref() == Some(execution))
        {
            let _ = entry.hash.set(Some(hash.to_owned()));
        }
    }

    /// Whether any components are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Read-only inspection components for an existing Launch. `Ok(None)` when
    /// no durable components are configured at all.
    /// # Errors
    /// `durable-components-unavailable` when configured sets exist but none
    /// matches the Launch's component hash.
    pub fn inspection_for(
        &self,
        launch: &Launch,
    ) -> Result<Option<super::operator::InspectionComponents>, RegistrationError> {
        if self.entries.is_empty() {
            return Ok(None);
        }
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.hash() == Some(launch.component_hash.as_str()))
        {
            return Ok(Some(entry.inspection.clone()));
        }
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.execution.is_none() && entry.hash().is_none())
        {
            return Ok(Some(entry.inspection.clone()));
        }
        Err(unavailable_components(launch))
    }

    /// Execution components (host, Node, runner, inspectors) for an existing Launch.
    /// # Errors
    /// `durable-components-unavailable` when no configured set matches exactly.
    pub fn execution_for(&self, launch: &Launch) -> Result<Execution, RegistrationError> {
        self.entries
            .iter()
            .filter(|entry| entry.execution.is_some())
            .find(|entry| entry.hash() == Some(launch.component_hash.as_str()))
            .and_then(|entry| entry.execution.clone())
            .ok_or_else(|| unavailable_components(launch))
    }
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
    attestation: Attestation,
    authority: Arc<DaemonAuthority>,
    retirement: Arc<retirement::Controller>,
    slots: Mutex<HashMap<String, Slot>>,
    faults: Faults,
}

impl Launcher {
    /// Test seam (T26): make every later launch fail at `fault`. It can only
    /// cause a failure, never enable, skip or advance anything.
    #[doc(hidden)]
    pub fn inject_fault(&self, fault: Fault) {
        if let Ok(mut faults) = self.faults.0.lock() {
            faults.insert(fault);
        }
    }

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
        let attestation = Attestation {
            host: Fingerprint::of(&settings.components.host)
                .ok_or_else(|| incompatible("the matched runner host is missing"))?,
            node: Fingerprint::of(&settings.components.node)
                .ok_or_else(|| incompatible("the matched Node is missing"))?,
        };
        Ok(Self {
            settings,
            attestation,
            authority,
            retirement,
            slots: Mutex::new(HashMap::new()),
            faults: Faults::default(),
        })
    }

    /// Launch preflight for every matched component, before any allocation:
    /// all components present and canonical; host and Node unchanged since
    /// composition; Node is the pinned version; the host implements this
    /// daemon's runner protocol and runtime schema and comes from this daemon's
    /// Forge Revision; and the runner sources still attest the Launch component
    /// hash. Nothing is found through `PATH` or npm.
    /// # Errors
    /// `durable-components-incompatible` naming the failing component.
    pub fn preflight(&self) -> Result<(), RegistrationError> {
        let components = &self.settings.components;
        components.validate().map_err(|_| {
            incompatible("a matched runner component is missing or not at its canonical path")
        })?;
        if Fingerprint::of(&components.host).as_ref() != Some(&self.attestation.host) {
            return Err(incompatible(
                "the matched runner host changed since the daemon composed it; restart the daemon from a verified Installation",
            ));
        }
        if Fingerprint::of(&components.node).as_ref() != Some(&self.attestation.node) {
            return Err(incompatible(
                "the matched Node changed since the daemon composed it; restart the daemon from a verified Installation",
            ));
        }
        require_pinned_node(&components.node)?;
        require_compatible_host(components, &self.settings.forge_revision)?;
        let sources = components
            .inspector
            .parent()
            .ok_or_else(|| incompatible("the matched runner sources are missing"))?;
        if attest_component_hash(&components.node, sources)
            .ok()
            .as_deref()
            != Some(self.settings.component_hash.as_str())
        {
            return Err(incompatible(
                "the matched runner sources no longer attest the composed component hash (tampered or replaced)",
            ));
        }
        Ok(())
    }

    /// Validate the matched components, the explicit profile and host
    /// authentication for a new launch. The cwd is re-validated against the
    /// bound Managed Worktree at provision.
    /// # Errors
    /// Rejects incompatible components or profiles and missing or unprotected credentials.
    pub fn validate_profile(&self, profile: &AgentProfile) -> Result<(), RegistrationError> {
        self.preflight()?;
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
        self.preflight()?;
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
        self.preflight()?;
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
        // One transaction: the single-launch claim and the admission intent
        // (T26). An earlier attempt (this or a previous daemon lifetime, or a
        // closed Assignment) is never respawned, and a refused precondition
        // leaves no claim behind.
        let Some(reserved) = repository.claim(&workspace, &launch)? else {
            return Err(RegistrationError::new(
                "durable-launch-claimed",
                "durable launch was already claimed or the Assignment is closed; observe, recover or retire it",
            ));
        };
        if reserved.launch != launch || reserved.admission != Admission::Pending {
            return Err(uncertain());
        }
        // From here every failure keeps its specific diagnostic (T26).
        let failed = |failure: launch_failure::Failure, error: RegistrationError| {
            if repository
                .record_launch_failure(&workspace, &launch, failure)
                .is_err()
            {
                tracing::warn!(
                    assignment_id = %launch.assignment_id,
                    code = failure.code(),
                    "durable launch failure could not be retained"
                );
            }
            error
        };
        let directory = runtime_directory::mapped(&repository, &workspace, &launch, true)
            .map_err(|error| failed(launch_failure::Failure::RuntimeInput, error))?;
        if directory.starts_with(&cwd) || cwd.starts_with(&directory) {
            return Err(failed(
                launch_failure::Failure::RuntimeInput,
                invalid("runtime storage must be separate from the Managed Worktree"),
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
        self.faults
            .inject(Fault::RuntimeInput)
            .and_then(|()| {
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
                )
            })
            .map_err(|error| failed(launch_failure::Failure::RuntimeInput, error))?;
        if self.faults.inject(Fault::LaunchIntent).is_err()
            || !repository
                .begin_agent_launch(&id, now_millis()?)
                .map_err(|error| failed(launch_failure::Failure::LaunchIntent, error))?
        {
            return Err(failed(launch_failure::Failure::LaunchIntent, uncertain()));
        }
        let c = &self.settings.components;
        let spawned = self.faults.inject(Fault::Spawn).and_then(|()| {
            Command::new(&c.host)
                .args(&c.host_arguments)
                .args([&directory, &cwd, &c.node, &c.runner])
                .env_clear()
                .current_dir(&directory)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| uncertain())
        });
        // A failed spawn leaves no process: `Command::spawn` reports exec
        // failure only after the forked child is gone.
        let child = spawned.map_err(|error| failed(launch_failure::Failure::Spawn, error))?;
        let state = Arc::new(Mutex::new(DriverState::Live(AgentObservation::Working)));
        let (stop, stops) = mpsc::channel();
        let driver = Driver {
            faults: self.faults.clone(),
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
    faults: Faults,
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

    /// Retain the specific failure point (T26) beside the generic fence.
    fn failed(&self, repository: &SqliteProjectRepository, failure: launch_failure::Failure) {
        if repository
            .record_launch_failure(&self.workspace, &self.launch, failure)
            .is_err()
        {
            tracing::warn!(
                assignment_id = %self.launch.assignment_id,
                code = failure.code(),
                "durable launch failure could not be retained"
            );
        }
    }

    /// A launch that failed before any grant reached its host (T26). Close the
    /// private pipe: the host fences on the lost issuer, stops its worker and,
    /// as subreaper, writes its exact ungranted receipt before exiting. A
    /// granted host instead reaches lease expiry and its T25 receipt. Either
    /// way the original Child is reaped here, never dropped live, and nothing
    /// is signalled or adopted. Retirement then needs that host's own proof.
    fn abandon(self, mut child: Child) {
        self.set(DriverState::Ended);
        drop(child.stdin.take());
        // Dropping the driver closes its stop channel, so retirement never
        // waits on this thread.
        drop(self);
        let _ = child.wait();
    }

    #[allow(clippy::too_many_lines)] // One ordered sequence; every failure point reaps the host.
    fn run(self, mut child: Child) {
        let Ok(repository) = SqliteProjectRepository::open(&self.settings.database) else {
            tracing::warn!(
                code = "durable-launch-store-unavailable",
                "durable launch fenced"
            );
            self.abandon(child);
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            self.retain(&repository, Failure::AuthorityUnavailable);
            self.failed(&repository, launch_failure::Failure::Owned);
            self.abandon(child);
            return;
        };
        let output = outputs(stdout);
        // The host's `owned` evidence must name exactly the matched components.
        let owned = matches!(
            output.recv_timeout(OWNED_TIMEOUT),
            Ok(Ok(RunnerOutput::Event(RunnerEvent::Owned { ref compatibility, .. })))
                if compatibility.validate(&self.launch.component_hash).is_ok()
        ) && self.faults.inject(Fault::Owned).is_ok();
        if !owned {
            self.retain(&repository, Failure::AuthorityUnavailable);
            self.failed(&repository, launch_failure::Failure::Owned);
            self.abandon(child);
            return;
        }
        while self.faults.inject(Fault::Stall).is_err() {
            std::thread::sleep(POLL);
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        let runtime = match runtime {
            Ok(runtime) if self.faults.inject(Fault::Bind).is_ok() => runtime,
            _ => {
                self.retain(&repository, Failure::AuthorityUnavailable);
                self.failed(&repository, launch_failure::Failure::Bind);
                self.abandon(child);
                return;
            }
        };
        let bound = runtime.block_on(self.authority.bind_inspected(
            &repository,
            self.workspace.clone(),
            self.launch.clone(),
            self.generation,
            &mut child,
        ));
        let (exit, pipe) = match bound {
            // `bind_inspected` already retained its failure, fenced the session
            // and closed the private pipe with its dropped Authorization.
            Err(_) => {
                self.failed(&repository, launch_failure::Failure::Bind);
                drop(runtime);
                self.abandon(child);
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
                self.set(DriverState::Crashed);
                drop(self);
                let _ = child.wait();
            }
            Exit::Ended => {
                // Never drop a live host: wait however long its own proof takes.
                self.set(DriverState::Ended);
                drop(self);
                let _ = child.wait();
            }
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
        let granted =
            self.driver.faults.inject(Fault::Grant).is_ok() && self.renew_if_due().is_ok();
        if !granted {
            self.driver
                .retain(self.repository, Failure::AuthorityUnavailable);
            self.driver
                .failed(self.repository, launch_failure::Failure::Grant);
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
        "invalid-durable-profile"
        | "durable-model-auth-unavailable"
        | "durable-launch-invalid"
        | "durable-components-incompatible"
        | "durable-components-unavailable" => AgentAdapterError::InvalidRequest(error.code.into()),
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
    /// Derive every component from a Forgeup-attested runner bundle root, using
    /// only the fixed layout (`forge_app::durable_execution::bundle`).
    /// # Errors
    /// Rejects a relative root.
    pub fn from_bundle(root: &Path) -> Result<Self, RegistrationError> {
        use forge_app::durable_execution::bundle;
        if !root.is_absolute() {
            return Err(invalid("select an absolute attested runner bundle root"));
        }
        Ok(Self {
            components: recovery::Components {
                host: root.join(bundle::HOST),
                host_arguments: vec![],
                node: root.join(bundle::NODE),
                runner: root.join(bundle::ENTRY),
                inspector: root.join(bundle::RECOVERY_INSPECTOR),
                answer_inspector: root.join(bundle::ANSWER_INSPECTOR),
            },
            effect_inspector: root.join(bundle::EFFECT_INSPECTOR),
        })
    }

    /// The read-only interrupted-effect/transcript inspection pair of this set.
    #[must_use]
    pub fn inspection(&self) -> super::operator::InspectionComponents {
        super::operator::InspectionComponents {
            node: self.components.node.clone(),
            inspector: self.effect_inspector.clone(),
        }
    }

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
/// historical retirement keeps working. `retained` are other installed
/// Revisions' matched components: they serve only Launches with their exact
/// component hash (inspection, recovery, retirement), never a new launch. The
/// caller wraps the result in the release gate; this function grants no launch
/// eligibility.
/// # Errors
/// Rejects invalid components, failed component attestation or unavailable identity.
pub fn compose(
    database: &Path,
    inspection: Option<&super::operator::InspectionComponents>,
    execution: Option<&Execution>,
    retained: &[Execution],
    model_key_file: Option<PathBuf>,
    original: Box<dyn forge_app::AgentDispatchAdapterFactory>,
) -> Result<retirement::Factory, RegistrationError> {
    if let Some(inspection) = inspection {
        inspection.validate()?;
    }
    let catalog = Arc::new(ComponentCatalog::new(inspection, execution, retained));
    let controller = Arc::new(retirement::Controller::with_catalog(catalog.clone()));
    let Some(execution) = execution else {
        return Ok(retirement::Factory {
            database: database.to_path_buf(),
            controller,
            launcher: None,
            recovery: None,
            original,
        });
    };
    let sources = execution
        .effect_inspector
        .parent()
        .ok_or_else(|| invalid("effect inspector has no component directory"))?;
    let component_hash = attest_component_hash(&execution.components.node, sources)?;
    catalog.seed(execution, &component_hash);
    let launcher = Launcher::new(
        Settings {
            database: database.to_path_buf(),
            components: execution.components.clone(),
            component_hash,
            forge_revision: current_forge_revision(),
            model_key_file,
        },
        Arc::new(DaemonAuthority::new()?.with_inspection(execution.inspection())?),
        controller.clone(),
    )?;
    let recovery = recovery::Controller::new(
        database.to_path_buf(),
        execution.components.clone(),
        controller.clone(),
    )?
    .with_catalog(catalog);
    Ok(retirement::Factory {
        database: database.to_path_buf(),
        controller,
        launcher: Some(Arc::new(launcher)),
        recovery: Some(Arc::new(recovery)),
        original,
    })
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
