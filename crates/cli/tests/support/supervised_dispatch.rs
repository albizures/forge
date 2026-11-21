//! Production durable composition (`forge_server::durable_authorization::launch`)
//! behind the production release gate, with only two test-only substitutions:
//! controlled launch eligibility and the runner entry (the shipped Check runner
//! with a faux model provider). The host is this test binary re-executing the
//! same `forge_runner_host::supervisor` library as the shipped executable.
//!
//! A thin wrapper counts provisions and, for the two bootstrap fault scenarios
//! only, replaces the delivered capability file or daemon endpoint. Launch,
//! identity, storage, host spawn, lease driver, admission transport, recovery
//! and retirement are the production code paths.
use forge_app::{
    AgentAdapterError, AgentAssignment, AgentDispatchAdapter, AgentDispatchAdapterFactory,
    AgentDispatchContext, AgentDispatchRepository, AgentObservation, AgentProfile, Application,
    DispatchWorkspace,
    durable_execution::{
        Launch, Repository as _,
        completion::{AnswerObservation, Repository as _},
    },
};
use forge_server::durable_authorization::{
    DaemonAuthority,
    launch::{DriverState, Execution},
    recovery,
    release_gate::{LaunchControl, ReleaseGate},
    retirement,
};
use forge_store::SqliteProjectRepository;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    os::{
        linux::net::SocketAddrExt,
        unix::{
            fs::PermissionsExt,
            net::{SocketAddr, UnixStream},
        },
    },
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[path = "supervised_recovery.rs"]
mod recovery_trace;

pub struct Report {
    pub launch: Launch,
    pub directory: PathBuf,
    pub answer: Option<AnswerObservation>,
    pub capability_file: PathBuf,
}
impl Report {
    /// After retirement the original host exits on its own; this only asserts
    /// that no process of this fixture survives (and removes one if it does).
    pub fn finish_retired(&mut self, _secret: &str) {
        terminate(&self.directory);
    }
}

#[derive(Clone)]
pub struct SupervisedDispatch {
    pub root: PathBuf,
    pub database: PathBuf,
    pub node: PathBuf,
    pub git: PathBuf,
    pub runner: PathBuf,
    pub cli: PathBuf,
    pub endpoint: String,
    pub bootstrap_endpoint: Option<String>,
    pub credential: PathBuf,
    pub component_hash: String,
    pub mode: String,
    pub provisions: Arc<AtomicUsize>,
    /// Real delivered capability file and launch context per Assignment.
    pub deliveries: Arc<Mutex<HashMap<String, AgentDispatchContext>>>,
    /// The single production composition of this fixture's daemon lifetime.
    pub production: Arc<retirement::Factory>,
    /// Explicit controlled eligibility for this verification composition only.
    pub launch: LaunchControl,
}

/// Original-backend stand-in: any non-durable or fallback call fails the test.
pub struct NoHerdr;
impl AgentDispatchAdapterFactory for NoHerdr {
    fn validate_profile(&self, _: &AgentProfile) -> Result<(), AgentAdapterError> {
        panic!("no Herdr fallback")
    }
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        panic!("no Herdr fallback")
    }
}

/// The test's matched execution components: the shipped runner sources and
/// inspectors, the faux-provider worker as entry, and this binary as host.
pub fn execution(node: &Path, runner: &Path) -> Execution {
    let src = runner.join("src");
    Execution {
        components: recovery::Components {
            host: std::env::current_exe().unwrap().canonicalize().unwrap(),
            host_arguments: vec!["--supervisor".into()],
            node: node.into(),
            runner: runner.join("test/supervised-check-worker.mjs"),
            inspector: src.join("inspect-recovery.ts"),
            answer_inspector: src.join("inspect-answer.ts"),
        },
        effect_inspector: src.join("inspect-effects.ts"),
    }
}

/// One production composition, as `application_from_config` builds it.
pub fn compose(
    database: &Path,
    node: &Path,
    runner: &Path,
    credential: &Path,
) -> retirement::Factory {
    let execution = execution(node, runner);
    forge_server::durable_authorization::launch::compose(
        database,
        None,
        Some(&execution),
        &[],
        Some(credential.into()),
        Box::new(NoHerdr),
    )
    .unwrap()
}

