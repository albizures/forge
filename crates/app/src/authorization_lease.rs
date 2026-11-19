//! Forge authority, not liveness, permits bounded execution. No wall-clock grants.
use crate::{RegistrationError, durable_execution::Launch};
use serde::{Deserialize, Serialize};

pub const LEASE_MILLIS: u64 = 30_000;
pub const RENEW_AFTER_MILLIS: u64 = 10_000;
pub const TERM_GRACE_MILLIS: u64 = 5_000;

/// Verified by the host adapter while it holds exclusive ownership. Never adopt
/// a process from these descriptive values alone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub boot_id: String,
    pub time_namespace: String,
    pub host_pid: u32,
    pub host_start: String,
    pub worker_pid: u32,
    pub worker_start: String,
    pub lock_device: String,
    pub lock_inode: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lease {
    pub launch: Launch,
    pub generation: u64,
    pub owner: Owner,
    /// Unique to the granting daemon lifetime; restart cannot renew old grants.
    pub daemon_epoch: String,
    pub sequence: u32,
    pub issued_boottime_millis: u64,
    pub expires_boottime_millis: u64,
}

fn denied() -> RegistrationError {
    RegistrationError::new(
        "assignment-lease-denied",
        "execution requires current bounded Forge authorization",
    )
}

impl Lease {
    /// Check monotonic succession. A late/expired grant never revives execution.
    /// The caller supplies a trusted `CLOCK_BOOTTIME` observation, not agent input.
    /// # Errors
    /// Rejects invalid bounds, changed owners/bindings/epochs and renewal gaps.
    pub fn validate(&self, previous: Option<&Self>, now: u64) -> Result<(), RegistrationError> {
        self.launch.validate()?;
        if self.generation == 0
            || self.daemon_epoch.trim().is_empty()
            || self.owner.host_pid == 0
            || self.owner.worker_pid == 0
            || [
                &self.owner.boot_id,
                &self.owner.time_namespace,
                &self.owner.host_start,
                &self.owner.worker_start,
                &self.owner.lock_device,
                &self.owner.lock_inode,
            ]
            .iter()
            .any(|s| s.trim().is_empty())
            || self.issued_boottime_millis > now
            || now >= self.expires_boottime_millis
            || self
                .expires_boottime_millis
                .checked_sub(self.issued_boottime_millis)
                != Some(LEASE_MILLIS)
        {
            return Err(denied());
        }
        match previous {
            None if self.sequence == 1 => Ok(()),
            Some(old)
                if old.launch == self.launch
                    && old.generation == self.generation
                    && old.owner == self.owner
                    && old.daemon_epoch == self.daemon_epoch
                    && old.sequence.checked_add(1) == Some(self.sequence)
                    && now < old.expires_boottime_millis
                    && self.issued_boottime_millis >= old.issued_boottime_millis
                    && self.expires_boottime_millis > old.expires_boottime_millis =>
            {
                Ok(())
            }
            _ => Err(denied()),
        }
    }
}

/// Internal trusted daemon interface; never an Assignment capability action.
/// The adapter must verify exclusive host ownership outside the transaction,
/// then the store revalidates live Assignment authority atomically with the grant.
pub trait Repository: Send + Sync {
    /// # Errors
    /// Rejects stale/revoked bindings, blockers, late renewals and storage errors.
    fn grant_lease(
        &self,
        workspace: &str,
        lease: &Lease,
        now: u64,
    ) -> Result<(), RegistrationError>;
}

/// Host evidence collected by a trusted adapter, never decoded from an agent
/// request. The adapter verifies the live exclusive owner and reads
/// `CLOCK_BOOTTIME` in that owner's boot/time namespace on every observation.
pub struct Observation {
    pub owner: Owner,
    pub boottime_millis: u64,
}

