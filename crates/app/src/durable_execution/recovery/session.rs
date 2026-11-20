//! Verified recovery commit/delivery followed by ordinary bounded renewal.
//! No retained grant adoption, host construction or automatic scheduling.
use super::authorization::{Grant, Prepared, Repository as RecoveryRepository};
use crate::{
    RegistrationError,
    authorization_lease::{
        AuthorityHost,
        session::{
            Delivery as LeaseDelivery, Failure, Session as LeaseSession, SessionRepository, Tick,
        },
    },
};

/// Both transitions must use the same authoritative repository. No external I/O
/// belongs inside either transaction; current policy is revalidated on each grant.
pub trait Repository: SessionRepository + RecoveryRepository {}
impl<T: SessionRepository + RecoveryRepository> Repository for T {}

/// Internal private-channel adapter, not an agent/operator payload interface.
/// The initial delivery must invoke the exact-envelope recovery handshake;
/// subsequent deliveries use ordinary sequential lease controls.
pub trait Delivery: LeaseDelivery {
    /// # Errors
    /// Failed/partial/blocked delivery is uncertainty; never replay bytes locally.
    fn deliver_recovery(&mut self, grant: &Grant) -> Result<(), RegistrationError>;
}

enum State {
    Waiting,
    Active(Box<LeaseSession>),
    Fenced(Failure),
}

/// Consumes opaque freshly verified authority, never a deserialized audit. The
/// caller must retain verified exclusive ownership through commit and delivery.
/// This coordinator cannot prove descendant cleanup or construct a waiting host.
pub struct Session {
    workspace: String,
    prepared: Prepared,
    state: State,
}

fn denied() -> RegistrationError {
    RegistrationError::new(
        "durable-recovery-session-denied",
        "recovery delivery stopped; retain diagnostics and runtime capacity",
    )
}

impl Session {
    /// # Errors
    /// Rejects invalid Workspace/binding; no host, transaction or delivery occurs.
    pub fn new(workspace: String, prepared: Prepared) -> Result<Self, RegistrationError> {
        if workspace.trim().is_empty() {
            return Err(denied());
        }
        prepared.validate(prepared.grant().lease.issued_boottime_millis)?;
        Ok(Self {
            workspace,
            prepared,
            state: State::Waiting,
        })
    }

    /// Terminally withdraw permission, preserving the first cause. Retention may
    /// be retried after storage failure; neither grants nor delivery are retried.
    /// # Errors
    /// Failed diagnostic retention leaves this object fenced and reports it.
    pub fn withdraw(
        &mut self,
        repository: &dyn Repository,
        failure: Failure,
    ) -> Result<(), RegistrationError> {
        if let State::Active(session) = &mut self.state {
            return session.withdraw(repository, failure);
        }
        let first = match self.state {
            State::Fenced(first) => first,
            _ => failure,
        };
        self.state = State::Fenced(first);
        repository
            .retain_failure(&self.workspace, &self.prepared.grant().lease.launch, first)
            .map_err(|_| {
                RegistrationError::new(
                    "authorization-retention-unavailable",
                    "authorization stopped; retained runtime capacity must not be released",
                )
            })
    }

    /// Stop without claiming retirement, releasing capacity or proving absence.
    /// # Errors
    /// Reports unavailable retention while preserving the terminal fence.
    pub fn stop(&mut self, repository: &dyn Repository) -> Result<(), RegistrationError> {
        self.withdraw(repository, Failure::Stopped)
    }

    fn fail(
        &mut self,
        repository: &dyn Repository,
        failure: Failure,
    ) -> Result<Tick, RegistrationError> {
        self.withdraw(repository, failure)?;
        Err(denied())
    }

    /// Observe the new waiting owner, commit the exact prepared grant, reobserve
    /// before bounded initial delivery, then use ordinary renewal policy. Storage
    /// or delivery latency never shifts a deadline. No failed commit result is
    /// deliverable, even if it actually committed; a later tick only retains the
    /// original failure. This deliberately does not exploit repository replay to
    /// turn an uncertain transport/commit into execution permission.
    /// # Errors
    /// Any owner, clock, authority, commit or delivery uncertainty fences forever.
    pub fn tick(
        &mut self,
        repository: &dyn Repository,
        host: &mut dyn AuthorityHost,
        delivery: &mut dyn Delivery,
    ) -> Result<Tick, RegistrationError> {
        match &mut self.state {
            State::Active(session) => return session.tick(repository, host, delivery),
            State::Fenced(first) => {
                let first = *first;
                return self.fail(repository, first);
            }
            State::Waiting => {}
        }
        let expected = self.prepared.grant();
        let Ok(before) = host.observe() else {
            return self.fail(repository, Failure::AuthorityUnavailable);
        };
        if before.owner != expected.lease.owner
            || before.boottime_millis < expected.lease.issued_boottime_millis
        {
            return self.fail(repository, Failure::AuthorityUnavailable);
        }
        if self.prepared.validate(before.boottime_millis).is_err() {
            return self.fail(repository, Failure::LeaseExpired);
        }
        let Ok(committed) =
            repository.grant_recovery(&self.workspace, &self.prepared, before.boottime_millis)
        else {
            return self.fail(repository, Failure::AuthorityUnavailable);
        };
        if committed != *expected {
            return self.fail(repository, Failure::AuthorityUnavailable);
        }
        let Ok(after) = host.observe() else {
            return self.fail(repository, Failure::AuthorityUnavailable);
        };
        if after.owner != expected.lease.owner || after.boottime_millis < before.boottime_millis {
            return self.fail(repository, Failure::AuthorityUnavailable);
        }
        if self.prepared.validate(after.boottime_millis).is_err() {
            return self.fail(repository, Failure::LeaseExpired);
        }
        // Construct the renewal state only from this in-memory verified object
        // and identical live transaction result, never a retained audit.
        let Ok(session) = LeaseSession::after_recovery(
            self.workspace.clone(),
            &self.prepared,
            &committed,
            after.boottime_millis,
        ) else {
            return self.fail(repository, Failure::AuthorityUnavailable);
        };
        if delivery.deliver_recovery(&committed).is_err() {
            return self.fail(repository, Failure::DeliveryUncertain);
        }
        let next_renewal_millis =
            committed.lease.issued_boottime_millis + crate::authorization_lease::RENEW_AFTER_MILLIS;
        self.state = State::Active(Box::new(session));
        Ok(Tick::Delivered {
            lease: Box::new(committed.lease),
            next_renewal_millis,
        })
    }
}