impl SupervisedDispatch {
    pub fn app(&self) -> Application {
        let open = || Box::new(SqliteProjectRepository::open(&self.database).unwrap());
        Application::with_managed_worktrees_and_agent_dispatch(
            open(),
            open(),
            Box::new(forge_server::GitWorktreeAdapter),
            open(),
            forge_server::assignment_delivery::adapter_factory(
                // Every scenario runs through the production release gate and its
                // persisted rollback, with controlled eligibility granted explicitly.
                Box::new(ReleaseGate {
                    control: self.launch.clone(),
                    rollback_store: Some(self.database.clone()),
                    inner: Box::new(self.clone()),
                }),
                self.endpoint.clone(),
                self.root.join("assignment-capabilities"),
                self.cli.clone(),
            ),
            Box::new(forge_server::GitInspector),
        )
        .with_workspace_repository(open())
    }

    /// Fixture-wide scripted data for the faux model and controlled effect only.
    pub fn write_fixture(&self) {
        let path = self.root.join("controlled-tracer.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "mode": self.mode,
                "git": self.git,
                "effect_script": self.runner.join("test/supervised-check-effect.mjs"),
            }))
            .unwrap(),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    pub fn state(&self, assignment: &str) -> Option<DriverState> {
        self.production.launcher.as_ref().unwrap().state(assignment)
    }

    fn wait_state(&self, assignment: &str, done: impl Fn(DriverState) -> bool) -> DriverState {
        wait(|| self.state(assignment).filter(|state| done(*state)))
    }

    pub fn directory(store: &SqliteProjectRepository, launch: &Launch) -> PathBuf {
        DaemonAuthority::new()
            .unwrap()
            .prepare_directory(store, "test", launch)
            .unwrap()
    }

    /// Observe a production-launched Assignment to its scenario end state. The
    /// only test interventions are releasing the faux model's barrier and, in
    /// the `recovered` scenario, killing the worker at its injected boundary.
    #[allow(clippy::too_many_lines)] // One ordered observation of a real launch.
    pub fn drive(&self, assignment: &AgentAssignment) -> Report {
        let store = SqliteProjectRepository::open(&self.database).unwrap();
        let id = &assignment.id;
        let context = self.deliveries.lock().unwrap().get(id).cloned().unwrap();
        let capability_file = context
            .capability_delivery
            .as_ref()
            .unwrap()
            .secret_file
            .clone();
        let launch = store.runtime("test", id).unwrap().unwrap().launch;
        // Production identity: real Revision and attested matched components.
        assert_eq!(
            launch.forge_revision,
            forge_server::durable_authorization::launch::current_forge_revision()
        );
        assert_eq!(launch.component_hash, self.component_hash);
        let attempt = store.assignment_check_attempt(id).unwrap().unwrap();
        assert_eq!(launch.attempt_id, attempt.id);
        let directory = Self::directory(&store, &launch);
        let cwd = context.agent_workspace.clone().unwrap();
        assert!(!directory.starts_with(&cwd));
        if matches!(
            self.mode.as_str(),
            "bootstrap-denied" | "bootstrap-uncertain" | "revoked"
        ) {
            let state = self.wait_state(id, |s| s != DriverState::Live(AgentObservation::Working));
            assert!(matches!(
                state,
                DriverState::Live(AgentObservation::Blocked)
                    | DriverState::Ended
                    | DriverState::Reaped
            ));
            return Report {
                launch,
                directory,
                answer: None,
                capability_file,
            };
        }
        wait(|| store.assignment_context_read(id).unwrap().then_some(()));
        let (existing, replay) = self.app().dispatch_record(assignment.record_id).unwrap();
        assert!(replay);
        assert_eq!(existing.id, *id);
        assert_eq!(self.provisions.load(Ordering::SeqCst), 1);
        // The shared kernel flock fences another host before any writer.
        let duplicate = Command::new(std::env::current_exe().unwrap())
            .arg("--supervisor")
            .args([&directory, &cwd, &self.node])
            .arg(self.runner.join("test/supervised-check-worker.mjs"))
            .env_clear()
            .output()
            .unwrap();
        assert!(!duplicate.status.success());
        assert!(String::from_utf8_lossy(&duplicate.stderr).contains("runner-owner-busy"));
        if self.mode == "accepted" {
            self.duplicate_provision(&context);
        }
        let owner = owner(&directory);
        let pid = owner["worker_pid"].as_u64().unwrap();
        let address = SocketAddr::from_abstract_name(format!("forge-t07-start-{pid}")).unwrap();
        UnixStream::connect_addr(&address)
            .unwrap()
            .write_all(b"start\n")
            .unwrap();
        if self.mode == "recovered" {
            return self.recover(&store, launch, directory, capability_file);
        }
        if self.mode == "packaged" {
            // T24: the first launch runs the Installation's packaged host and Node.
            let installed = self.node.parent().and_then(Path::parent).unwrap();
            let record = super::supervised_dispatch::owner(&directory);
            let exe = |pid: &Value| {
                fs::read_link(format!("/proc/{}/exe", pid.as_u64().unwrap())).unwrap()
            };
            assert_eq!(
                exe(&record["owner"]["host_pid"]),
                installed.join(forge_app::durable_execution::bundle::HOST)
            );
            assert_eq!(exe(&record["worker_pid"]), self.node);
            self.crash_at_boundary(&store, &launch, &directory);
            return Report {
                launch,
                directory,
                answer: None,
                capability_file,
            };
        }
        let state = self.wait_state(id, |s| {
            matches!(
                s,
                DriverState::Live(AgentObservation::Idle | AgentObservation::Blocked)
            )
        });
        let accepted = state == DriverState::Live(AgentObservation::Idle);
        assert_eq!(
            accepted,
            !matches!(self.mode.as_str(), "missing" | "rejected")
        );
        let runtime = store.runtime("test", id).unwrap().unwrap();
        let forge_app::durable_execution::Admission::Accepted { ref admission_id } =
            runtime.admission
        else {
            panic!("missing committed admission")
        };
        assert_eq!(self.observe(&directory, &launch, admission_id), "answered");
        // Exact replay of the driver's committed answer observation.
        let answer = store.observe_answer("test", &launch).unwrap();
        Report {
            launch,
            directory,
            answer: Some(answer),
            capability_file,
        }
    }

    /// Exclusive ownership across a duplicate provision in this daemon lifetime and
    /// a restarted daemon's fresh composition: neither spawns, adopts or recovers.
    fn duplicate_provision(&self, context: &AgentDispatchContext) {
        let store = SqliteProjectRepository::open(&self.database).unwrap();
        // Let the live driver finish reconciling its one-shot provisioning marker
        // after the accepted admission, so the snapshot is not racing it.
        let before = wait(|| {
            let info = store.agent_assignment_info(&context.assignment.id).unwrap();
            info.resources
                .iter()
                .any(|r| r.resource_kind == "provision-intent" && r.retired_at_millis.is_some())
                .then_some(info)
        });
        let adapter = self
            .production
            .adapter_for(&context.assignment.profile)
            .unwrap();
        adapter.provision(context, &store).unwrap();
        let restarted = compose(&self.database, &self.node, &self.runner, &self.credential);
        let adapter = restarted.adapter_for(&context.assignment.profile).unwrap();
        assert!(matches!(
            adapter.provision(context, &store),
            Err(AgentAdapterError::Conflict(code)) if code == "durable-launch-claimed"
        ));
        // Unavailable local supervision is uncertainty, never absence or completion.
        assert_eq!(adapter.observe(context).unwrap(), AgentObservation::Unknown);
        // A live original owner is not crash proof: recovery refuses to compete.
        assert!(adapter.recover(context).is_err());
        assert!(
            restarted
                .launcher
                .as_ref()
                .unwrap()
                .state(&context.assignment.id)
                .is_none()
        );
        assert_eq!(
            store.agent_assignment_info(&context.assignment.id).unwrap(),
            before
        );
    }

    fn observe(&self, directory: &Path, launch: &Launch, admission: &str) -> String {
        let module = self.runner.join("src/storage.ts");
        let script = format!(
            "import {{observeAnswer}} from {}; process.stdout.write(await observeAnswer(process.argv[1], JSON.parse(process.argv[2]), process.argv[3]));",
            json!(module)
        );
        let output = Command::new(&self.node)
            .env_clear()
            .args(["--input-type=module", "-e", &script])
            .arg(directory)
            .arg(serde_json::to_string(launch).unwrap())
            .arg(admission)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "read-only answer observation failed"
        );
        String::from_utf8(output.stdout).unwrap()
    }
}

