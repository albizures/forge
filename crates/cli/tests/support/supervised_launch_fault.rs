//! T26: a production durable launch that fails after its claim never strands
//! the Assignment.
//!
//! Each scenario forces one failure point through the launcher's test seam
//! (`Launcher::inject_fault`, which can only make a launch fail). Then:
//! - CLI/RPC status names the specific failure;
//! - `assignment-retire` reconciles with the matching proof and releases
//!   capacity, with exact retries;
//! - `assignment-purge-runtime` removes the runtime directory.
//!
//! Proof by failure point:
//! - before the launch intent (runtime input, launch intent): the store alone;
//! - spawn: a directory sealed as never owned;
//! - after spawn but before any grant (`owned`, bind, and a daemon killed
//!   while its host waits): the host's own ungranted quiescence receipt;
//! - after a failed first grant in the same daemon: the original Child and
//!   pipe go to retirement and the host writes its exact retirement receipt.
//!
//! `launch-fault-restart-*` modes run the launch in a separate daemon process
//! (this test binary with `--daemon`) and SIGKILL it at the failure point.
//! This process's composition never launched the Assignment.
use super::{
    AgentDispatchRepository, Command, Path, RecordRepository, SqliteProjectRepository,
    SupervisedDispatch, Value, fs, supervised_dispatch, supervised_restart,
};
use forge_app::durable_execution::{Admission, Launch, Repository as _};
use forge_server::durable_authorization::launch::Fault;
use std::{
    io::{BufRead, BufReader},
    process::Stdio,
};

/// The forced failure point and its expected specific status code.
pub fn fault(mode: &str) -> (Fault, &'static str) {
    let point = mode
        .trim_start_matches("launch-fault-")
        .trim_start_matches("restart-");
    match point {
        "runtime-input" => (Fault::RuntimeInput, "durable-launch-runtime-input-failed"),
        "launch-intent" => (Fault::LaunchIntent, "durable-launch-intent-refused"),
        "spawn" => (Fault::Spawn, "durable-launch-spawn-failed"),
        "owned" => (Fault::Owned, "durable-launch-owned-invalid"),
        "bind" => (Fault::Bind, "durable-launch-bind-failed"),
        "grant" => (Fault::Grant, "durable-launch-grant-failed"),
        "stall" => (Fault::Stall, ""),
        other => panic!("unknown launch fault {other}"),
    }
}

