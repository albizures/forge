//! Trusted pipe delivery and an independent (non-JavaScript) watchdog loop.
use crate::supervision::{Supervised, boottime_millis};
use forge_app::authorization_lease::{Lease, LeaseWatchdog, Owner, TERM_GRACE_MILLIS};
use forge_rpc::durable_execution::{SUPERVISOR_CONTROL_VERSION, SupervisorControl};
use std::{
    fs,
    io::{self, BufRead, Read, Write},
    os::unix::fs::MetadataExt,
    path::Path,
    process::ChildStdin,
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Clone, PartialEq)]
enum Shutdown {
    Retirement(Box<forge_app::durable_execution::retirement::Operation>),
    Quiescence(Box<forge_app::durable_execution::recovery::quiescence::Operation>),
}

impl Shutdown {
    fn kind(&self) -> &'static str {
        match self {
            Self::Retirement(_) => "retirement",
            Self::Quiescence(_) => "quiescence",
        }
    }

    fn stop(&self, worker: &mut Supervised<'_>, directory: &Path) -> io::Result<()> {
        match self {
            Self::Retirement(operation) => crate::retirement::retire(worker, directory, operation),
            Self::Quiescence(operation) => crate::quiescence::quiesce(worker, directory, operation),
        }
    }
}

#[cfg(test)]
#[path = "quiescence_tests.rs"]
mod quiescence_tests;

pub struct State {
    policy: LeaseWatchdog,
    fault: Option<&'static str>,
    retained_blocker: bool,
    shutdown: Option<Shutdown>,
    shutdown_denied: bool,
    owner: Owner,
}

impl State {
    pub fn new(owner: Owner, retained_blocker: bool) -> Self {
        Self {
            policy: LeaseWatchdog::new(owner.clone()),
            fault: None,
            retained_blocker,
            shutdown: None,
            shutdown_denied: false,
            owner,
        }
    }

    fn request_shutdown(&mut self, operation: Shutdown) -> io::Result<()> {
        let valid = match &operation {
            Shutdown::Retirement(op) => {
                op.validate().is_ok()
                    && op.owner == self.owner
                    && self
                        .policy
                        .last_lease()
                        .is_none_or(|lease| lease.launch == op.launch)
            }
            Shutdown::Quiescence(op) => {
                op.validate().is_ok()
                    && !self.retained_blocker
                    && self.fault.is_none()
                    && op.predecessor.owner == self.owner
                    && self.policy.last_lease() == Some(&op.predecessor)
            }
        };
        self.shutdown_denied |=
            !valid || self.shutdown.as_ref().is_some_and(|old| *old != operation);
        self.shutdown.get_or_insert(operation);
        self.policy.fence();
        if self.shutdown_denied {
            Err(io::Error::other("shutdown denied"))
        } else {
            Ok(())
        }
    }

    fn fence(&mut self, code: &'static str) {
        self.policy.fence();
        self.fault.get_or_insert(code);
    }
}

