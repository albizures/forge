//! Failed durable launches (T26): specific retained diagnostics and the policy
//! for reconciling a launch that never received any grant.
//!
//! A launch that fails after its claim must never strand the Assignment. When
//! the original host may have run, the normal revoke-then-retire path applies
//! with the host's own receipt. When no grant was ever committed and the
//! admission was never sent, a narrower proof suffices; this module decides
//! which proofs are acceptable. It performs no I/O.
use super::{Admission, Launch};
use crate::RegistrationError;

/// Where a durable launch failed. Each is retained as a `durable-launch-failure`
/// resource whose id is [`Failure::code`], shown by status until verified
/// reconciliation retires it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    /// Protected runtime storage or the runner's protected input was not written.
    RuntimeInput,
    /// The launch intent was refused (the Assignment closed or moved on).
    LaunchIntent,
    /// The matched host could not be spawned.
    Spawn,
    /// The host sent no valid `owned` evidence for the matched components.
    Owned,
    /// The spawned host could not be bound through the interrupted-effect gate.
    Bind,
    /// The first grant could not be committed or delivered.
    Grant,
}

impl Failure {
    pub const ALL: [Self; 6] = [
        Self::RuntimeInput,
        Self::LaunchIntent,
        Self::Spawn,
        Self::Owned,
        Self::Bind,
        Self::Grant,
    ];

    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::RuntimeInput => "durable-launch-runtime-input-failed",
            Self::LaunchIntent => "durable-launch-intent-refused",
            Self::Spawn => "durable-launch-spawn-failed",
            Self::Owned => "durable-launch-owned-invalid",
            Self::Bind => "durable-launch-bind-failed",
            Self::Grant => "durable-launch-grant-failed",
        }
    }

    #[must_use]
    pub fn parse(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|failure| failure.code() == code)
    }
}

/// Evidence that a launch with no grant never acted, gathered by the server.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Proof {
    /// The launch intent was never committed, so no host was ever spawned: the
    /// store refuses a launch intent for a closed Assignment.
    NeverLaunched,
    /// The runtime directory was sealed under its exclusive lock with no owner
    /// evidence, so no host ever owned it and none ever can.
    NeverOwned,
    /// The original host fenced before any grant and proved, as subreaper, that
    /// its worker and every descendant are gone (`ungranted-receipt-*.json`).
    UngrantedQuiescence,
}

impl Proof {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NeverLaunched => "never-launched",
            Self::NeverOwned => "never-owned",
            Self::UngrantedQuiescence => "ungranted-quiescence",
        }
    }
}

/// Store facts read in the reconciling transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Facts {
    pub closed: bool,
    pub admission: Admission,
    pub lease_count: u32,
    pub launch_started: bool,
    /// A host retirement operation was journaled; it must finish that way.
    pub retirement_begun: bool,
}

fn refused(code: &'static str, message: &str) -> RegistrationError {
    RegistrationError::new(code, message.to_owned())
}

/// # Errors
/// Requires revoked authority, a never-sent admission, no committed grant and
/// no begun host retirement; `NeverLaunched` also requires no launch intent.
pub fn validate(facts: &Facts, proof: Proof) -> Result<(), RegistrationError> {
    if !facts.closed {
        return Err(refused(
            "assignment-not-revoked",
            "failed-launch reconciliation requires revoked authority",
        ));
    }
    if facts.admission != Admission::Pending || facts.lease_count != 0 {
        return Err(refused(
            "durable-launch-granted",
            "a launch that received a grant or sent its admission needs the original host's retirement or crash proof",
        ));
    }
    if facts.retirement_begun {
        return Err(refused(
            "durable-retirement-begun",
            "a host retirement is in progress; it completes only with the host's receipt",
        ));
    }
    if proof == Proof::NeverLaunched && facts.launch_started {
        return Err(refused(
            "durable-launch-started",
            "the launch intent was committed; a host may have been spawned",
        ));
    }
    Ok(())
}

/// Resources a proven never-granted launch discharges: its runtime identity,
/// the launch claim and its retained launch/authorization diagnostics. Effects,
/// recovery claims and anything unknown remain blockers.
#[must_use]
pub fn reconciles_resource(launch: &Launch, kind: &str, id: &str) -> bool {
    super::retirement::reconciles_resource(launch, kind, id)
}

