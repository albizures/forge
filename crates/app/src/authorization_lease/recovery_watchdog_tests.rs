use super::*;
use crate::durable_execution::recovery::{Claim, authorization::Grant};

fn grant() -> Grant {
    let mut lease: Lease = serde_json::from_str(include_str!(
        "../../../../runner/test/authorization-lease.json"
    ))
    .unwrap();
    lease.sequence = 7;
    lease.daemon_epoch = "recovery-epoch".into();
    lease.owner.host_pid += 100;
    lease.owner.worker_pid += 100;
    Grant {
        claim: Claim {
            launch: lease.launch.clone(),
            operation_id: "recover-original".into(),
            daemon_epoch: lease.daemon_epoch.clone(),
            candidate_sha256: "a".repeat(64),
        },
        lease,
        evidence_sha256: "b".repeat(64),
    }
}
fn waiting() -> LeaseWatchdog {
    let expected = grant();
    LeaseWatchdog::for_recovery(expected.lease.owner.clone(), expected).unwrap()
}
fn renewal() -> Lease {
    let mut lease = grant().lease;
    lease.sequence += 1;
    lease.issued_boottime_millis = 11_000;
    lease.expires_boottime_millis = 41_000;
    lease
}

#[test]
fn recovery_watchdog_arms_only_exact_envelope_then_renews_without_resetting_sequence() {
    let mut watchdog = waiting();
    watchdog.check(1_000).unwrap();
    assert!(watchdog.last_lease().is_none());
    watchdog.accept_recovery(&grant(), 1_000).unwrap();
    watchdog.accept_recovery(&grant(), 2_000).unwrap();
    assert_eq!(watchdog.last_lease(), Some(&grant().lease));
    watchdog.accept(renewal(), 11_000).unwrap();
    watchdog.accept(renewal(), 12_000).unwrap();
    watchdog.check(40_999).unwrap();
    assert!(watchdog.check(41_000).is_err());
    assert!(watchdog.accept(renewal(), 12_000).is_err());
    assert_eq!(watchdog.last_lease(), Some(&renewal()));
}

#[test]
fn recovery_watchdog_ordinary_delivery_and_restart_cannot_adopt_recovery_permission() {
    for lease in [grant().lease, {
        let mut lease = grant().lease;
        lease.sequence = 1;
        lease
    }] {
        let mut watchdog = waiting();
        assert!(watchdog.accept(lease, 1_000).is_err());
        assert!(watchdog.accept_recovery(&grant(), 1_000).is_err());
        assert!(watchdog.last_lease().is_none());
    }
    let mut ordinary = LeaseWatchdog::new(grant().lease.owner);
    assert!(ordinary.accept_recovery(&grant(), 1_000).is_err());
    let mut restarted = LeaseWatchdog::new(grant().lease.owner);
    assert!(restarted.accept(grant().lease, 1_000).is_err());
}

#[test]
fn recovery_watchdog_mismatched_envelope_terminally_fences_without_arming() {
    let changes: &[fn(&mut Grant)] = &[
        |g| g.claim.operation_id.push('x'),
        |g| g.claim.daemon_epoch.push('x'),
        |g| g.claim.candidate_sha256 = "c".repeat(64),
        |g| g.claim.launch.attempt_id.push('x'),
        |g| g.evidence_sha256 = "c".repeat(64),
        |g| g.lease.sequence += 1,
        |g| g.lease.owner.worker_start.push('x'),
        |g| g.lease.generation += 1,
        |g| g.lease.launch.runtime_id.push('x'),
        |g| g.lease.expires_boottime_millis += 1,
    ];
    for change in changes {
        let mut watchdog = waiting();
        let mut wrong = grant();
        change(&mut wrong);
        assert!(watchdog.accept_recovery(&wrong, 1_000).is_err());
        assert!(watchdog.accept_recovery(&grant(), 1_000).is_err());
        assert!(watchdog.last_lease().is_none());
    }
}

#[test]
fn recovery_watchdog_invalid_expectation_owner_binding_and_overflow_reject() {
    let changes: &[fn(&mut Grant)] = &[
        |g| g.claim.operation_id.clear(),
        |g| g.claim.candidate_sha256 = "A".repeat(64),
        |g| g.claim.daemon_epoch.push('x'),
        |g| g.claim.launch.assignment_id.push('x'),
        |g| g.evidence_sha256.clear(),
        |g| g.lease.sequence = 1,
        |g| g.lease.generation = 0,
        |g| g.lease.owner.worker_pid = 0,
        |g| g.lease.owner.worker_start.clear(),
        |g| g.lease.expires_boottime_millis += 1,
        |g| g.lease.issued_boottime_millis = u64::MAX,
    ];
    for change in changes {
        let mut expected = grant();
        change(&mut expected);
        assert!(LeaseWatchdog::for_recovery(expected.lease.owner.clone(), expected).is_err());
    }
    let mut foreign = grant().lease.owner;
    foreign.worker_start.push('x');
    assert!(LeaseWatchdog::for_recovery(foreign, grant()).is_err());
}

#[test]
fn recovery_watchdog_expiry_backward_clock_fence_and_superseded_retry_are_terminal() {
    for fault in [
        "early",
        "late-first",
        "expiry",
        "rollback",
        "fenced",
        "superseded",
    ] {
        let mut watchdog = waiting();
        if !["early", "late-first"].contains(&fault) {
            watchdog.accept_recovery(&grant(), 1_000).unwrap();
        }
        let now = match fault {
            "early" | "rollback" => 999,
            "late-first" | "expiry" => 31_000,
            "superseded" => {
                watchdog.accept(renewal(), 11_000).unwrap();
                12_000
            }
            _ => {
                watchdog.fence();
                2_000
            }
        };
        assert!(watchdog.accept_recovery(&grant(), now).is_err(), "{fault}");
        assert!(watchdog.accept_recovery(&grant(), 2_000).is_err());
        assert!(watchdog.accept(renewal(), 12_000).is_err());
    }
}