// A stalled worker may block this thread's pipe write, but it cannot block the
// watchdog: never hold the policy mutex during reads, writes or process I/O.
pub fn forward_input(mut input: ChildStdin, state: &Mutex<State>) {
    let result = (|| -> io::Result<()> {
        let mut source = io::stdin().lock();
        loop {
            let mut line = Vec::new();
            let count = source
                .by_ref()
                .take(1_048_577)
                .read_until(b'\n', &mut line)?;
            if count == 0 {
                // Pipe loss cannot renew. Existing authority expires at its
                // absolute deadline even if a model/tool ignores EOF.
                return Ok(());
            }
            if count > 1_048_576 || line.last() != Some(&b'\n') {
                return Err(io::Error::other("invalid bounded host input"));
            }
            let value = serde_json::from_slice::<serde_json::Value>(&line).ok();
            if value
                .as_ref()
                .is_some_and(|v| v.get("host_retire").is_some())
            {
                let control: forge_rpc::durable_execution::SupervisorRetirement =
                    serde_json::from_slice(&line)
                        .map_err(|_| io::Error::other("invalid retirement control"))?;
                let mut state = state
                    .lock()
                    .map_err(|_| io::Error::other("watchdog state unavailable"))?;
                if control.host_retire
                    != forge_rpc::durable_execution::SUPERVISOR_RETIREMENT_VERSION
                {
                    state.fence("runner-retirement-conflict");
                    return Err(io::Error::other("invalid retirement control"));
                }
                state.request_shutdown(Shutdown::Retirement(Box::new(control.operation)))?;
                drop(state);
                // The watchdog verifies ownership and persists host intent
                // before signalling TERM, whose runner handler aborts authority.
                // No cancellation is forwarded on descriptive evidence alone.
                continue;
            }
            if value
                .as_ref()
                .is_some_and(|v| v.get("host_quiesce").is_some())
            {
                let control: forge_rpc::durable_execution::SupervisorQuiescence =
                    serde_json::from_slice(&line)
                        .map_err(|_| io::Error::other("invalid recovery shutdown control"))?;
                let mut state = state
                    .lock()
                    .map_err(|_| io::Error::other("watchdog state unavailable"))?;
                if control.host_quiesce
                    != forge_rpc::durable_execution::SUPERVISOR_QUIESCENCE_VERSION
                {
                    state.fence("runner-quiescence-conflict");
                    return Err(io::Error::other("invalid recovery shutdown control"));
                }
                state.request_shutdown(Shutdown::Quiescence(Box::new(control.operation)))?;
                // Cancellation goes through the original subreaper only, never
                // through a worker message or a retained PID supplied by a client.
                continue;
            }
            if value
                .as_ref()
                .is_some_and(|v| v.get("host_control").is_some())
            {
                let control = serde_json::from_slice::<SupervisorControl>(&line);
                let mut state = state
                    .lock()
                    .map_err(|_| io::Error::other("watchdog state unavailable"))?;
                // Read inside the short policy critical section so a concurrent
                // poll cannot make this observation appear to move backwards.
                let accepted = match (control, boottime_millis()) {
                    (Ok(control), Ok(now))
                        if control.host_control == SUPERVISOR_CONTROL_VERSION
                            && !state.retained_blocker =>
                    {
                        state.policy.accept(control.lease, now).is_ok()
                    }
                    _ => false,
                };
                if !accepted {
                    state.fence("runner-authorization-denied");
                    return Err(io::Error::other("authorization denied"));
                }
                // Release the guard before writing to a possibly stalled worker.
                drop(state);
            }
            input.write_all(&line)?;
        }
    })();
    if result.is_err() {
        eprintln!("runner-host-input-failed; ownership retained");
        if let Ok(mut state) = state.lock() {
            state.fence("runner-input-uncertain");
        }
    }
}

fn diagnostic(
    directory: &Path,
    name: &str,
    owner: &Owner,
    code: &str,
    lease: Option<&Lease>,
    identity: (u64, u64),
) -> io::Result<()> {
    let meta = fs::symlink_metadata(directory)?;
    if !meta.is_dir()
        || meta.uid() != rustix::process::getuid().as_raw()
        || meta.mode() & 0o7777 != 0o700
        || (meta.dev(), meta.ino()) != identity
        || fs::canonicalize(directory)? != directory
    {
        return Err(io::Error::other("unverified diagnostic directory"));
    }
    let fd = rustix::fs::open(
        directory.join(name),
        rustix::fs::OFlags::CREATE
            | rustix::fs::OFlags::EXCL
            | rustix::fs::OFlags::WRONLY
            | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )?;
    let mut file = fs::File::from(fd);
    let value = serde_json::json!({ "version": 1, "code": code, "owner": owner, "last_lease": lease, "cleanup": "unverified" });
    writeln!(file, "{value}")?;
    file.sync_all()?;
    fs::File::open(directory)?.sync_all()
}

fn retain(
    directory: &Path,
    identity: (u64, u64),
    owner: &Owner,
    code: &str,
    lease: Option<&Lease>,
    marker: bool,
) {
    let name = if marker {
        "watchdog.json".into()
    } else {
        format!(
            "watchdog-{}-{}-{code}.json",
            owner.host_pid, owner.host_start
        )
    };
    if diagnostic(directory, &name, owner, code, lease, identity).is_err() {
        // Failure does not prevent cancellation or release the held flock. No
        // paths, payloads, credentials or untrusted I/O error strings are logged.
        eprintln!("runner-watchdog-diagnostic-unavailable; ownership retained");
    }
}

