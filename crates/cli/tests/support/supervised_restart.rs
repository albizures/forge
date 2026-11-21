//! T25: a real daemon-process kill while a production-launched runner is live.
//!
//! A separate daemon process (this test binary re-executed with `--daemon`)
//! composes the production launcher, dispatches the Task and owns the host's
//! pipes and registry. The test SIGKILLs it at an injected point: before the
//! first generation commit, or inside a long-running tool that ignores
//! cancellation. The orphaned host loses renewal, fences at lease expiry,
//! proves descendant absence and exits. This test process's own production
//! composition never launched the Assignment: it is the restarted daemon.
//! It recovers or retires the Assignment from the host's protected receipt.
//! The RPC listener stays in this process; only the launching daemon dies.
use super::{
    AgentDispatchRepository, Command, Path, PathBuf, RecordRepository, SqliteProjectRepository,
    SupervisedDispatch, Value, fs, json, supervised_dispatch,
};
use forge_app::{
    AgentObservation,
    durable_execution::{
        Admission, Launch, Repository as _,
        completion::{AnswerObservation, Repository as _},
    },
};
use std::{
    io::{BufRead, BufReader, Write},
    os::{
        linux::net::SocketAddrExt,
        unix::net::{SocketAddr, UnixStream},
    },
    process::Stdio,
    sync::{Arc, Mutex, atomic::AtomicUsize},
    time::{Duration, Instant},
};

