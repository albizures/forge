//! Explicitly injected T07 adapter, never production backend selection. Uses the
//! real protected delivery wrapper, canonical runtime mapping, supervisor and
//! daemon grant driver. Only the model and finite controlled test inputs are fake.
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, Application, DispatchWorkspace,
    assignment::BoundIdentity,
    authorization_lease::session::{Failure, SessionRepository, Tick},
    durable_execution::{
        Admission, Launch, Repository as _,
        completion::{AnswerObservation, Repository as _},
    },
};
use forge_rpc::durable_execution::{
    RunnerCompatibility, RunnerOperation, RunnerRequest, RunnerResponse,
};
use forge_server::{durable_authorization::DaemonAuthority, durable_profiles::prepare};
use forge_store::SqliteProjectRepository;
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::{
        linux::net::SocketAddrExt,
        unix::{
            fs::PermissionsExt,
            net::{SocketAddr, UnixStream},
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver},
    },
    time::Duration,
};

#[path = "supervised_recovery.rs"]
mod recovery;

pub struct Report {
    pub launch: Launch,
    pub directory: PathBuf,
    pub answer: Option<AnswerObservation>,
    pub events: Vec<Value>,
    pub capability_file: PathBuf,
    process: Option<Process>,
}
impl Report {
    pub fn finish_retired(&mut self, secret: &str) {
        self.process.take().unwrap().finish_redacted(secret);
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
    pub authority: Arc<DaemonAuthority>,
    pub mode: String,
    pub provisions: Arc<AtomicUsize>,
    pub reports: Arc<Mutex<Vec<Report>>>,
    pub retirement: Arc<forge_server::durable_authorization::retirement::Controller>,
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
                Box::new(self.clone()),
                self.endpoint.clone(),
                self.root.join("assignment-capabilities"),
                self.cli.clone(),
            ),
            Box::new(forge_server::GitInspector),
        )
        .with_workspace_repository(open())
    }

    fn spawn(&self, directory: &Path, worktree: &Path) -> Process {
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--supervisor")
            .args([directory, worktree, &self.node, &self.runner])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Process::new(child, directory)
    }

    #[allow(clippy::too_many_lines)] // One finite, assertion-heavy composed transport boundary.
    fn execute(&self, context: &AgentDispatchContext) {
        assert_eq!(self.provisions.fetch_add(1, Ordering::SeqCst), 0);
        assert!(context.dispatch_workspace.is_none());
        let delivery = context
            .capability_delivery
            .as_ref()
            .expect("real protected capability delivery");
        assert!(
            !context
                .bootstrap_prompt
                .as_ref()
                .unwrap()
                .contains("T07-SUPERVISED-BEHAVIOR")
        );
        assert!(
            context
                .bootstrap_prompt
                .as_ref()
                .unwrap()
                .contains("assignment bootstrap")
        );
        let store = SqliteProjectRepository::open(&self.database).unwrap();
        let cwd = context.agent_workspace.as_ref().unwrap();
        let prepared = prepare(&context.assignment.profile, cwd, Some(&self.credential)).unwrap();
        let attempt = store
            .assignment_check_attempt(&context.assignment.id)
            .unwrap()
            .unwrap();
        let id = &context.assignment.id;
        let launch = Launch {
            assignment_id: id.clone(),
            operation_id: format!("{id}-launch"),
            attempt_id: attempt.id.clone(),
            profile_revision: context.assignment.profile.revision,
            runtime_id: format!("{id}-runtime"),
            incarnation_id: format!("{id}-incarnation"),
            conversation_id: format!("{id}-conversation"),
            store_id: format!("{id}-store"),
            forge_revision: "controlled-source-tracer".into(),
            component_hash: self.component_hash.clone(),
            runtime_schema: 2,
        };
        assert_eq!(
            store.reserve("test", &launch).unwrap().admission,
            Admission::Pending
        );
        assert!(store.begin_agent_provision(id).unwrap());
        let directory = self
            .authority
            .prepare_directory(&store, "test", &launch)
            .unwrap();
        assert!(!directory.starts_with(cwd));
        let request = RunnerRequest {
            compatibility: RunnerCompatibility {
                protocol: 1,
                runtime_schema: 2,
                component_hash: self.component_hash.clone(),
            },
            assignment_id: id.clone(),
            runtime_id: launch.runtime_id.clone(),
            operation_id: launch.operation_id.clone(),
            operation: RunnerOperation::Admit {
                launch: launch.clone(),
            },
        };
        request.validate(&self.component_hash).unwrap();
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
        let secret_file = if self.mode == "bootstrap-denied" {
            let path = directory.join("controlled-wrong-capability");
            fs::write(&path, "incorrect-credential").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            path
        } else {
            delivery.secret_file.clone()
        };
        let input = json!({ "request": request, "bound": bound, "generation": attempt.generation,
            "configuration": prepared.configuration, "credential_file": prepared.credential_file,
            "capability": { "FORGE_ASSIGNMENT_ID": id, "FORGE_ASSIGNMENT_DAEMON_URL": self.bootstrap_endpoint.as_ref().unwrap_or(&delivery.endpoint),
                "FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE": secret_file, "FORGE_ASSIGNMENT_FORGE_CLI": delivery.cli_path },
            "mode": self.mode, "git": self.git,
            "effect_script": self.runner.with_file_name("supervised-check-effect.mjs"),
        });
        let file = directory.join("controlled-launch.json");
        fs::write(&file, serde_json::to_vec(&input).unwrap()).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        // Intent precedes the process effect. A lost report remains Unknown.
        assert!(store.begin_send("test", &launch).unwrap());
        let now = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        assert!(store.begin_agent_launch(id, now).unwrap());
        let mut process = self.spawn(&directory, cwd);
        let owned = process.event();
        assert_eq!(owned["event"], "owned");
        serde_json::from_value::<RunnerCompatibility>(owned["compatibility"].clone())
            .unwrap()
            .validate(&self.component_hash)
            .unwrap();
        // The actual shared kernel flock fences another host before any writer.
        let duplicate = Command::new(std::env::current_exe().unwrap())
            .arg("--supervisor")
            .args([&directory, cwd, &self.node, &self.runner])
            .env_clear()
            .output()
            .unwrap();
        assert!(!duplicate.status.success());
        assert!(String::from_utf8_lossy(&duplicate.stderr).contains("runner-owner-busy"));
        if self.mode == "revoked" {
            store
                .close_agent_assignment(id, forge_app::AgentAssignmentClosureReason::Retired)
                .unwrap();
        }
        if self.mode == "recovered" {
            self.execute_recovery(
                context,
                &store,
                recovery::Trace {
                    launch,
                    request,
                    directory,
                    process,
                    owned,
                },
            );
            return;
        }
        let mut service = tokio::runtime::Handle::current()
            .block_on(self.authority.bind_inspected(
                &store,
                "test".into(),
                launch.clone(),
                attempt.generation,
                &mut process.child,
            ))
            .unwrap();
        let mut events = vec![owned];
        let mut answer = None;
        if self.mode == "revoked" {
            assert!(service.tick(&store).is_err());
        } else {
            assert!(matches!(
                service.tick(&store).unwrap(),
                Tick::Delivered { .. }
            ));
            let (stop, stopped) = mpsc::channel();
            std::thread::scope(|scope| {
                let driver_store = &store;
                let driver_service = &mut service;
                let driver = scope.spawn(move || driver_service.run(driver_store, &stopped));
                loop {
                    let event = receive(&process.output);
                    let kind = event["event"].as_str().unwrap();
                    match kind {
                        "authorized" => {
                            let previous = events
                                .iter()
                                .filter(|event| event["event"] == "authorized")
                                .count();
                            assert_eq!(
                                event["sequence"].as_u64(),
                                Some(u64::try_from(previous + 1).unwrap())
                            );
                        }
                        "bootstrapped" => {
                            assert_eq!(event["calls"], 0);
                            assert_eq!(event["tools"], 0);
                            assert!(store.assignment_context_read(id).unwrap());
                            let (existing, replay) = self
                                .app()
                                .dispatch_record(context.assignment.record_id)
                                .unwrap();
                            assert!(replay);
                            assert_eq!(existing.id, *id);
                            assert_eq!(self.provisions.load(Ordering::SeqCst), 1);
                            let pid = events[0]["identity"]["worker_pid"].as_u64().unwrap();
                            let address =
                                SocketAddr::from_abstract_name(format!("forge-t07-start-{pid}"))
                                    .unwrap();
                            UnixStream::connect_addr(&address)
                                .unwrap()
                                .write_all(b"start\n")
                                .unwrap();
                        }
                        "admission" => {
                            let response: RunnerResponse =
                                serde_json::from_value(event["response"].clone()).unwrap();
                            response.validate(&request).unwrap();
                            assert!(matches!(response.admission, Admission::Accepted { .. }));
                            store
                                .record_result("test", &launch, &response.admission)
                                .unwrap();
                        }
                        "settled" => {
                            assert!(!self.mode.starts_with("bootstrap-"));
                            assert_eq!(event["calls"], if self.mode == "missing" { 1 } else { 2 });
                            assert_eq!(event["tools"], u64::from(self.mode != "missing"));
                            let runtime = store.runtime("test", id).unwrap().unwrap();
                            let Admission::Accepted { ref admission_id } = runtime.admission else {
                                panic!("missing committed admission")
                            };
                            assert_eq!(self.observe(&directory, &launch, admission_id), "answered");
                            answer = Some(store.observe_answer("test", &launch).unwrap());
                            events.push(event);
                            break;
                        }
                        "runtime-failed" => {
                            assert!(self.mode.starts_with("bootstrap-"));
                            assert_eq!(event["calls"], 0);
                            assert_eq!(event["tools"], 0);
                            events.push(event);
                            break;
                        }
                        _ => panic!("unexpected controlled worker event"),
                    }
                    events.push(event);
                }
                stop.send(()).unwrap();
                driver.join().unwrap().unwrap();
            });
        }
        let retirement_pipe = service.retirement_control();
        drop(service);
        // Failure retention is explicit even when no runtime submission exists.
        if self.mode.starts_with("bootstrap-") {
            store
                .retain_failure("test", &launch, Failure::AuthorityUnavailable)
                .unwrap();
        }
        let secret = fs::read_to_string(&delivery.secret_file).unwrap();
        let retained_process = if matches!(
            self.mode.as_str(),
            "retired" | "continuation-chain" | "chain-complete" | "review"
        ) {
            let mut placeholder = Command::new("/bin/true").spawn().unwrap();
            placeholder.wait().unwrap();
            let host = std::mem::replace(&mut process.child, placeholder);
            self.retirement
                .register(&store, "test", launch.clone(), host, retirement_pipe)
                .unwrap();
            Some(process)
        } else {
            process.finish_redacted(&secret);
            None
        };
        assert!(!serde_json::to_string(&events).unwrap().contains(&secret));
        self.reports.lock().unwrap().push(Report {
            launch,
            directory,
            answer,
            events,
            capability_file: delivery.secret_file.clone(),
            process: retained_process,
        });
        // Test teardown kills only this directly spawned owner/group. It does not
        // retire Forge resources or claim safe descendant absence in product state.
    }

    fn observe(&self, directory: &Path, launch: &Launch, admission: &str) -> String {
        let module = self
            .runner
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("src/storage.ts");
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
        prepare(profile, &self.root, Some(&self.credential))
            .map(|_| ())
            .map_err(|error| AgentAdapterError::InvalidRequest(error.code.into()))
    }
    fn adapter_for(
        &self,
        profile: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        assert_eq!(profile.provider, "pi-durable");
        // Historical cleanup must not reload profile sources/authentication.
        Ok(Box::new(self.clone()))
    }
}
impl AgentDispatchAdapter for SupervisedDispatch {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        Ok(())
    }
    fn verify_workspace(&self, _: &str) -> Result<DispatchWorkspace, AgentAdapterError> {
        panic!("no Herdr calls")
    }
    fn validate_provision(&self, context: &AgentDispatchContext) -> Result<(), AgentAdapterError> {
        prepare(
            &context.assignment.profile,
            context.agent_workspace.as_ref().unwrap(),
            Some(&self.credential),
        )
        .map(|_| ())
        .map_err(|error| AgentAdapterError::InvalidRequest(error.code.into()))
    }
    fn provision(
        &self,
        context: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        self.validate_provision(context)?;
        self.execute(context);
        Ok(())
    }
    fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        Ok(AgentObservation::Working)
    }
    fn recover(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        panic!("automatic recovery disabled")
    }
    fn retire(
        &self,
        context: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        assert!(context.assignment.closed_at_millis.is_some());
        assert!(context.capability_delivery.is_none());
        let store = SqliteProjectRepository::open(&self.database).unwrap();
        let runtime = store
            .runtime("test", &context.assignment.id)
            .unwrap()
            .unwrap();
        self.retirement
            .retire(&store, "test", &runtime.launch)
            .map_err(|error| AgentAdapterError::Cleanup(error.code.into()))
    }
}