pub fn supervise(mut worker: Supervised<'_>, directory: &Path, state: &Mutex<State>) {
    let owner = worker.owner().clone();
    let identity = worker.directory_identity();
    let record = |code: &str, lease: Option<&Lease>, marker: bool| {
        retain(directory, identity, &owner, code, lease, marker);
    };
    let mut alarm: Option<(Instant, Option<u64>)> = None;
    let mut kill_attempted = false;
    let mut exited = false;
    let mut shutdown_attempted = false;
    loop {
        let shutdown = state.lock().ok().and_then(|state| {
            state
                .shutdown
                .clone()
                .map(|op| (op, !state.shutdown_denied))
        });
        if !shutdown_attempted && let Some((operation, valid)) = shutdown {
            shutdown_attempted = true;
            let kind = operation.kind();
            if valid && operation.stop(&mut worker, directory).is_ok() {
                eprintln!("runner-{kind}-verified");
                return;
            }
            record(&format!("{kind}-unverified"), None, true);
            eprintln!("runner-{kind}-unverified; ownership retained");
            // Incomplete shutdown is terminal for this supervisor. No new
            // grant or blind signal retry may turn uncertainty into permission.
            exited = true;
        }
        if shutdown_attempted {
            // No fallback watchdog signals after a failed identity-bound
            // shutdown, including intent/receipt storage or ownership failure.
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        let observation = if exited { Ok(()) } else { worker.verify() };
        let (fault, lease, now) = match state.lock() {
            Ok(mut state) => {
                // Input may have requested shutdown after the loop's first
                // snapshot. Do not mistake its deliberate policy fence for an
                // expiry and issue watchdog signals ahead of protected intent.
                if state.shutdown.is_some() {
                    continue;
                }
                let now = boottime_millis().ok();
                if let Some(now) = now {
                    if state.policy.check(now).is_err() {
                        state.fence("runner-lease-expired");
                    }
                } else {
                    state.fence("runner-clock-unavailable");
                }
                if observation.is_err() && state.policy.last_lease().is_some() {
                    state.fence("runner-owner-uncertain");
                }
                (state.fault, state.policy.last_lease().cloned(), now)
            }
            Err(_) => (Some("runner-watchdog-state-unavailable"), None, None),
        };
        if let Some(code) = fault
            && alarm.is_none()
        {
            alarm = Some((Instant::now(), now));
            // Retain the safety marker before initiating termination. Its
            // mere existence blocks new grants after restart; no adoption.
            record(code, lease.as_ref(), true);
            eprintln!("{code}; ownership retained pending reconciliation");
            let code = if worker.signal(rustix::process::Signal::TERM).is_ok() {
                "term-sent"
            } else {
                "term-unverified"
            };
            record(code, lease.as_ref(), false);
        }
        if let Some((started, boot_started)) = alarm {
            let grace_elapsed = match (boot_started, now) {
                (Some(start), Some(now)) if now >= start => now - start >= TERM_GRACE_MILLIS,
                _ => started.elapsed() >= Duration::from_millis(TERM_GRACE_MILLIS),
            };
            if !kill_attempted && grace_elapsed {
                kill_attempted = true;
                let code = if worker.signal(rustix::process::Signal::KILL).is_ok() {
                    "kill-sent"
                } else {
                    "kill-unverified"
                };
                record(code, lease.as_ref(), false);
                eprintln!("runner-watchdog-{code}; cleanup unverified");
            }
        }
        if !exited {
            match worker.try_wait() {
                Ok(Some(status)) => {
                    exited = true;
                    eprintln!(
                        "runner-worker-exited: {status}; ownership retained pending reconciliation"
                    );
                }
                Ok(None) => {}
                Err(_) => eprintln!("runner-worker-wait-unavailable; ownership retained"),
            }
        }
        // Retain ownership indefinitely. Neither process exit nor successful
        // signals establish descendant absence or authorize replacement.
        std::thread::sleep(Duration::from_millis(50));
    }
}
