use super::*;
use crate::authorization_lease::{Observation, Owner};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

fn grant() -> Lease {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../runner/test/authorization-lease.json"
    )))
    .unwrap()
}

struct Host<'a> {
    clock: &'a AtomicU64,
    owner: Owner,
    unavailable: bool,
    calls: usize,
}
impl AuthorityHost for Host<'_> {
    fn observe(&mut self) -> Result<Observation, RegistrationError> {
        self.calls += 1;
        if self.unavailable {
            return Err(denied());
        }
        Ok(Observation {
            owner: self.owner.clone(),
            boottime_millis: self.clock.load(Ordering::Relaxed),
        })
    }
}

#[derive(Default)]
struct Store {
    grants: Mutex<Vec<Lease>>,
    failures: Mutex<Vec<Failure>>,
    reject_grant: AtomicBool,
    lose_commit_reply: AtomicBool,
    reject_retention: AtomicBool,
}
impl Repository for Store {
    fn grant_lease(
        &self,
        _workspace: &str,
        lease: &Lease,
        now: u64,
    ) -> Result<(), RegistrationError> {
        if self.reject_grant.load(Ordering::Relaxed) {
            return Err(denied());
        }
        let mut grants = self.grants.lock().unwrap();
        lease.validate(grants.last(), now)?;
        grants.push(lease.clone());
        if self.lose_commit_reply.load(Ordering::Relaxed) {
            return Err(denied());
        }
        Ok(())
    }
}
impl SessionRepository for Store {
    fn retain_failure(
        &self,
        _workspace: &str,
        _launch: &Launch,
        failure: Failure,
    ) -> Result<(), RegistrationError> {
        if self.reject_retention.load(Ordering::Relaxed) {
            return Err(denied());
        }
        self.failures.lock().unwrap().push(failure);
        Ok(())
    }
}
struct Pipe<'a> {
    store: &'a Store,
    calls: usize,
    fail: bool,
}
impl Delivery for Pipe<'_> {
    fn deliver(&mut self, lease: &Lease) -> Result<(), RegistrationError> {
        // Prove commit-before-delivery, not just grant construction ordering.
        assert_eq!(self.store.grants.lock().unwrap().last(), Some(lease));
        self.calls += 1;
        if self.fail { Err(denied()) } else { Ok(()) }
    }
}
fn session() -> Session {
    Session::new("test".into(), grant().launch, 1, "fresh-epoch".into()).unwrap()
}

#[test]
fn commits_only_when_due_and_delivers_once_per_persisted_sequence() {
    let clock = AtomicU64::new(1000);
    let store = Store::default();
    let mut host = Host {
        clock: &clock,
        owner: grant().owner,
        unavailable: false,
        calls: 0,
    };
    let mut pipe = Pipe {
        store: &store,
        calls: 0,
        fail: false,
    };
    let mut session = session();
    assert!(matches!(
        session.tick(&store, &mut host, &mut pipe).unwrap(),
        Tick::Delivered {
            lease,
            next_renewal_millis: 11000
        } if lease.sequence == 1
    ));
    clock.store(10999, Ordering::Relaxed);
    assert_eq!(
        session.tick(&store, &mut host, &mut pipe).unwrap(),
        Tick::Waiting {
            next_renewal_millis: 11000
        }
    );
    assert_eq!(pipe.calls, 1);
    clock.store(11000, Ordering::Relaxed);
    assert!(matches!(
        session.tick(&store, &mut host, &mut pipe).unwrap(),
        Tick::Delivered {
            lease,
            ..
        } if lease.sequence == 2
    ));
    assert_eq!(pipe.calls, 2);
    assert_eq!(store.grants.lock().unwrap().len(), 2);
    assert!(store.failures.lock().unwrap().is_empty());
}

