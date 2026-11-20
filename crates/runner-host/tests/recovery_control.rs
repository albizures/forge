//! Actual private `ChildStdin` -> Node consumer transport; fixed synthetic host
//! expectation/clock, not process cleanup proof or writable runtime recovery.
use forge_app::{
    authorization_lease::session::Delivery as _,
    durable_execution::recovery::session::Delivery as _,
};
use forge_rpc::durable_execution::SupervisorRecovery;
use forge_runner_host::control::ControlPipe;
use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn spawn(ordinary: bool) -> (Worker, ControlPipe, Receiver<serde_json::Value>) {
    let probe = Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .unwrap();
    assert!(probe.status.success());
    let executable = String::from_utf8(probe.stdout).unwrap();
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runner/test/recovery-control-worker.mjs");
    let mut command = Command::new(executable.trim());
    command
        .arg(script)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if ordinary {
        command.arg("ordinary");
    }
    let mut worker = Worker(command.spawn().unwrap());
    let pipe = ControlPipe::take(&mut worker.0).unwrap();
    let output = worker.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else {
                break;
            };
            if tx.send(serde_json::from_str(&line).unwrap()).is_err() {
                break;
            }
        }
    });
    assert_eq!(event(&rx)["event"], "waiting");
    (worker, pipe, rx)
}
fn event(rx: &Receiver<serde_json::Value>) -> serde_json::Value {
    rx.recv_timeout(Duration::from_secs(20))
        .expect("synthetic consumer response")
}
fn control() -> SupervisorRecovery {
    serde_json::from_str(include_str!("../../../runner/test/recovery-control.json")).unwrap()
}

#[test]
fn recovery_pipe_delivers_exact_envelope_then_sequential_renewal_from_a_clone() {
    let (_worker, mut pipe, rx) = spawn(false);
    let mut clone = pipe.clone();
    let grant = control().grant;
    pipe.deliver_recovery(&grant).unwrap();
    assert_eq!(
        event(&rx),
        serde_json::json!({"event": "accepted", "sequence": 7})
    );
    let mut renewal = grant.lease;
    renewal.sequence += 1;
    renewal.issued_boottime_millis += 10_000;
    renewal.expires_boottime_millis += 10_000;
    clone.deliver(&renewal).unwrap();
    assert_eq!(
        event(&rx),
        serde_json::json!({"event": "accepted", "sequence": 8})
    );
    // Successful write is not acceptance: an old exact recovery retry after
    // renewal is refused, and even correct later deliveries cannot revive it.
    pipe.deliver_recovery(&control().grant).unwrap();
    assert_eq!(event(&rx)["event"], "denied");
    clone.deliver(&renewal).unwrap();
    assert_eq!(event(&rx)["event"], "denied");
}

#[test]
fn recovery_pipe_does_not_prepare_an_ordinary_restarted_consumer() {
    let (_worker, mut pipe, rx) = spawn(true);
    pipe.deliver_recovery(&control().grant).unwrap();
    assert_eq!(event(&rx)["event"], "denied");
}

#[test]
fn wrong_envelope_terminally_denies_even_when_transport_reports_success() {
    let (_worker, mut pipe, rx) = spawn(false);
    let mut grant = control().grant;
    grant.evidence_sha256 = "c".repeat(64);
    pipe.deliver_recovery(&grant).unwrap();
    assert_eq!(event(&rx)["event"], "denied");
    pipe.deliver_recovery(&control().grant).unwrap();
    assert_eq!(event(&rx)["event"], "denied");
}

#[test]
fn oversized_recovery_fences_the_shared_real_pipe_before_any_bytes_are_sent() {
    let (_worker, mut pipe, _rx) = spawn(false);
    let mut clone = pipe.clone();
    let mut grant = control().grant;
    grant.evidence_sha256 = "x".repeat(1_048_576);
    assert_eq!(
        pipe.deliver_recovery(&grant).unwrap_err().code,
        "authorization-delivery-uncertain"
    );
    assert!(clone.deliver_recovery(&control().grant).is_err());
    assert!(clone.deliver(&control().grant.lease).is_err());
}