/// External host boundary. Missing or uncertain ownership/clock evidence is an
/// error, not permission to use the previous observation.
pub trait AuthorityHost {
    /// # Errors
    /// Fails when exclusive ownership or a trustworthy clock cannot be verified.
    fn observe(&mut self) -> Result<Observation, RegistrationError>;
}

/// One daemon-lifetime coordinator for one immutable Assignment binding. This is
/// not a renewal transport or a scheduler: callers deliver only committed grants
/// and invoke it at the configured cadence. Restart creates a fresh epoch and
/// cannot adopt retained grants as permission. The repository rejects sequence 1
/// if history already exists; authorized recovery needs a separate protocol.
///
/// Any failed observation, expired deadline or uncertain commit permanently
/// fences this coordinator. Recreating it cannot bypass repository history.
/// Serial `&mut self` access prevents competing renewal branches within a lifetime.
pub struct Issuer {
    workspace: String,
    launch: Launch,
    generation: u64,
    daemon_epoch: String,
    previous: Option<Lease>,
    last_observation: Option<u64>,
    fenced: bool,
}

impl Issuer {
    /// Construct from trusted daemon binding and a unique daemon-lifetime epoch,
    /// not operator/agent-supplied owner, clock or retained lease payloads.
    /// # Errors
    /// Rejects malformed immutable binding before consulting external systems.
    pub fn new(
        workspace: String,
        launch: Launch,
        generation: u64,
        daemon_epoch: String,
    ) -> Result<Self, RegistrationError> {
        launch.validate()?;
        if workspace.trim().is_empty() || generation == 0 || daemon_epoch.trim().is_empty() {
            return Err(denied());
        }
        Ok(Self {
            workspace,
            launch,
            generation,
            daemon_epoch,
            previous: None,
            last_observation: None,
            fenced: false,
        })
    }

    /// Issue only after fresh external evidence and transactional reauthorization.
    /// No wall-clock fallback, local renewal, late revival or retry after an
    /// uncertain persistence result is allowed. Host operations precede the store
    /// call; no external operation runs inside the repository transaction.
    /// Storage/delivery latency never shifts the deadline: a consumer must check
    /// expiry using its own trusted clock before admission, not receipt time.
    /// # Errors
    /// Permanently fences on any host, policy or repository failure. A failed
    /// return must never be delivered as a grant, even if the commit succeeded.
    pub fn issue(
        &mut self,
        repository: &dyn Repository,
        host: &mut dyn AuthorityHost,
    ) -> Result<Lease, RegistrationError> {
        if self.fenced {
            return Err(denied());
        }
        // Default to terminal failure, including a commit whose reply is lost.
        self.fenced = true;
        let observation = host.observe()?;
        let now = observation.boottime_millis;
        if self.last_observation.is_some_and(|last| now < last) {
            return Err(denied());
        }
        let sequence = match &self.previous {
            Some(old) => old.sequence.checked_add(1).ok_or_else(denied)?,
            None => 1,
        };
        let lease = Lease {
            launch: self.launch.clone(),
            generation: self.generation,
            owner: observation.owner,
            daemon_epoch: self.daemon_epoch.clone(),
            sequence,
            issued_boottime_millis: now,
            expires_boottime_millis: now.checked_add(LEASE_MILLIS).ok_or_else(denied)?,
        };
        lease.validate(self.previous.as_ref(), now)?;
        repository.grant_lease(&self.workspace, &lease, now)?;
        self.previous = Some(lease.clone());
        self.last_observation = Some(now);
        self.fenced = false;
        Ok(lease)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant() -> Lease {
        Lease {
            launch: Launch {
                assignment_id: "assignment".into(),
                operation_id: "launch".into(),
                attempt_id: "attempt".into(),
                profile_revision: 1,
                runtime_id: "runtime".into(),
                incarnation_id: "incarnation".into(),
                conversation_id: "conversation".into(),
                store_id: "store".into(),
                forge_revision: "revision".into(),
                component_hash: "hash".into(),
                runtime_schema: 2,
            },
            generation: 1,
            owner: Owner {
                boot_id: "boot".into(),
                time_namespace: "time:[1]".into(),
                host_pid: 1,
                host_start: "10".into(),
                worker_pid: 2,
                worker_start: "11".into(),
                lock_device: "1".into(),
                lock_inode: "2".into(),
            },
            daemon_epoch: "epoch".into(),
            sequence: 1,
            issued_boottime_millis: 1_000,
            expires_boottime_millis: 31_000,
        }
    }

    struct Host {
        owner: Owner,
        now: u64,
        unavailable: bool,
        calls: usize,
    }

    impl AuthorityHost for Host {
        fn observe(&mut self) -> Result<Observation, RegistrationError> {
            self.calls += 1;
            if self.unavailable {
                return Err(denied());
            }
            Ok(Observation {
                owner: self.owner.clone(),
                boottime_millis: self.now,
            })
        }
    }

    #[derive(Default)]
    struct Store {
        grants: std::sync::Mutex<Vec<Lease>>,
        // Model a commit with a lost reply, not just a definite rejection.
        lose_reply: std::sync::atomic::AtomicBool,
    }

    impl Repository for Store {
        fn grant_lease(
            &self,
            workspace: &str,
            lease: &Lease,
            now: u64,
        ) -> Result<(), RegistrationError> {
            assert_eq!(workspace, "workspace");
            let mut grants = self.grants.lock().unwrap();
            lease.validate(grants.last(), now)?;
            grants.push(lease.clone());
            if self.lose_reply.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(denied());
            }
            Ok(())
        }
    }

