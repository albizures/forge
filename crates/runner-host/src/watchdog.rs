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
#[cfg(test)]
#[path = "recovery_receiver_tests.rs"]
mod recovery_receiver_tests;

pub struct State {
    policy: LeaseWatchdog,
    fault: Option<&'static str>,
    retained_blocker: bool,
    shutdown: Option<Shutdown>,
    shutdown_denied: bool,
    owner: Owner,
    recovery: Option<forge_rpc::durable_execution::SupervisorRecoveryPreparation>,
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
            recovery: None,
        }
    }

    fn prepare_recovery(
        &mut self,
        control: forge_rpc::durable_execution::SupervisorRecoveryPreparation,
        now: u64,
    ) -> io::Result<()> {
        let clock_valid = self.policy.check(now).is_ok();
        let valid = clock_valid
            && control.host_prepare_recovery
                == forge_rpc::durable_execution::SUPERVISOR_RECOVERY_PREPARATION_VERSION
            && self.retained_blocker
            && self.fault.is_none()
            && self.shutdown.is_none()
            && self.policy.last_lease().is_none()
            && control.grant.lease.owner == self.owner
            && control
                .grant
                .validate_candidate(&control.candidate, now)
                .is_ok();
        if !valid || self.recovery.as_ref().is_some_and(|old| old != &control) {
            self.fence("runner-recovery-preparation-denied");
            return Err(io::Error::other("recovery preparation denied"));
        }
        // Exact preparation retries do not replace policy, clock observations or
        // deadlines. Preparation after any first grant is forbidden above.
        if self.recovery.is_none() {
            let mut policy = LeaseWatchdog::for_recovery(self.owner.clone(), control.grant.clone())
                .map_err(|_| io::Error::other("recovery preparation denied"))?;
            policy
                .check(now)
                .map_err(|_| io::Error::other("recovery preparation denied"))?;
            self.policy = policy;
            self.recovery = Some(control);
        }
        Ok(())
    }

    fn accept_lease(&mut self, control: SupervisorControl, now: u64) -> io::Result<()> {
        if control.host_control != SUPERVISOR_CONTROL_VERSION
            || self.fault.is_some()
            || (self.retained_blocker && self.recovery.is_none())
            || self.policy.accept(control.lease, now).is_err()
        {
            self.fence("runner-authorization-denied");
            return Err(io::Error::other("authorization denied"));
        }
        Ok(())
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
pub fn forward_input(mut input: ChildStdin, state: &Mutex<State>, directory: &Path) {
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
                // absolute deadline even if a model/tool ignores EOF. Before
                // any accepted grant, the private pipe was the only channel a
                // grant could ever arrive on: the issuer is gone, so fence now
                // and let the original subreaper prove quiescence (T26).
                if let Ok(mut state) = state.lock()
                    && state.shutdown.is_none()
                    && state.policy.last_lease().is_none()
                {
                    state.fence("runner-issuer-lost");
                }
                return Ok(());
            }
            if count > 1_048_576 || line.last() != Some(&b'\n') {
                return Err(io::Error::other("invalid bounded host input"));
            }
            let value = serde_json::from_slice::<serde_json::Value>(&line).ok();
            if forward_control(&line, value.as_ref(), state, directory)? {
                input.write_all(&line)?;
            }
        }
    })();
    if result.is_err() {
        eprintln!("runner-host-input-failed; ownership retained");
        if let Ok(mut state) = state.lock() {
            state.fence("runner-input-uncertain");
        }
    }
}