/// Everything the launching daemon process needs to compose the same fixture.
pub fn write_config(adapter: &SupervisedDispatch) -> PathBuf {
    let path = adapter.root.join("controlled-daemon.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "root": adapter.root, "database": adapter.database, "node": adapter.node,
            "git": adapter.git, "runner": adapter.runner, "cli": adapter.cli,
            "endpoint": adapter.endpoint, "credential": adapter.credential,
            "component_hash": adapter.component_hash, "mode": adapter.mode,
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

/// `--daemon CONFIG RECORD`: compose production, dispatch, then keep running
/// until killed. Prints the Assignment ID once the host is spawned.
pub fn daemon(config: &Path, record: u64) -> ! {
    let value: Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
    let path = |key: &str| PathBuf::from(value[key].as_str().unwrap());
    let (node, runner) = (path("node"), path("runner"));
    let adapter = SupervisedDispatch {
        root: path("root"),
        database: path("database"),
        production: Arc::new(supervised_dispatch::compose(
            &path("database"),
            &node,
            &runner,
            &path("credential"),
        )),
        node,
        git: path("git"),
        runner,
        cli: path("cli"),
        endpoint: value["endpoint"].as_str().unwrap().into(),
        bootstrap_endpoint: None,
        credential: path("credential"),
        component_hash: value["component_hash"].as_str().unwrap().into(),
        mode: value["mode"].as_str().unwrap().into(),
        provisions: Arc::new(AtomicUsize::new(0)),
        deliveries: Arc::new(Mutex::new(std::collections::HashMap::new())),
        launch: {
            let launch =
                forge_server::durable_authorization::release_gate::LaunchControl::default();
            assert!(launch.enable_controlled());
            launch
        },
    };
    // T26: a `launch-fault-restart-*` daemon fails or stalls at its point.
    super::supervised_launch_fault::inject(&adapter);
    let id = match adapter.app().dispatch_record(record) {
        Ok((assignment, existing)) => {
            assert!(!existing);
            assignment.id
        }
        Err(_) if adapter.mode.starts_with("launch-fault-") => {
            SqliteProjectRepository::open(&adapter.database)
                .unwrap()
                .list_agent_assignments(record)
                .unwrap()[0]
                .id
                .clone()
        }
        Err(error) => panic!("{error:?}"),
    };
    println!("{id}");
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn cli(adapter: &SupervisedDispatch, args: &[&str]) -> std::process::Output {
    Command::new(&adapter.cli)
        .env_clear()
        .arg("dispatcher")
        .args(args)
        .args([
            "--json",
            "--server-url",
            &format!("{}/rpc", adapter.endpoint),
        ])
        .output()
        .unwrap()
}

fn status(adapter: &SupervisedDispatch, id: &str) -> Value {
    let output = cli(adapter, &["assignment-status", id, "--workspace", "test"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn retire(adapter: &SupervisedDispatch, id: &str) -> Result<(), String> {
    let output = cli(adapter, &["assignment-retire", id]);
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

fn names(directory: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The shipped runner exits on EOF: the orphaned host records ordinary crash
/// proof, which the restarted daemon recovers from as before T25.
fn crash_receipt(directory: &Path) {
    supervised_dispatch::wait(|| {
        let names = names(directory);
        assert!(
            !names.iter().any(|n| n.starts_with("watchdog")),
            "{names:?}"
        );
        names
            .iter()
            .any(|n| n.starts_with("recovery-crash-"))
            .then_some(())
    });
}

/// Wait (bounded by lease expiry, TERM grace and KILL) for the orphaned host's
/// post-fence receipt. A crash receipt would mean the fence path was not taken.
fn fence_receipt(directory: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let names = names(directory);
        assert!(
            !names.iter().any(|n| n.starts_with("recovery-crash-")),
            "the worker exited before the watchdog fence: {names:?}"
        );
        if let Some(name) = names.iter().find(|n| n.starts_with("watchdog-receipt-")) {
            assert!(names.iter().any(|n| n == "watchdog.json"));
            return name.clone();
        }
        assert!(Instant::now() < deadline, "no fence receipt: {names:?}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[allow(clippy::too_many_lines)] // One ordered restart journey per mode.
pub fn run(adapter: &SupervisedDispatch, store: &SqliteProjectRepository, record: u64) {
    let config = write_config(adapter);
    let mut daemon = Command::new(std::env::current_exe().unwrap())
        .arg("--daemon")
        .arg(&config)
        .arg(record.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(daemon.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let id = line.trim().to_owned();
    let launch: Launch =
        supervised_dispatch::wait(|| store.runtime("test", &id).unwrap().map(|r| r.launch));
    let directory = SupervisedDispatch::directory(store, &launch);
    let owner = supervised_dispatch::owner(&directory);
    let host = owner["owner"]["host_pid"].as_u64().unwrap();
    let worker = owner["worker_pid"].as_u64().unwrap();
    let address = SocketAddr::from_abstract_name(format!("forge-t07-start-{worker}")).unwrap();
    supervised_dispatch::wait(|| {
        UnixStream::connect_addr(&address)
            .ok()
            .and_then(|mut s| s.write_all(b"start\n").ok())
    });
    // The launching daemon committed the runner's admission before the fault.
    supervised_dispatch::wait(|| {
        matches!(
            store.runtime("test", &id).unwrap().unwrap().admission,
            Admission::Accepted { .. }
        )
        .then_some(())
    });
    let point = if adapter.mode == "daemon-restart-tool" {
        "controlled-tool-started"
    } else {
        "controlled-recovery-boundary.json"
    };
    supervised_dispatch::wait(|| directory.join(point).exists().then_some(()));
    // The real fault: SIGKILL the daemon process that launched and renews.
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    // This composition never launched it: it has no driver or Child to adopt.
    assert!(adapter.state(&id).is_none());
    let before = store.agent_assignment_info(&id).unwrap();
    assert!(before.capacity_reserved());
    let alive = |pid: u64| Path::new(&format!("/proc/{pid}")).exists();
    assert!(alive(host), "the orphaned host keeps ownership until proof");
    match adapter.mode.as_str() {
        "daemon-restart" | "daemon-restart-exit" => {
            recover_after_restart(adapter, store, &id, &launch, &directory);
        }
        "daemon-restart-retire" => retire_after_restart(adapter, store, &id, record, &directory),
        "daemon-restart-tool" => {
            effect_blocks_after_restart(adapter, store, &id, &launch, &directory);
        }
        other => panic!("unknown restart mode {other}"),
    }
    supervised_dispatch::terminate(&directory);
}

fn reconcile(adapter: &SupervisedDispatch) {
    let now = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    adapter
        .app()
        .reconcile_agent_dispatch("restarted-daemon", now)
        .unwrap();
}

/// Restart during model generation -> fence -> proof -> same-attempt recovery
/// by the restarted daemon to an accepted Outcome. No second writable owner.
fn recover_after_restart(
    adapter: &SupervisedDispatch,
    store: &SqliteProjectRepository,
    id: &str,
    launch: &Launch,
    directory: &Path,
) {
    let attempt = store.assignment_check_attempt(id).unwrap().unwrap();
    let recovery = adapter.production.recovery.as_ref().unwrap();
    let fenced = adapter.mode == "daemon-restart";
    if fenced {
        // While the orphaned host is live, recovery refuses to compete.
        assert!(recovery.recover("test", launch).is_err());
        fence_receipt(directory);
        assert_eq!(status(adapter, id)["runtime_fence"], "runtime-fence-proven");
    } else {
        crash_receipt(directory);
        assert!(status(adapter, id)["runtime_fence"].is_null());
    }
    reconcile(adapter);
    supervised_dispatch::wait(|| {
        (recovery.recover("test", launch).ok() == Some(AgentObservation::Idle)).then_some(())
    });
    assert_eq!(
        store.assignment_check_attempt(id).unwrap().unwrap(),
        attempt
    );
    assert_eq!(store.runtime("test", id).unwrap().unwrap().launch, *launch);
    assert_eq!(
        store.observe_answer("test", launch).unwrap(),
        AnswerObservation::OutcomeAccepted
    );
    if fenced {
        // The earlier fence is settled history beside the recovered owner.
        assert_eq!(status(adapter, id)["runtime_fence"], "runtime-fence-proven");
    }
    println!(
        "T25 daemon restart ({}) -> same-attempt recovery: passed",
        if fenced { "fence proof" } else { "crash proof" }
    );
}

/// Restart -> `assignment-retire` is refused with a specific code while the
/// orphaned host still owns the runtime -> host proof -> exact retirement
/// reconciliation releasing capacity -> exact retries -> purge.
fn retire_after_restart(
    adapter: &SupervisedDispatch,
    store: &SqliteProjectRepository,
    id: &str,
    record: u64,
    directory: &Path,
) {
    let early = retire(adapter, id).unwrap_err();
    assert!(early.contains("durable-runtime-owner-active"), "{early}");
    let held = store.agent_assignment_info(id).unwrap();
    assert!(held.capacity_reserved());
    let receipt = fence_receipt(directory);
    // Tampered proof: CLI/RPC status and retirement name the specific
    // retained-uncertainty code; nothing is released.
    let path = directory.join(&receipt);
    let original = fs::read(&path).unwrap();
    let mut tampered: Value = serde_json::from_slice(&original).unwrap();
    tampered["marker_contents"] = "tampered".into();
    fs::write(&path, serde_json::to_vec(&tampered).unwrap()).unwrap();
    let view = status(adapter, id);
    assert_eq!(view["runtime_fence"], "runtime-fence-unproven");
    assert!(
        view["uncertainty"]
            .as_array()
            .unwrap()
            .contains(&json!("runtime-fence-unproven")),
        "{view}"
    );
    let human = Command::new(&adapter.cli)
        .env_clear()
        .args(["dispatcher", "assignment-status", id, "--workspace", "test"])
        .args(["--server-url", &format!("{}/rpc", adapter.endpoint)])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&human.stdout).contains("Runtime fence: runtime-fence-unproven")
    );
    let unproven = retire(adapter, id).unwrap_err();
    assert!(
        unproven.contains("durable-runtime-fence-unproven"),
        "{unproven}"
    );
    assert!(store.agent_assignment_info(id).unwrap().capacity_reserved());
    fs::write(&path, &original).unwrap();
    retire(adapter, id).unwrap();
    let retired = store.agent_assignment_info(id).unwrap();
    assert!(retired.assignment.closed_at_millis.is_some());
    assert!(!retired.capacity_reserved());
    // Exact retry returns the original result without new effects.
    retire(adapter, id).unwrap();
    assert_eq!(store.agent_assignment_info(id).unwrap(), retired);
    let view = status(adapter, id);
    assert_eq!(view["runtime_fence"], "runtime-fence-proven");
    // Purge after clearing the retirement hold. Only test fixture files that
    // production never writes are removed by the test; all T25 artifacts stay.
    let resume = cli(adapter, &["resume", &record.to_string()]);
    assert!(
        resume.status.success(),
        "{}",
        String::from_utf8_lossy(&resume.stderr)
    );
    for name in names(directory) {
        if name.starts_with("controlled") {
            fs::remove_file(directory.join(name)).unwrap();
        }
    }
    let lifecycle = names(directory);
    for name in [
        "watchdog.json",
        receipt.as_str(),
        "owner.lock",
        "owner.json",
    ] {
        assert!(lifecycle.iter().any(|n| n == name), "{name}: {lifecycle:?}");
    }
    let output = cli(
        adapter,
        &[
            "assignment-purge-runtime",
            id,
            "--workspace",
            "test",
            "--operation-id",
            "t25-purge",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let purged: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(purged["state"], "purged");
    assert_eq!(purged["removal"], "removed");
    assert!(!directory.exists());
    assert!(store.show_record(record).is_ok());
    println!(
        "T25 daemon restart -> owner-active refusal -> fence proof -> retirement -> purge: passed"
    );
}

/// Restart inside a long-running tool -> fence -> proof. The committed
/// interrupted effect still blocks same-attempt recovery; retirement then
/// reconciles through the same receipt and keeps the effect's retained hold.
fn effect_blocks_after_restart(
    adapter: &SupervisedDispatch,
    store: &SqliteProjectRepository,
    id: &str,
    launch: &Launch,
    directory: &Path,
) {
    fence_receipt(directory);
    reconcile(adapter);
    let recovery = adapter.production.recovery.as_ref().unwrap();
    assert_ne!(
        recovery.recover("test", launch).ok(),
        Some(AgentObservation::Idle)
    );
    assert_ne!(
        store.observe_answer("test", launch).ok(),
        Some(AnswerObservation::OutcomeAccepted)
    );
    assert!(
        !directory.join("controlled-effects").exists(),
        "never replayed"
    );
    assert!(store.agent_assignment_info(id).unwrap().capacity_reserved());
    // Retirement reconciles the fenced runtime through the same receipt, but the
    // committed interrupted effect keeps finalization (and capacity) blocked.
    let refused = retire(adapter, id).unwrap_err();
    assert!(
        refused.contains("assignment-retirement-unreconciled"),
        "{refused}"
    );
    let view = status(adapter, id);
    assert_eq!(view["runtime_fence"], "runtime-fence-proven");
    assert!(
        view["uncertainty"]
            .as_array()
            .unwrap()
            .contains(&json!("interrupted-effect")),
        "{view}"
    );
    let info = store.agent_assignment_info(id).unwrap();
    assert!(info.capacity_reserved());
    let effect = info
        .resources
        .iter()
        .find(|r| r.resource_kind == "interrupted-effect" && r.retired_at_millis.is_none())
        .unwrap();
    assert!(
        info.resources
            .iter()
            .any(|r| r.resource_kind == "durable-runtime" && r.retired_at_millis.is_some()),
        "the receipt reconciled the runtime itself"
    );
    // The operator's audited effect resolution, then the same retirement.
    let effect: Value = serde_json::from_str(&effect.resource_id).unwrap();
    let output = cli(
        adapter,
        &[
            "assignment-resolve-effect",
            id,
            "--workspace",
            "test",
            "--operation-id",
            "t25-effect",
            "--execution-task",
            effect["task_id"].as_str().unwrap(),
            "--conversation",
            effect["conversation_id"].as_str().unwrap(),
            "--call",
            effect["call_id"].as_str().unwrap(),
            "--outcome",
            "not-applied",
            "--actor",
            "operator",
            "--evidence",
            "the controlled tool hung before any external effect",
            "--message",
            "T25 restart fault",
            "--confirm-old-agent-cannot-act",
            "--confirm-external-work-safe",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    retire(adapter, id).unwrap();
    let retired = store.agent_assignment_info(id).unwrap();
    assert!(!retired.capacity_reserved());
    retire(adapter, id).unwrap();
    assert_eq!(store.agent_assignment_info(id).unwrap(), retired);
    println!(
        "T25 daemon restart in a long tool -> fence proof -> effect blocks recovery and retirement -> resolution -> retirement: passed"
    );
}
