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