// Return false for host-only shutdown; never forward cancellation on descriptive
// evidence. All guards are dropped before writing to a possibly stalled worker.
fn forward_control(
    line: &[u8],
    value: Option<&serde_json::Value>,
    state: &Mutex<State>,
    directory: &Path,
) -> io::Result<bool> {
    use forge_rpc::durable_execution::{
        SUPERVISOR_QUIESCENCE_VERSION, SUPERVISOR_RECOVERY_VERSION, SUPERVISOR_RETIREMENT_VERSION,
        SupervisorQuiescence, SupervisorRecovery, SupervisorRecoveryPreparation,
        SupervisorRetirement,
    };
    let has = |key| value.is_some_and(|v| v.get(key).is_some());
    let invalid = || io::Error::other("invalid private supervisor control");
    let deny = |code| {
        if let Ok(mut state) = state.lock() {
            state.fence(code);
        }
        invalid()
    };
    let shutdown = if has("host_retire") {
        let control: SupervisorRetirement = serde_json::from_slice(line).map_err(|_| invalid())?;
        if control.host_retire != SUPERVISOR_RETIREMENT_VERSION {
            return Err(deny("runner-retirement-conflict"));
        }
        Some(Shutdown::Retirement(Box::new(control.operation)))
    } else if has("host_quiesce") {
        let control: SupervisorQuiescence = serde_json::from_slice(line).map_err(|_| invalid())?;
        if control.host_quiesce != SUPERVISOR_QUIESCENCE_VERSION {
            return Err(deny("runner-quiescence-conflict"));
        }
        Some(Shutdown::Quiescence(Box::new(control.operation)))
    } else {
        None
    };
    if let Some(shutdown) = shutdown {
        state
            .lock()
            .map_err(|_| invalid())?
            .request_shutdown(shutdown)?;
        return Ok(false);
    }
    if has("host_prepare_recovery") {
        let control: SupervisorRecoveryPreparation =
            serde_json::from_slice(line).map_err(|_| invalid())?;
        let operation =
            forge_app::durable_execution::recovery::quiescence::Operation::for_candidate(
                &control.candidate,
                control.grant.claim.clone(),
            )
            .map_err(|_| invalid())?;
        // Filesystem/process I/O never holds the watchdog policy mutex.
        crate::quiescence::waiting_receipt(directory, &operation)?;
        let mut state = state.lock().map_err(|_| invalid())?;
        state.prepare_recovery(control, boottime_millis()?)?;
    } else if has("host_recovery") {
        let control: SupervisorRecovery = serde_json::from_slice(line).map_err(|_| invalid())?;
        let operation = {
            let state = state.lock().map_err(|_| invalid())?;
            let expected = state.recovery.as_ref().ok_or_else(invalid)?;
            forge_app::durable_execution::recovery::quiescence::Operation::for_candidate(
                &expected.candidate,
                expected.grant.claim.clone(),
            )
            .map_err(|_| invalid())?
        };
        crate::quiescence::waiting_receipt(directory, &operation)?;
        let mut state = state.lock().map_err(|_| invalid())?;
        if control.host_recovery != SUPERVISOR_RECOVERY_VERSION
            || state.fault.is_some()
            || state.recovery.is_none()
            || state
                .policy
                .accept_recovery(&control.grant, boottime_millis()?)
                .is_err()
        {
            state.fence("runner-recovery-authorization-denied");
            return Err(invalid());
        }
    } else if has("host_control") {
        let control: SupervisorControl =
            serde_json::from_slice(line).map_err(|_| deny("runner-authorization-denied"))?;
        let mut state = state.lock().map_err(|_| invalid())?;
        // Read inside the policy critical section to preserve clock ordering.
        state.accept_lease(control, boottime_millis()?)?;
    }
    Ok(true)
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

/// Returns the marker name actually written. A first fence writes
/// `watchdog.json`; a later episode in a recovered runtime directory, whose
/// earlier marker is retained history, writes its own episode marker.
fn retain(
    directory: &Path,
    identity: (u64, u64),
    owner: &Owner,
    code: &str,
    lease: Option<&Lease>,
    marker: bool,
) -> Option<String> {
    let mut names = if marker {
        vec![
            crate::fence::MARKER.to_owned(),
            crate::fence::episode_marker(owner),
        ]
    } else {
        vec![format!(
            "watchdog-{}-{}-{code}.json",
            owner.host_pid, owner.host_start
        )]
    };
    if marker
        && fs::symlink_metadata(directory.join(crate::fence::MARKER))
            .is_ok_and(|meta| meta.is_file())
    {
        names.remove(0);
    } else {
        names.truncate(1);
    }
    let name = names.remove(0);
    if diagnostic(directory, &name, owner, code, lease, identity).is_err() {
        // Failure does not prevent cancellation or release the held flock. No
        // paths, payloads, credentials or untrusted I/O error strings are logged.
        eprintln!("runner-watchdog-diagnostic-unavailable; ownership retained");
        return None;
    }
    Some(name)
}

// A direct Child wait and original subreaper proof, never a /proc absence guess.
fn reaped_crash(
    worker: &mut Supervised<'_>,
    directory: &Path,
    state: &Mutex<State>,
) -> io::Result<bool> {
    if worker.try_wait()?.is_none() {
        return Ok(false);
    }
    let lease = state
        .lock()
        .ok()
        .and_then(|state| {
            (state.fault.is_none()
                && (!state.retained_blocker || state.recovery.is_some())
                && state.shutdown.is_none())
            .then(|| state.policy.last_lease().cloned())
            .flatten()
        })
        .ok_or_else(|| io::Error::other("unverified crash authority"))?;
    crate::crash::record(worker, directory, &lease)?;
    eprintln!("runner-crash-verified");
    Ok(true)
}

fn sample_worker(
    worker: &mut Supervised<'_>,
    directory: &Path,
    state: &Mutex<State>,
) -> io::Result<bool> {
    if reaped_crash(worker, directory, state)? {
        return Ok(true);
    }
    if worker.verify_or_exited()? {
        return reaped_crash(worker, directory, state);
    }
    Ok(false)
}

/// Post-fence state: the signals sent, and quiescence proof progress.
struct Fence {
    started: Instant,
    boot_started: Option<u64>,
    lease: Option<Lease>,
    marker: Option<String>,
    kill_attempted: bool,
    killed_at: Option<Instant>,
    unproven_recorded: bool,
    proof_failed: bool,
}

pub fn supervise(mut worker: Supervised<'_>, directory: &Path, state: &Mutex<State>) {
    let owner = worker.owner().clone();
    let identity = worker.directory_identity();
    let record = |code: &str, lease: Option<&Lease>, marker: bool| {
        retain(directory, identity, &owner, code, lease, marker)
    };
    let mut fence: Option<Fence> = None;
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
        let observation = if exited {
            Ok(())
        } else {
            match sample_worker(&mut worker, directory, state) {
                Ok(true) => return,
                Ok(false) => Ok(()),
                Err(error) => {
                    exited = true;
                    eprintln!(
                        "runner-worker-exited-or-unverified; ownership retained pending reconciliation"
                    );
                    Err(error)
                }
            }
        };
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
                if observation.is_err() {
                    // An unarmed waiting owner can lose verified identity too.
                    // Remember that fault before any first/recovery grant; later
                    // file restoration must not revive a fenced Supervised handle.
                    state.fence("runner-owner-uncertain");
                }
                (state.fault, state.policy.last_lease().cloned(), now)
            }
            Err(_) => (Some("runner-watchdog-state-unavailable"), None, None),
        };
        if let Some(code) = fault
            && fence.is_none()
        {
            // Retain the safety marker before initiating termination. Its
            // mere existence blocks new grants after restart; no adoption.
            let marker = record(code, lease.as_ref(), true);
            eprintln!("{code}; ownership retained pending reconciliation");
            let mut proof_failed = false;
            match worker.signal_or_exited(rustix::process::Signal::TERM) {
                Ok(true) => {
                    record("term-sent", lease.as_ref(), false);
                }
                Ok(false) => {}
                Err(_) => {
                    proof_failed = true;
                    record("term-unverified", lease.as_ref(), false);
                }
            }
            fence = Some(Fence {
                started: Instant::now(),
                boot_started: now,
                lease: lease.clone(),
                marker,
                kill_attempted: false,
                killed_at: None,
                unproven_recorded: false,
                proof_failed,
            });
        }
        if let Some(fence) = &mut fence
            && quiesce_after_fence(&mut worker, directory, &owner, fence, now, &record)
        {
            return;
        }
        // Without positive post-fence proof, retain ownership indefinitely.
        // Neither process exit nor successful signals establish descendant
        // absence or authorize replacement.
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// One post-fence step: escalate to KILL after the grace period, then try to
/// prove descendant absence. Returns true once the exact receipt is persisted;
/// the caller then exits, releasing the shared lock with its last holder.
fn quiesce_after_fence(
    worker: &mut Supervised<'_>,
    directory: &Path,
    owner: &Owner,
    fence: &mut Fence,
    now: Option<u64>,
    record: &impl Fn(&str, Option<&Lease>, bool) -> Option<String>,
) -> bool {
    let grace_elapsed = match (fence.boot_started, now) {
        (Some(start), Some(now)) if now >= start => now - start >= TERM_GRACE_MILLIS,
        _ => fence.started.elapsed() >= Duration::from_millis(TERM_GRACE_MILLIS),
    };
    if !fence.kill_attempted && grace_elapsed {
        fence.kill_attempted = true;
        fence.killed_at = Some(Instant::now());
        let code = match worker.signal_or_exited(rustix::process::Signal::KILL) {
            Ok(true) => Some("kill-sent"),
            Ok(false) => None,
            Err(_) => {
                fence.proof_failed = true;
                Some("kill-unverified")
            }
        };
        if let Some(code) = code {
            record(code, fence.lease.as_ref(), false);
            eprintln!("runner-watchdog-{code}; cleanup unverified");
        }
    }
    if fence.proof_failed {
        // An unverified signal or identity: never proof. Say so once, after
        // the KILL attempt, so the retained hold has a specific diagnostic.
        if fence.kill_attempted && !fence.unproven_recorded {
            fence.unproven_recorded = true;
            record("quiescence-unproven", fence.lease.as_ref(), false);
            eprintln!("runner-watchdog-quiescence-unproven; ownership retained");
        }
        return false;
    }
    // Proof needs this episode's own marker and the last accepted lease to bind.
    // A fence before any accepted grant (no lease) binds the owner instead (T26).
    let Some(marker) = &fence.marker else {
        fence.proof_failed = true;
        return false;
    };
    match worker.fenced_quiescence() {
        Ok(true) => {
            let recorded = match &fence.lease {
                Some(lease) => crate::fence::record(directory, marker, lease),
                None => crate::fence::record_ungranted(directory, marker, owner),
            };
            if recorded.is_ok() {
                eprintln!("runner-watchdog-quiescence-verified");
                return true;
            }
            fence.proof_failed = true;
        }
        Ok(false) => {
            let overdue = fence
                .killed_at
                .is_some_and(|killed| killed.elapsed() >= Duration::from_millis(2_000));
            if overdue && !fence.unproven_recorded {
                fence.unproven_recorded = true;
                // A surviving worker or escaped descendant: keep the hold.
                // Proof may still follow if it exits; nothing is signalled.
                record("quiescence-unproven", fence.lease.as_ref(), false);
                eprintln!("runner-watchdog-quiescence-unproven; ownership retained");
            }
            return false;
        }
        Err(_) => fence.proof_failed = true,
    }
    false
}
