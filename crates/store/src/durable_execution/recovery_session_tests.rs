//! Actual SQLite commits/retained failures with scripted host and delivery.
//! No process cleanup or writable-runtime recovery is inferred from these fakes.
use super::*;
use forge_app::{
    authorization_lease::{
        AuthorityHost, Lease, Repository as LeaseRepository,
        session::{Delivery as LeaseDelivery, Failure, SessionRepository, Tick},
    },
    durable_execution::recovery::{
        authorization::{Grant, Repository as RecoveryRepository},
        session::{Delivery as RecoveryDelivery, Session},
    },
};
use std::{collections::VecDeque, sync::Mutex};

fn failure() -> RegistrationError {
    RegistrationError::new("scripted-unavailable", "controlled failure")
}
struct Observer {
    owner: Owner,
    times: VecDeque<u64>,
    calls: usize,
    fail_on: Option<usize>,
    drift_on: Option<usize>,
}
impl AuthorityHost for Observer {
    fn observe(&mut self) -> Result<Observation, RegistrationError> {
        self.calls += 1;
        if self.fail_on == Some(self.calls) {
            return Err(failure());
        }
        let mut owner = self.owner.clone();
        if self.drift_on == Some(self.calls) {
            owner.worker_start.push_str("-foreign");
        }
        Ok(Observation {
            owner,
            boottime_millis: self.times.pop_front().expect("unexpected host observation"),
        })
    }
}
fn observer(prepared: &Prepared, times: &[u64]) -> Observer {
    Observer {
        owner: prepared.grant().lease.owner.clone(),
        times: times.iter().copied().collect(),
        calls: 0,
        fail_on: None,
        drift_on: None,
    }
}
struct Store<'a> {
    inner: &'a SqliteProjectRepository,
    commits: Mutex<usize>,
    lose_reply: bool,
    conflict_reply: bool,
    retention_unavailable: bool,
    retained: Mutex<Vec<Failure>>,
}
impl<'a> Store<'a> {
    fn new(inner: &'a SqliteProjectRepository) -> Self {
        Self {
            inner,
            commits: Mutex::new(0),
            lose_reply: false,
            conflict_reply: false,
            retention_unavailable: false,
            retained: Mutex::new(Vec::new()),
        }
    }
}
impl LeaseRepository for Store<'_> {
    fn grant_lease(
        &self,
        workspace: &str,
        lease: &Lease,
        now: u64,
    ) -> Result<(), RegistrationError> {
        self.inner.grant_lease(workspace, lease, now)
    }
}
impl RecoveryRepository for Store<'_> {
    fn grant_recovery(
        &self,
        workspace: &str,
        prepared: &Prepared,
        now: u64,
    ) -> Result<Grant, RegistrationError> {
        *self.commits.lock().unwrap() += 1;
        let mut grant = self.inner.grant_recovery(workspace, prepared, now)?;
        if self.lose_reply {
            return Err(failure());
        }
        if self.conflict_reply {
            grant.evidence_sha256 = "c".repeat(64);
        }
        Ok(grant)
    }
}
impl SessionRepository for Store<'_> {
    fn retain_failure(
        &self,
        workspace: &str,
        launch: &Launch,
        failure: Failure,
    ) -> Result<(), RegistrationError> {
        self.retained.lock().unwrap().push(failure);
        if self.retention_unavailable {
            return Err(self::failure());
        }
        self.inner.retain_failure(workspace, launch, failure)
    }
}
struct Pipe<'a> {
    store: &'a SqliteProjectRepository,
    grants: Vec<Grant>,
    leases: Vec<Lease>,
    fail: bool,
    watchdog: Option<LeaseWatchdog>,
}
impl<'a> Pipe<'a> {
    fn new(store: &'a SqliteProjectRepository) -> Self {
        Self {
            store,
            grants: vec![],
            leases: vec![],
            fail: false,
            watchdog: None,
        }
    }
}
impl RecoveryDelivery for Pipe<'_> {
    fn deliver_recovery(&mut self, grant: &Grant) -> Result<(), RegistrationError> {
        let info = self.store.agent_assignment_info("assignment").unwrap();
        let audit = info
            .resources
            .iter()
            .find(|r| r.resource_kind == "recovery-grant")
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Grant>(&audit.resource_id).unwrap(),
            *grant
        );
        assert!(
            info.resources
                .iter()
                .find(|r| r.resource_kind == "recovery-claim")
                .unwrap()
                .retired_at_millis
                .is_some()
        );
        if let Some(watchdog) = &mut self.watchdog {
            watchdog.accept_recovery(grant, grant.lease.issued_boottime_millis + 1)?;
        }
        self.grants.push(grant.clone());
        if self.fail {
            return Err(failure());
        }
        Ok(())
    }
}
impl LeaseDelivery for Pipe<'_> {
    fn deliver(&mut self, lease: &Lease) -> Result<(), RegistrationError> {
        let current: String = self.store.connection.lock().unwrap().query_row(
            "SELECT lease_json FROM assignment_authorization_leases WHERE assignment_id = 'assignment' ORDER BY sequence DESC LIMIT 1", [], |r| r.get(0),
        ).unwrap();
        assert_eq!(serde_json::from_str::<Lease>(&current).unwrap(), *lease);
        if let Some(watchdog) = &mut self.watchdog {
            watchdog.accept(lease.clone(), lease.issued_boottime_millis + 1)?;
        }
        self.leases.push(lease.clone());
        if self.fail {
            return Err(failure());
        }
        Ok(())
    }
}
fn retained(f: &Fixture) {
    let info = f.reopen().agent_assignment_info("assignment").unwrap();
    assert!(info.capacity_reserved());
    assert!(
        info.resources
            .iter()
            .any(|r| r.resource_kind == "authorization-uncertain" && r.retired_at_millis.is_none())
    );
}