    fn issuer(epoch: &str) -> Issuer {
        let lease = grant();
        Issuer::new(
            "workspace".into(),
            lease.launch,
            lease.generation,
            epoch.into(),
        )
        .unwrap()
    }

    fn host() -> Host {
        Host {
            owner: grant().owner,
            now: 1_000,
            unavailable: false,
            calls: 0,
        }
    }

    #[test]
    fn issuer_observes_and_persists_each_renewal_without_local_extension() {
        let mut issuer = issuer("epoch");
        let store = Store::default();
        let mut host = host();
        let first = issuer.issue(&store, &mut host).unwrap();
        host.now += RENEW_AFTER_MILLIS;
        let second = issuer.issue(&store, &mut host).unwrap();
        assert_eq!(second.sequence, 2);
        assert_eq!(second.expires_boottime_millis, 41_000);
        assert_eq!(first.expires_boottime_millis, 31_000);
        assert_eq!(host.calls, 2);
        assert_eq!(*store.grants.lock().unwrap(), vec![first, second]);
    }

    #[test]
    fn issuer_failure_is_terminal_even_if_host_becomes_available_again() {
        let store = Store::default();
        let mut issuer = issuer("epoch");
        let mut host = host();
        issuer.issue(&store, &mut host).unwrap();
        host.unavailable = true;
        assert!(issuer.issue(&store, &mut host).is_err());
        host.unavailable = false;
        host.now += RENEW_AFTER_MILLIS;
        assert!(issuer.issue(&store, &mut host).is_err());
        assert_eq!(host.calls, 2); // fenced retries perform no external calls
        assert_eq!(store.grants.lock().unwrap().len(), 1);
    }

    #[test]
    fn issuer_fences_clock_discontinuity_owner_change_expiry_and_overflow() {
        let changes: Vec<fn(&mut Host)> = vec![
            |h| h.now -= 1,
            |h| h.now = 31_000,
            |h| h.now = 31_001,
            |h| h.now = u64::MAX,
            |h| h.owner.boot_id.push('x'),
            |h| h.owner.time_namespace.push('x'),
            |h| h.owner.worker_start.push('x'),
            |h| h.owner.lock_inode.push('x'),
        ];
        for change in changes {
            let store = Store::default();
            let mut issuer = issuer("epoch");
            let mut host = host();
            issuer.issue(&store, &mut host).unwrap();
            change(&mut host);
            assert!(issuer.issue(&store, &mut host).is_err());
            host.now = 11_000;
            host.owner = grant().owner;
            assert!(issuer.issue(&store, &mut host).is_err());
            assert_eq!(store.grants.lock().unwrap().len(), 1);
        }
    }