pub trait Repository: Send + Sync {
    /// Retain one specific launch failure for status (idempotent).
    /// # Errors
    /// Rejects foreign scope or runtime and storage failures.
    fn record_launch_failure(
        &self,
        workspace: &str,
        launch: &Launch,
        failure: Failure,
    ) -> Result<(), RegistrationError>;

    /// Reconcile a never-granted launch with `proof`, which the caller holds
    /// (any exclusive inspection guard stays alive through this commit).
    /// Exact retries keep the original timestamps.
    /// # Errors
    /// Policy refusals from [`validate`], foreign scope or runtime, storage.
    fn reconcile_ungranted_launch(
        &self,
        workspace: &str,
        launch: &Launch,
        proof: Proof,
    ) -> Result<(), RegistrationError>;

    /// A launch claim with no reserved runtime (written before T26 made the
    /// claim atomic): the closed Assignment can never reserve one, so the claim
    /// is discharged. Exact retries are no-ops.
    /// # Errors
    /// Rejects an open Assignment, a reserved runtime and storage failures.
    fn reconcile_unreserved_claim(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<(), RegistrationError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            closed: true,
            admission: Admission::Pending,
            lease_count: 0,
            launch_started: false,
            retirement_begun: false,
        }
    }

    #[test]
    fn codes_round_trip_and_are_distinct() {
        for failure in Failure::ALL {
            assert_eq!(Failure::parse(failure.code()), Some(failure));
        }
        assert_eq!(Failure::parse("durable-launch-other"), None);
    }

    #[test]
    fn only_revoked_never_granted_unsent_launches_reconcile() {
        for proof in [
            Proof::NeverLaunched,
            Proof::NeverOwned,
            Proof::UngrantedQuiescence,
        ] {
            validate(&facts(), proof).unwrap();
            let open = Facts {
                closed: false,
                ..facts()
            };
            assert_eq!(
                validate(&open, proof).unwrap_err().code,
                "assignment-not-revoked"
            );
            let granted = Facts {
                lease_count: 1,
                ..facts()
            };
            assert_eq!(
                validate(&granted, proof).unwrap_err().code,
                "durable-launch-granted"
            );
            let sent = Facts {
                admission: Admission::Unknown,
                ..facts()
            };
            assert_eq!(
                validate(&sent, proof).unwrap_err().code,
                "durable-launch-granted"
            );
            let begun = Facts {
                retirement_begun: true,
                ..facts()
            };
            assert_eq!(
                validate(&begun, proof).unwrap_err().code,
                "durable-retirement-begun"
            );
        }
        let started = Facts {
            launch_started: true,
            ..facts()
        };
        assert_eq!(
            validate(&started, Proof::NeverLaunched).unwrap_err().code,
            "durable-launch-started"
        );
        validate(&started, Proof::NeverOwned).unwrap();
        validate(&started, Proof::UngrantedQuiescence).unwrap();
    }

    #[test]
    fn proof_discharges_only_launch_resources() {
        let launch = Launch {
            assignment_id: "a".into(),
            operation_id: "o".into(),
            attempt_id: "t".into(),
            profile_revision: 1,
            runtime_id: "r".into(),
            incarnation_id: "i".into(),
            conversation_id: "c".into(),
            store_id: "s".into(),
            forge_revision: "f".into(),
            component_hash: "h".into(),
            runtime_schema: 1,
        };
        assert!(reconciles_resource(&launch, "durable-runtime", "r"));
        assert!(reconciles_resource(&launch, "provision-intent", "once"));
        assert!(reconciles_resource(
            &launch,
            "durable-launch-failure",
            Failure::Spawn.code()
        ));
        assert!(!reconciles_resource(&launch, "durable-runtime", "other"));
        assert!(!reconciles_resource(&launch, "interrupted-effect", "x"));
        assert!(!reconciles_resource(&launch, "recovery-claim", "x"));
        assert!(!reconciles_resource(&launch, "durable-launch-failure", "x"));
    }
}