#[test]
fn outage_revocation_delivery_loss_clock_and_expiry_terminally_fence() {
    for fault in 0..8 {
        let clock = AtomicU64::new(1000);
        let store = Store::default();
        let mut host = Host {
            clock: &clock,
            owner: grant().owner,
            unavailable: false,
            calls: 0,
        };
        let mut pipe = Pipe {
            store: &store,
            calls: 0,
            fail: false,
        };
        let mut session = session();
        session.tick(&store, &mut host, &mut pipe).unwrap();
        clock.store(11000, Ordering::Relaxed);
        match fault {
            0 => host.unavailable = true,
            1 => store.reject_grant.store(true, Ordering::Relaxed),
            2 => pipe.fail = true,
            3 => clock.store(31000, Ordering::Relaxed),
            4 => clock.store(999, Ordering::Relaxed),
            5 => host.owner.worker_start = "changed".into(),
            6 => {
                session.stop(&store).unwrap();
            }
            7 => store.lose_commit_reply.store(true, Ordering::Relaxed),
            _ => unreachable!(),
        }
        assert!(session.tick(&store, &mut host, &mut pipe).is_err());
        let calls = (host.calls, pipe.calls);
        host.unavailable = false;
        host.owner = grant().owner;
        pipe.fail = false;
        clock.store(12000, Ordering::Relaxed);
        assert!(session.tick(&store, &mut host, &mut pipe).is_err());
        assert_eq!((host.calls, pipe.calls), calls);
        assert!(!store.failures.lock().unwrap().is_empty());
    }
}

#[test]
fn failed_retention_retries_diagnostics_only_not_authority_or_delivery() {
    let clock = AtomicU64::new(1000);
    let store = Store::default();
    store.reject_retention.store(true, Ordering::Relaxed);
    let mut host = Host {
        clock: &clock,
        owner: grant().owner,
        unavailable: false,
        calls: 0,
    };
    let mut pipe = Pipe {
        store: &store,
        calls: 0,
        fail: true,
    };
    let mut session = session();
    assert_eq!(
        session.tick(&store, &mut host, &mut pipe).unwrap_err().code,
        "authorization-retention-unavailable"
    );
    assert_eq!(store.grants.lock().unwrap().len(), 1);
    assert_eq!(pipe.calls, 1);
    store.reject_retention.store(false, Ordering::Relaxed);
    let calls = host.calls;
    assert!(session.tick(&store, &mut host, &mut pipe).is_err());
    assert_eq!(host.calls, calls);
    assert_eq!(pipe.calls, 1);
    assert_eq!(
        *store.failures.lock().unwrap(),
        vec![Failure::DeliveryUncertain]
    );
}

#[test]
fn persistence_delay_cannot_deliver_past_initial_or_prior_expiry() {
    struct Slow<'a> {
        store: Store,
        clock: &'a AtomicU64,
        advance_to: AtomicU64,
    }
    impl Repository for Slow<'_> {
        fn grant_lease(
            &self,
            workspace: &str,
            lease: &Lease,
            now: u64,
        ) -> Result<(), RegistrationError> {
            self.store.grant_lease(workspace, lease, now)?;
            self.clock
                .store(self.advance_to.load(Ordering::Relaxed), Ordering::Relaxed);
            Ok(())
        }
    }
    impl SessionRepository for Slow<'_> {
        fn retain_failure(
            &self,
            workspace: &str,
            launch: &Launch,
            failure: Failure,
        ) -> Result<(), RegistrationError> {
            self.store.retain_failure(workspace, launch, failure)
        }
    }
    for renewal in [false, true] {
        let clock = AtomicU64::new(1000);
        let slow = Slow {
            store: Store::default(),
            clock: &clock,
            advance_to: AtomicU64::new(if renewal { 1000 } else { 31000 }),
        };
        let mut host = Host {
            clock: &clock,
            owner: grant().owner,
            unavailable: false,
            calls: 0,
        };
        let mut pipe = Pipe {
            store: &slow.store,
            calls: 0,
            fail: false,
        };
        let mut session = session();
        if renewal {
            session.tick(&slow, &mut host, &mut pipe).unwrap();
            clock.store(11000, Ordering::Relaxed);
            slow.advance_to.store(31000, Ordering::Relaxed);
        }
        assert!(session.tick(&slow, &mut host, &mut pipe).is_err());
        assert_eq!(pipe.calls, usize::from(renewal));
        assert_eq!(
            slow.store.failures.lock().unwrap().last(),
            Some(&Failure::LeaseExpired)
        );
    }
}