    #[test]
    fn uncertain_commit_never_delivers_or_retries_a_grant() {
        let store = Store::default();
        store
            .lose_reply
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let mut issuer = issuer("epoch");
        let mut host = host();
        assert!(issuer.issue(&store, &mut host).is_err());
        assert_eq!(store.grants.lock().unwrap().len(), 1);
        store
            .lose_reply
            .store(false, std::sync::atomic::Ordering::Relaxed);
        host.now += RENEW_AFTER_MILLIS;
        assert!(issuer.issue(&store, &mut host).is_err());
        assert_eq!(host.calls, 1);
        assert_eq!(store.grants.lock().unwrap().len(), 1);
    }

    #[test]
    fn daemon_restart_cannot_turn_retained_grants_into_permission() {
        let store = Store::default();
        let mut old = issuer("old-epoch");
        let mut host = host();
        old.issue(&store, &mut host).unwrap();
        host.now += RENEW_AFTER_MILLIS;
        let mut restarted = issuer("new-epoch");
        assert!(restarted.issue(&store, &mut host).is_err());
        assert_eq!(store.grants.lock().unwrap().len(), 1);
        assert!(restarted.issue(&store, &mut host).is_err());
        assert_eq!(host.calls, 2);
    }

    #[test]
    fn exact_expiry_and_late_renewal_are_terminal() {
        let first = grant();
        assert!(first.validate(None, 999).is_err());
        assert!(first.validate(None, 1_000).is_ok());
        assert!(first.validate(None, 30_999).is_ok());
        assert!(first.validate(None, 31_000).is_err());
        let mut next = first.clone();
        next.sequence = 2;
        next.issued_boottime_millis += RENEW_AFTER_MILLIS;
        next.expires_boottime_millis += RENEW_AFTER_MILLIS;
        assert!(next.validate(Some(&first), 11_000).is_ok());
        assert!(next.validate(Some(&first), 31_000).is_err());
        assert!(next.validate(None, 11_000).is_err());
    }

    #[test]
    fn renewal_cannot_change_authority_ownership_or_clock_coordinates() {
        let first = grant();
        let mut next = first.clone();
        next.sequence = 2;
        next.issued_boottime_millis += RENEW_AFTER_MILLIS;
        next.expires_boottime_millis += RENEW_AFTER_MILLIS;
        let changes: Vec<fn(&mut Lease)> = vec![
            |l| l.generation += 1,
            |l| l.daemon_epoch.push('x'),
            |l| l.sequence += 1,
            |l| l.launch.assignment_id.push('x'),
            |l| l.launch.attempt_id.push('x'),
            |l| l.launch.incarnation_id.push('x'),
            |l| l.launch.profile_revision += 1,
            |l| l.owner.boot_id.push('x'),
            |l| l.owner.time_namespace.push('x'),
            |l| l.owner.host_pid += 1,
            |l| l.owner.host_start.push('x'),
            |l| l.owner.worker_pid += 1,
            |l| l.owner.worker_start.push('x'),
            |l| l.owner.lock_device.push('x'),
            |l| l.owner.lock_inode.push('x'),
            |l| l.expires_boottime_millis += 1,
            |l| {
                l.issued_boottime_millis = 0;
                l.expires_boottime_millis = LEASE_MILLIS;
            },
        ];
        for change in changes {
            let mut invalid = next.clone();
            change(&mut invalid);
            assert!(
                invalid.validate(Some(&first), 11_000).is_err(),
                "{invalid:?}"
            );
        }
    }
}