impl AgentDispatchAdapterFactory for SupervisedDispatch {
    fn validate_profile(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        assert_eq!(profile.provider, "pi-durable");
        self.production.validate_profile(profile)
    }
    fn validate_preflight(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        assert_eq!(profile.provider, "pi-durable");
        self.production.validate_preflight(profile)
    }
    fn adapter_for(
        &self,
        profile: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        assert_eq!(profile.provider, "pi-durable");
        Ok(Box::new(Injected {
            inner: self.production.adapter_for(profile)?,
            dispatch: self.clone(),
        }))
    }
}

struct Injected {
    inner: Box<dyn AgentDispatchAdapter>,
    dispatch: SupervisedDispatch,
}
impl AgentDispatchAdapter for Injected {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        self.inner.probe()
    }
    fn verify_workspace(&self, _: &str) -> Result<DispatchWorkspace, AgentAdapterError> {
        panic!("no Herdr calls")
    }
    fn validate_provision(&self, context: &AgentDispatchContext) -> Result<(), AgentAdapterError> {
        self.inner.validate_provision(context)
    }
    fn provision(
        &self,
        context: &AgentDispatchContext,
        resources: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        self.dispatch.provisions.fetch_add(1, Ordering::SeqCst);
        assert!(context.dispatch_workspace.is_none());
        let prompt = context.bootstrap_prompt.as_ref().unwrap();
        assert!(!prompt.contains("T07-SUPERVISED-BEHAVIOR"));
        assert!(prompt.contains("assignment bootstrap"));
        self.dispatch
            .deliveries
            .lock()
            .unwrap()
            .insert(context.assignment.id.clone(), context.clone());
        let mut injected = context.clone();
        let delivery = injected.capability_delivery.as_mut().unwrap();
        match self.dispatch.mode.as_str() {
            "bootstrap-denied" => {
                let path = self.dispatch.root.join("controlled-wrong-capability");
                fs::write(&path, "incorrect-credential").unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                delivery.secret_file = path;
            }
            "bootstrap-uncertain" => {
                delivery
                    .endpoint
                    .clone_from(self.dispatch.bootstrap_endpoint.as_ref().unwrap());
            }
            _ => {}
        }
        self.inner.provision(&injected, resources)?;
        if self.dispatch.mode == "revoked" {
            // Revocation immediately after the host spawn, before its first grant.
            SqliteProjectRepository::open(&self.dispatch.database)
                .unwrap()
                .close_agent_assignment(
                    &context.assignment.id,
                    forge_app::AgentAssignmentClosureReason::Retired,
                )
                .unwrap();
        }
        Ok(())
    }
    fn observe(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentObservation, AgentAdapterError> {
        self.inner.observe(context)
    }
    fn recover(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentObservation, AgentAdapterError> {
        self.inner.recover(context)
    }
    fn retire(
        &self,
        context: &AgentDispatchContext,
        resources: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        assert!(context.assignment.closed_at_millis.is_some());
        assert!(context.capability_delivery.is_none());
        self.inner.retire(context, resources)
    }
}

pub fn wait<T>(mut ready: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(value) = ready() {
            return value;
        }
        assert!(Instant::now() < deadline, "supervised condition timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn owner(directory: &Path) -> Value {
    wait(|| {
        fs::read(directory.join("owner.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    })
}

fn start_time(pid: u32) -> Option<(String, u32)> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    Some(((*fields.get(19)?).to_owned(), fields.get(1)?.parse().ok()?))
}

/// Test-only teardown of processes this fixture's production composition spawned:
/// the recorded worker group and its host, each verified by start time (and the
/// host by parentage). This is not retirement and asserts nothing about Forge state.
pub fn terminate(directory: &Path) {
    let Ok(bytes) = fs::read(directory.join("owner.json")) else {
        return;
    };
    let Ok(owner) = serde_json::from_slice::<Value>(&bytes) else {
        return;
    };
    if let Some(pid) = owner["worker_pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        && start_time(pid)
            .is_some_and(|(start, _)| Some(start.as_str()) == owner["worker_start"].as_str())
        && let Some(pid) = rustix::process::Pid::from_raw(pid.cast_signed())
    {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
    if let Some(pid) = owner["owner"]["host_pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        && start_time(pid).is_some_and(|(start, parent)| {
            Some(start.as_str()) == owner["owner"]["host_start"].as_str()
                && parent == std::process::id()
        })
        && let Some(pid) = rustix::process::Pid::from_raw(pid.cast_signed())
    {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
    }
}