#[test]
fn recovery_session_commits_then_delivers_once_and_renews_original_attempt() {
    let (f, original, claim) = reserved();
    let prepared = proposal(&original, &claim);
    let expected = prepared.grant().clone();
    let mut host = observer(&prepared, &[40_000, 40_001, 49_999, 50_000, 50_001]);
    let mut session = Session::new("test".into(), prepared).unwrap();
    let store = Store::new(&f.store);
    let mut pipe = Pipe::new(&f.store);
    pipe.watchdog =
        Some(LeaseWatchdog::for_recovery(expected.lease.owner.clone(), expected.clone()).unwrap());
    assert!(pipe.watchdog.as_ref().unwrap().last_lease().is_none());
    let before = snapshot_except(
        &f,
        &[
            "assignment_authorization_leases",
            "agent_assignment_resources",
        ],
    );
    assert_eq!(
        session.tick(&store, &mut host, &mut pipe).unwrap(),
        Tick::Delivered {
            lease: Box::new(expected.lease.clone()),
            next_renewal_millis: 50_000,
        }
    );
    assert_eq!(
        session.tick(&store, &mut host, &mut pipe).unwrap(),
        Tick::Waiting {
            next_renewal_millis: 50_000
        }
    );
    let Tick::Delivered {
        lease,
        next_renewal_millis,
    } = session.tick(&store, &mut host, &mut pipe).unwrap()
    else {
        panic!("renewal not delivered")
    };
    assert_eq!(lease.sequence, 3);
    assert_eq!(lease.launch, original.launch);
    assert_eq!(lease.owner, expected.lease.owner);
    assert_eq!(lease.daemon_epoch, claim.daemon_epoch);
    assert_eq!(next_renewal_millis, 60_000);
    assert_eq!(pipe.grants, vec![expected]);
    assert_eq!(
        pipe.watchdog.as_ref().unwrap().last_lease(),
        Some(lease.as_ref())
    );
    assert_eq!(pipe.leases, vec![*lease]);
    assert_eq!(*store.commits.lock().unwrap(), 1);
    assert_eq!(host.calls, 5);
    assert_eq!(
        snapshot_except(
            &f,
            &[
                "assignment_authorization_leases",
                "agent_assignment_resources"
            ]
        ),
        before
    );
}

