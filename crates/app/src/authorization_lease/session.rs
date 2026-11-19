//! Commit-before-delivery and renewal scheduling for one daemon lifetime.
use super::{AuthorityHost, Issuer, Lease, RENEW_AFTER_MILLIS, Repository, denied};
use crate::{RegistrationError, durable_execution::Launch};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    AuthorityUnavailable,
    LeaseExpired,
    DeliveryUncertain,
    Stopped,
}

impl Failure {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::AuthorityUnavailable => "authorization-unavailable",
            Self::LeaseExpired => "authorization-expired",
            Self::DeliveryUncertain => "authorization-delivery-uncertain",
            Self::Stopped => "authorization-stopped",
        }
    }
}

/// Internal trusted persistence port; diagnostics may be retained after closure.
pub trait SessionRepository: Repository {
    /// Atomically retain a blocker and diagnostic without clearing older holds,
    /// resources or history. Do not require current execution permission.
    /// # Errors
    /// Rejects foreign/mismatched runtime identity or unavailable persistence.
    fn retain_failure(
        &self,
        workspace: &str,
        launch: &Launch,
        failure: Failure,
    ) -> Result<(), RegistrationError>;
}

/// The adapter owns the private directly spawned supervisor pipe. Delivery must
/// be bounded/nonblocking. A successful write is not worker acceptance or liveness.
pub trait Delivery {
    /// # Errors
    /// Returns uncertainty for failed, partial or unavailable delivery. Never
    /// locally retries a partially written control or extends its deadline.
    fn deliver(&mut self, lease: &Lease) -> Result<(), RegistrationError>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum Tick {
    Waiting {
        next_renewal_millis: u64,
    },
    Delivered {
        lease: Box<Lease>,
        next_renewal_millis: u64,
    },
}

impl Tick {
    #[must_use]
    pub fn next_renewal_millis(&self) -> u64 {
        match self {
            Self::Waiting {
                next_renewal_millis,
            }
            | Self::Delivered {
                next_renewal_millis,
                ..
            } => *next_renewal_millis,
        }
    }
}

enum State {
    New,
    Active {
        next_renewal: u64,
        last_observed: u64,
    },
    Fenced(Failure),
}

/// No retained grant adoption: construct only with a fresh trusted daemon epoch.
/// Timer ticks/heartbeats cannot issue permission without host/store validation.
pub struct Session {
    issuer: Issuer,
    state: State,
}

impl Session {
    /// # Errors
    /// Rejects invalid immutable binding before observing host or repository.
    pub fn new(
        workspace: String,
        launch: Launch,
        generation: u64,
        epoch: String,
    ) -> Result<Self, RegistrationError> {
        Ok(Self {
            issuer: Issuer::new(workspace, launch, generation, epoch)?,
            state: State::New,
        })
    }

    /// Withdraw permission after an adapter failure, preserving the first
    /// terminal cause. This may retry diagnostic retention, never authorization.
    /// # Errors
    /// Returns a stable error when durable retention is unavailable.
    pub fn withdraw(
        &mut self,
        repository: &dyn SessionRepository,
        failure: Failure,
    ) -> Result<(), RegistrationError> {
        let failure = match self.state {
            State::Fenced(first) => first,
            _ => failure,
        };
        self.state = State::Fenced(failure);
        self.issuer.fenced = true;
        repository
            .retain_failure(&self.issuer.workspace, &self.issuer.launch, failure)
            .map_err(|_| {
                RegistrationError::new(
                    "authorization-retention-unavailable",
                    "authorization stopped; retained runtime capacity must not be released",
                )
            })
    }

    fn fail(
        &mut self,
        repository: &dyn SessionRepository,
        failure: Failure,
    ) -> Result<Tick, RegistrationError> {
        self.withdraw(repository, failure)?;
        Err(denied())
    }

    /// Stop renewal and retain uncertainty; this is not retirement or process
    /// absence. Failure to persist leaves this object fenced for a later retry.
    /// # Errors
    /// Returns a retention error if storage is unavailable.
    pub fn stop(&mut self, repository: &dyn SessionRepository) -> Result<(), RegistrationError> {
        let failure = match self.state {
            State::Fenced(failure) => failure,
            _ => Failure::Stopped,
        };
        self.withdraw(repository, failure)
    }

    /// Read fresh evidence, reauthorize/commit only when due, then reobserve
    /// before bounded delivery. Storage latency never moves a deadline. Missing
    /// the old grant's deadline fences even if a new commit eventually succeeds.
    /// # Errors
    /// Any authority, clock, persistence or delivery failure terminally fences
    /// renewal and attempts durable retention. Later ticks may retry retention
    /// only; they cannot contact delivery or revive execution.
    pub fn tick(
        &mut self,
        repository: &dyn SessionRepository,
        host: &mut dyn AuthorityHost,
        delivery: &mut dyn Delivery,
    ) -> Result<Tick, RegistrationError> {
        if let State::Fenced(failure) = self.state {
            return self.fail(repository, failure);
        }
        let Ok(observation) = host.observe() else {
            return self.fail(repository, Failure::AuthorityUnavailable);
        };
        let now = observation.boottime_millis;
        let old_expiry = self
            .issuer
            .previous
            .as_ref()
            .map(|lease| lease.expires_boottime_millis);
        if let State::Active {
            next_renewal,
            last_observed,
        } = self.state
        {
            if now < last_observed
                || self
                    .issuer
                    .previous
                    .as_ref()
                    .is_none_or(|lease| lease.owner != observation.owner)
            {
                return self.fail(repository, Failure::AuthorityUnavailable);
            }
            if old_expiry.is_some_and(|expiry| now >= expiry) {
                return self.fail(repository, Failure::LeaseExpired);
            }
            self.state = State::Active {
                next_renewal,
                last_observed: now,
            };
            if now < next_renewal {
                return Ok(Tick::Waiting {
                    next_renewal_millis: next_renewal,
                });
            }
        }
        let Ok(lease) = self.issuer.issue_observed(repository, observation) else {
            return self.fail(repository, Failure::AuthorityUnavailable);
        };
        let Ok(after) = host.observe() else {
            return self.fail(repository, Failure::AuthorityUnavailable);
        };
        if after.owner != lease.owner || after.boottime_millis < now {
            return self.fail(repository, Failure::AuthorityUnavailable);
        }
        if after.boottime_millis >= lease.expires_boottime_millis
            || old_expiry.is_some_and(|expiry| after.boottime_millis >= expiry)
        {
            return self.fail(repository, Failure::LeaseExpired);
        }
        if delivery.deliver(&lease).is_err() {
            return self.fail(repository, Failure::DeliveryUncertain);
        }
        let next_renewal = lease.issued_boottime_millis + RENEW_AFTER_MILLIS; // bounded by the validated 30-second expiry
        self.state = State::Active {
            next_renewal,
            last_observed: after.boottime_millis,
        };
        Ok(Tick::Delivered {
            lease: Box::new(lease),
            next_renewal_millis: next_renewal,
        })
    }
}

#[cfg(test)]
mod tests;