/// Inject this mode's fault into a composition about to launch.
pub fn inject(adapter: &SupervisedDispatch) {
    if adapter.mode.starts_with("launch-fault-") {
        adapter
            .production
            .launcher
            .as_ref()
            .unwrap()
            .inject_fault(fault(&adapter.mode).0);
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

/// Any process-table entry, including an unreaped zombie, counts as present.
fn alive(pid: u64) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// The failed launch's single Assignment and its reserved runtime.
fn failed_launch(store: &SqliteProjectRepository, record: u64) -> (String, Launch) {
    let id = supervised_dispatch::wait(|| {
        store
            .list_agent_assignments(record)
            .unwrap()
            .first()
            .map(|a| a.id.clone())
    });
    let launch =
        supervised_dispatch::wait(|| store.runtime("test", &id).unwrap().map(|r| r.launch));
    (id, launch)
}

/// `launch-fault-<point>`: same daemon lifetime.
pub fn run(adapter: &SupervisedDispatch, store: &SqliteProjectRepository, record: u64) {
    let (_, code) = fault(&adapter.mode);
    inject(adapter);
    // A failed provision is retained by the application, never relaunched.
    let _ = adapter.app().dispatch_record(record);
    let (id, launch) = failed_launch(store, record);
    let status_view = supervised_dispatch::wait(|| {
        let view = status(adapter, &id);
        view["retirement_blockers"]
            .as_array()
            .unwrap()
            .contains(&Value::from(code))
            .then_some(view)
    });
    assert!(
        status_view["uncertainty"]
            .as_array()
            .unwrap()
            .contains(&Value::from("launch-failed")),
        "{status_view}"
    );
    assert_eq!(
        store.runtime("test", &id).unwrap().unwrap().admission,
        Admission::Pending
    );
    assert!(
        store
            .agent_assignment_info(&id)
            .unwrap()
            .capacity_reserved()
    );
    // A second provision of the claimed Assignment never respawns.
    let _ = adapter.app().dispatch_record(record);
    assert_eq!(store.list_agent_assignments(record).unwrap().len(), 1);
    let directory = SupervisedDispatch::directory(store, &launch);
    let point = adapter.mode.trim_start_matches("launch-fault-");
    if matches!(point, "owned" | "bind") {
        // The driver closed the private pipe: the host fenced before any grant,
        // proved quiescence, wrote its receipt and exited (reaped, no zombie).
        let host = supervised_dispatch::owner(&directory)["owner"]["host_pid"]
            .as_u64()
            .unwrap();
        ungranted_receipt(&directory);
        supervised_dispatch::wait(|| (!alive(host)).then_some(()));
    }
    retire_and_purge(adapter, store, &id, record, &directory, point);
}

fn ungranted_receipt(directory: &Path) -> String {
    supervised_dispatch::wait(|| {
        names(directory)
            .into_iter()
            .find(|name| name.starts_with("ungranted-receipt-"))
    })
}

/// `launch-fault-restart-<point>`: a separate daemon process launches, fails or
/// stalls at the point, and is killed there.
pub fn run_restart(adapter: &SupervisedDispatch, store: &SqliteProjectRepository, record: u64) {
    let (_, code) = fault(&adapter.mode);
    let config = supervised_restart::write_config(adapter);
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
    let (id, launch) = failed_launch(store, record);
    assert_eq!(line.trim(), id);
    let directory = SupervisedDispatch::directory(store, &launch);
    let point = adapter.mode.trim_start_matches("launch-fault-restart-");
    let host = if code.is_empty() {
        // Stalled after `owned`, before any grant.
        let owner = supervised_dispatch::wait(|| {
            directory
                .join("owner.json")
                .exists()
                .then(|| supervised_dispatch::owner(&directory))
        });
        Some(owner["owner"]["host_pid"].as_u64().unwrap())
    } else {
        supervised_dispatch::wait(|| {
            store
                .agent_assignment_info(&id)
                .unwrap()
                .resources
                .iter()
                .any(|r| r.resource_kind == "durable-launch-failure" && r.resource_id == code)
                .then_some(())
        });
        (point == "grant").then(|| {
            supervised_dispatch::owner(&directory)["owner"]["host_pid"]
                .as_u64()
                .unwrap()
        })
    };
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    assert!(
        adapter.state(&id).is_none(),
        "this composition never launched it"
    );
    assert!(
        store
            .agent_assignment_info(&id)
            .unwrap()
            .capacity_reserved()
    );
    if let Some(host) = host {
        // The orphaned host lost its only grant channel before any grant: it
        // fences, proves quiescence as subreaper, writes its receipt and exits.
        ungranted_receipt(&directory);
        supervised_dispatch::wait(|| (!alive(host)).then_some(()));
    }
    retire_and_purge(adapter, store, &id, record, &directory, point);
}

fn retire_and_purge(
    adapter: &SupervisedDispatch,
    store: &SqliteProjectRepository,
    id: &str,
    record: u64,
    directory: &Path,
    point: &str,
) {
    retire(adapter, id).unwrap();
    let retired = store.agent_assignment_info(id).unwrap();
    assert!(retired.assignment.closed_at_millis.is_some());
    assert!(!retired.capacity_reserved(), "{retired:?}");
    assert!(
        retired
            .resources
            .iter()
            .all(|r| r.retired_at_millis.is_some())
    );
    // Exact retry: the original result, timestamps unchanged.
    retire(adapter, id).unwrap();
    assert_eq!(store.agent_assignment_info(id).unwrap(), retired);
    let lifecycle = names(directory);
    let expected = match point {
        "runtime-input" | "launch-intent" | "spawn" => "launch-sealed.json",
        "grant" if !adapter.mode.contains("restart") => "retirement-receipt.json",
        _ => "ungranted-receipt-",
    };
    assert!(
        lifecycle.iter().any(|name| name.starts_with(expected)),
        "{expected}: {lifecycle:?}"
    );
    // Only test-fixture files production never writes are removed by the test;
    // every lifecycle artifact stays for the host purge to recognize.
    for name in &lifecycle {
        if name.starts_with("controlled") {
            fs::remove_file(directory.join(name)).unwrap();
        }
    }
    let resume = cli(adapter, &["resume", &record.to_string()]);
    assert!(
        resume.status.success(),
        "{}",
        String::from_utf8_lossy(&resume.stderr)
    );
    let output = cli(
        adapter,
        &[
            "assignment-purge-runtime",
            id,
            "--workspace",
            "test",
            "--operation-id",
            "t26-purge",
        ],
    );
    assert!(
        output.status.success(),
        "{}: {:?}",
        String::from_utf8_lossy(&output.stderr),
        names(directory)
    );
    let purged: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(purged["state"], "purged");
    assert!(!directory.exists());
    assert!(store.show_record(record).is_ok());
    println!(
        "T26 {} -> specific diagnostic -> retirement ({expected}) -> purge: passed",
        adapter.mode
    );
}