fn lines(source: impl std::io::Read + Send + 'static) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(source).lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    rx
}
fn receive(output: &Receiver<String>) -> Value {
    let line = output
        .recv_timeout(Duration::from_secs(20))
        .expect("supervised worker event timed out");
    serde_json::from_str(&line).expect("worker output must be bounded structured events")
}
struct Process {
    child: Child,
    original_host: u32,
    original_start: String,
    output: Receiver<String>,
    errors: Receiver<String>,
    directory: PathBuf,
}
impl Process {
    fn new(mut child: Child, directory: &Path) -> Self {
        let original_host = child.id();
        let stat = fs::read_to_string(format!("/proc/{original_host}/stat")).unwrap();
        let original_start = stat
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .nth(19)
            .unwrap()
            .into();
        let output = lines(child.stdout.take().unwrap());
        let errors = lines(child.stderr.take().unwrap());
        Self {
            child,
            original_host,
            original_start,
            output,
            errors,
            directory: directory.into(),
        }
    }
    fn event(&self) -> Value {
        let line = self
            .output
            .recv_timeout(Duration::from_secs(20))
            .unwrap_or_else(|error| {
                panic!(
                    "controlled worker event {error:?}: {:?}",
                    self.errors.try_iter().collect::<Vec<_>>()
                );
            });
        serde_json::from_str(&line).unwrap()
    }
    fn finish_redacted(mut self, secret: &str) {
        self.terminate();
        for receiver in [&self.output, &self.errors] {
            loop {
                match receiver.recv_timeout(Duration::from_secs(20)) {
                    Ok(line) => {
                        assert!(!line.contains(secret));
                        assert!(!line.contains("T07-SUPERVISED-MODEL-KEY"));
                        assert!(!line.contains("T07-SUPERVISED-BEHAVIOR"));
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        panic!("controlled process output did not close")
                    }
                }
            }
        }
    }
    fn terminate(&mut self) {
        if let Ok(bytes) = fs::read(self.directory.join("owner.json"))
            && let Ok(owner) = serde_json::from_slice::<Value>(&bytes)
            && owner["owner"]["host_pid"] == self.original_host
            && let Some(pid) = owner["worker_pid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
        {
            let process_info = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            let start = process_info
                .rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().nth(19));
            if start == owner["worker_start"].as_str()
                && let Some(pid) = rustix::process::Pid::from_raw(pid.cast_signed())
            {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
        }
        // Test-only teardown after transferring the actual Child to the real
        // registry: verify this fixture's original spawn before forced cleanup.
        if self.child.id() != self.original_host {
            let stat = fs::read_to_string(format!("/proc/{}/stat", self.original_host))
                .unwrap_or_default();
            let fields: Vec<_> = stat
                .rsplit_once(')')
                .map(|(_, r)| r.split_whitespace().collect())
                .unwrap_or_default();
            if fields.get(19).copied() == Some(self.original_start.as_str())
                && fields.get(1).and_then(|p| p.parse::<u32>().ok()) == Some(std::process::id())
                && let Some(pid) = rustix::process::Pid::from_raw(self.original_host.cast_signed())
            {
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
                let _ = rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::empty());
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.terminate();
    }
}