#[test]
fn recovery_session_lost_or_conflicting_commit_reply_never_delivers_or_retries() {
    for conflict in [false, true] {
        let (f, original, claim) = reserved();
        let prepared = proposal(&original, &claim);
        let mut host = observer(&prepared, &[40_000]);
        let mut session = Session::new("test".into(), prepared).unwrap();
        let mut store = Store::new(&f.store);
        store.lose_reply = !conflict;
        store.conflict_reply = conflict;
        let mut pipe = Pipe::new(&f.store);
        assert!(session.tick(&store, &mut host, &mut pipe).is_err());
        store.lose_reply = false;
        store.conflict_reply = false;
        assert!(session.tick(&store, &mut host, &mut pipe).is_err());
        assert_eq!(*store.commits.lock().unwrap(), 1);
        assert_eq!(host.calls, 1);
        assert!(pipe.grants.is_empty());
        assert_eq!(
            store.retained.lock().unwrap().as_slice(),
            &[Failure::AuthorityUnavailable; 2]
        );
        retained(&f);
    }
}

#[test]
fn recovery_session_reobserves_owner_clock_and_deadline_after_commit() {
    for fault in ["unavailable", "owner", "rollback", "expiry"] {
        let (f, original, claim) = reserved();
        let prepared = proposal(&original, &claim);
        let after = match fault {
            "rollback" => 39_999,
            "expiry" => 70_000,
            _ => 40_001,
        };
        let mut host = observer(&prepared, &[40_000, after]);
        if fault == "unavailable" {
            host.fail_on = Some(2);
        }
        if fault == "owner" {
            host.drift_on = Some(2);
        }
        let mut session = Session::new("test".into(), prepared).unwrap();
        let store = Store::new(&f.store);
        let mut pipe = Pipe::new(&f.store);
        assert!(
            session.tick(&store, &mut host, &mut pipe).is_err(),
            "{fault}"
        );
        assert!(session.tick(&store, &mut host, &mut pipe).is_err());
        assert_eq!(*store.commits.lock().unwrap(), 1);
        assert_eq!(host.calls, 2);
        assert!(pipe.grants.is_empty());
        retained(&f);
    }
}

#[test]
fn recovery_session_initial_owner_clock_or_authority_failure_cannot_commit() {
    for fault in [
        "unavailable",
        "owner",
        "early",
        "expiry",
        "revoked",
        "freshness",
    ] {
        let (f, original, claim) = reserved();
        let prepared = proposal(&original, &claim);
        let now = match fault {
            "early" => 39_999,
            "expiry" => 70_000,
            _ => 40_000,
        };
        let mut host = observer(&prepared, &[now]);
        if fault == "unavailable" {
            host.fail_on = Some(1);
        }
        if fault == "owner" {
            host.drift_on = Some(1);
        }
        if fault == "revoked" {
            f.store
                .connection
                .lock()
                .unwrap()
                .execute(
                    "UPDATE agent_assignments SET closed_at_millis = 1 WHERE id = 'assignment'",
                    [],
                )
                .unwrap();
        }
        if fault == "freshness" {
            f.store
                .connection
                .lock()
                .unwrap()
                .execute(
                    "INSERT INTO record_work(record_id, generation) SELECT record_id, 2 FROM agent_assignments WHERE id = 'assignment' ON CONFLICT(record_id) DO UPDATE SET generation = 2",
                    [],
                )
                .unwrap();
        }
        let mut session = Session::new("test".into(), prepared).unwrap();
        let store = Store::new(&f.store);
        let mut pipe = Pipe::new(&f.store);
        assert!(
            session.tick(&store, &mut host, &mut pipe).is_err(),
            "{fault}"
        );
        assert!(session.tick(&store, &mut host, &mut pipe).is_err());
        assert_eq!(host.calls, 1);
        assert!(pipe.grants.is_empty());
        assert!(
            !f.store
                .agent_assignment_info("assignment")
                .unwrap()
                .resources
                .iter()
                .any(|r| r.resource_kind == "recovery-grant")
        );
        retained(&f);
    }
}

