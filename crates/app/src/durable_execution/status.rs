//! Read-only operator view of retained durable identity and authorization history.
//! It is persisted evidence, never process liveness, recovery or execution permission.
use super::{Admission, Backend};
use crate::RegistrationError;

/// Bounded boot-clock lease record. Only the host that issued it can interpret
/// the values, so the daemon never infers expiry or liveness from them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseSummary {
    pub sequence: u32,
    pub generation: u64,
    pub issued_boottime_millis: u64,
    pub expires_boottime_millis: u64,
}

/// Allowlisted identity: no store path, owner process facts, capability or credential.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Status {
    pub backend: Backend,
    pub assignment_id: String,
    pub runtime_id: String,
    pub operation_id: String,
    pub attempt_id: String,
    pub incarnation_id: String,
    pub profile_revision: u32,
    pub forge_revision: String,
    pub component_hash: String,
    pub admission: Admission,
    pub lease_count: u32,
    pub latest_lease: Option<LeaseSummary>,
    /// Closure revokes execution authority for every earlier lease.
    pub assignment_closed: bool,
}

pub trait Repository: Send + Sync {
    /// Read retained facts without starting, repairing or authorizing execution.
    /// `None` means no durable runtime is retained, not that no agent exists.
    /// # Errors
    /// Rejects unauthorized scope, invalid retained records and unreadable storage.
    fn durable_status(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<Option<Status>, RegistrationError>;
}

/// Authorization history as retained by Forge. It never claims a live grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseState {
    /// No lease was ever committed (not started, rejected or lost before grant).
    NeverGranted,
    /// An open Assignment has retained grants. Current validity is host-only.
    GrantedUnverified,
    /// Closure revoked authority; retained grants are history only.
    Revoked,
}

/// Operator-facing interpretation owned by policy, not by CLI/RPC adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct View {
    pub lease_state: LeaseState,
    pub missing_outcome: bool,
    /// Retained unresolved facts that make absence or safety unknown.
    pub uncertainty: Vec<&'static str>,
    /// Facts that block retirement or capacity release.
    pub retirement_blockers: Vec<String>,
}

const UNCERTAIN_KINDS: [(&str, &str); 5] = [
    ("provision-intent", "uncertain-provisioning"),
    ("interrupted-effect", "interrupted-effect"),
    ("authorization-uncertain", "authorization-uncertain"),
    ("recovery-claim", "recovery-claim-unresolved"),
    ("retirement-pending", "retirement-pending"),
];

impl Status {
    #[must_use]
    pub fn lease_state(&self) -> LeaseState {
        match (&self.latest_lease, self.assignment_closed) {
            (None, _) => LeaseState::NeverGranted,
            (Some(_), true) => LeaseState::Revoked,
            (Some(_), false) => LeaseState::GrantedUnverified,
        }
    }
}

/// Derive uncertainty only from retained unretired resources. Absence of a row
/// is never reported as proof that nothing is running or that cleanup is safe.
#[must_use]
pub fn view(info: &crate::AgentAssignmentInfo, status: Option<&Status>) -> View {
    let unretired = |kind: &str| {
        info.resources
            .iter()
            .any(|r| r.resource_kind == kind && r.retired_at_millis.is_none())
    };
    View {
        lease_state: status.map_or(LeaseState::NeverGranted, Status::lease_state),
        missing_outcome: unretired("missing-outcome"),
        uncertainty: UNCERTAIN_KINDS
            .iter()
            .filter(|(kind, _)| unretired(kind))
            .map(|(_, label)| *label)
            .collect(),
        retirement_blockers: info.recovery_blockers(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(latest: bool, closed: bool) -> Status {
        Status {
            backend: Backend::PiDurable,
            assignment_id: "a".into(),
            runtime_id: "r".into(),
            operation_id: "o".into(),
            attempt_id: "t".into(),
            incarnation_id: "i".into(),
            profile_revision: 1,
            forge_revision: "f".into(),
            component_hash: "c".into(),
            admission: Admission::Pending,
            lease_count: u32::from(latest),
            latest_lease: latest.then_some(LeaseSummary {
                sequence: 1,
                generation: 1,
                issued_boottime_millis: 1,
                expires_boottime_millis: 30_001,
            }),
            assignment_closed: closed,
        }
    }

    #[test]
    fn lease_state_reports_history_without_claiming_live_authority() {
        assert_eq!(status(false, false).lease_state(), LeaseState::NeverGranted);
        assert_eq!(status(false, true).lease_state(), LeaseState::NeverGranted);
        assert_eq!(
            status(true, false).lease_state(),
            LeaseState::GrantedUnverified
        );
        assert_eq!(status(true, true).lease_state(), LeaseState::Revoked);
    }
}