#[test]
fn recovery_session_uncertain_initial_delivery_retains_first_cause_even_if_retention_failed() {
    let (f, original, claim) = reserved();
    let prepared = proposal(&original, &claim);
    let mut host = observer(&prepared, &[40_000, 40_001]);
    let mut session = Session::new("test".into(), prepared).unwrap();
    let mut store = Store::new(&f.store);
    store.retention_unavailable = true;
    let mut pipe = Pipe::new(&f.store);
    pipe.fail = true;
    assert_eq!(
        session.tick(&store, &mut host, &mut pipe).unwrap_err().code,
        "authorization-retention-unavailable"
    );
    store.retention_unavailable = false;
    pipe.fail = false;
    session.stop(&store).unwrap();
    assert!(session.tick(&store, &mut host, &mut pipe).is_err());
    assert_eq!(
        store.retained.lock().unwrap().as_slice(),
        &[Failure::DeliveryUncertain; 3]
    );
    assert_eq!(pipe.grants.len(), 1);
    assert_eq!(host.calls, 2);
    assert_eq!(*store.commits.lock().unwrap(), 1);
    retained(&f);
}

#[test]
fn recovery_session_active_expiry_revocation_and_delivery_faults_use_existing_renewal_fences() {
    for fault in ["expiry", "revoked", "delivery", "stopped"] {
        let (f, original, claim) = reserved();
        let prepared = proposal(&original, &claim);
        let next = if fault == "expiry" { 70_000 } else { 50_000 };
        let mut host = observer(&prepared, &[40_000, 40_001, next, next + 1]);
        let mut session = Session::new("test".into(), prepared).unwrap();
        let store = Store::new(&f.store);
        let mut pipe = Pipe::new(&f.store);
        session.tick(&store, &mut host, &mut pipe).unwrap();
        if fault == "revoked" {
            f.store
                .connection
                .lock()
                .unwrap()
                .execute(
                    "UPDATE agent_assignments SET closed_at_millis = 1 WHERE id = 'assignment'",
                    [],
                )
                .unwrap();
        }
        if fault == "delivery" {
            pipe.fail = true;
        }
        if fault == "stopped" {
            session.stop(&store).unwrap();
        }
        assert!(
            session.tick(&store, &mut host, &mut pipe).is_err(),
            "{fault}"
        );
        let calls = host.calls;
        assert!(session.tick(&store, &mut host, &mut pipe).is_err());
        assert_eq!(host.calls, calls);
        assert_eq!(pipe.grants.len(), 1);
        assert_eq!(pipe.leases.len(), usize::from(fault == "delivery"));
        assert_eq!(*store.commits.lock().unwrap(), 1);
        retained(&f);
    }
}

#[test]
fn recovery_session_stopping_while_waiting_retains_claim_without_contacting_host_or_delivery() {
    let (f, original, claim) = reserved();
    let prepared = proposal(&original, &claim);
    let mut host = observer(&prepared, &[]);
    let mut session = Session::new("test".into(), prepared).unwrap();
    let store = Store::new(&f.store);
    let mut pipe = Pipe::new(&f.store);
    session.stop(&store).unwrap();
    assert!(session.tick(&store, &mut host, &mut pipe).is_err());
    assert_eq!(host.calls, 0);
    assert_eq!(*store.commits.lock().unwrap(), 0);
    assert!(pipe.grants.is_empty());
    let info = f.reopen().agent_assignment_info("assignment").unwrap();
    assert!(
        info.resources
            .iter()
            .any(|r| r.resource_kind == "recovery-claim" && r.retired_at_millis.is_none())
    );
    retained(&f);
}
