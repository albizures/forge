//! Receiver policy adapter tests; filesystem/real pipe composition is separate.
use super::*;
use forge_app::{
    durable_execution::recovery::{Candidate, Claim, authorization::Grant},
    outcomes::{Attempt, Evidence},
};
use forge_rpc::durable_execution::SupervisorRecoveryPreparation;

fn preparation() -> SupervisorRecoveryPreparation {
    let original: Lease = serde_json::from_str(include_str!(
        "../../../runner/test/authorization-lease.json"
    ))
    .unwrap();
    let candidate = Candidate {
        launch: original.launch.clone(),
        admission_id: "original".into(),
        attempt: Attempt {
            id: original.launch.attempt_id.clone(),
            record_id: 1,
            check_id: 1,
            generation: original.generation,
            evidence: Evidence::Repository {
                repository: "repository".into(),
                commit: "a".repeat(40),
            },
            actor: original.launch.assignment_id.clone(),
            body: "bound task".into(),
            continuation: None,
            recovery: None,
        },
        execution_directory: "/bound/worktree".into(),
        last_lease: original.clone(),
    };
    let claim = Claim::for_candidate(&candidate, "recover".into(), "new-epoch".into()).unwrap();
    let mut lease = original;
    lease.sequence += 1;
    lease.owner.host_pid += 100;
    lease.owner.worker_pid += 100;
    lease.daemon_epoch.clone_from(&claim.daemon_epoch);
    SupervisorRecoveryPreparation {
        host_prepare_recovery: 1,
        candidate,
        grant: Grant {
            claim,
            lease,
            evidence_sha256: "a".repeat(64),
        },
    }
}

// Rebind the entire expected envelope after changing history, so rejection
// cannot be attributed merely to a stale candidate commitment.
fn rebind(control: &mut SupervisorRecoveryPreparation) {
    let epoch = control.grant.claim.daemon_epoch.clone();
    control.grant.claim = Claim::for_candidate(
        &control.candidate,
        control.grant.claim.operation_id.clone(),
        epoch,
    )
    .unwrap();
    control.grant.lease.sequence = control.candidate.last_lease.sequence + 1;
}

#[test]
fn receiver_malformed_predecessor_terminally_fences_even_with_matching_commitment() {
    let changes: &[fn(&mut Lease)] = &[
        |l| l.sequence = 0,
        |l| l.daemon_epoch.clear(),
        |l| l.owner.host_pid = 0,
        |l| l.owner.worker_pid = 0,
        |l| l.owner.boot_id.clear(),
        |l| l.owner.time_namespace.clear(),
        |l| l.owner.host_start.clear(),
        |l| l.owner.worker_start.clear(),
        |l| l.owner.lock_device.clear(),
        |l| l.owner.lock_inode.clear(),
        |l| l.expires_boottime_millis += 1,
        |l| l.expires_boottime_millis = l.issued_boottime_millis,
    ];
    for change in changes {
        let good = preparation();
        let mut malformed = good.clone();
        change(&mut malformed.candidate.last_lease);
        rebind(&mut malformed);
        let mut state = State::new(good.grant.lease.owner.clone(), true);
        assert!(state.prepare_recovery(malformed, 1_000).is_err());
        assert!(state.policy.last_lease().is_none());
        assert!(state.prepare_recovery(good.clone(), 1_001).is_err());
        assert!(state.policy.accept_recovery(&good.grant, 1_002).is_err());
    }
}

#[test]
fn recovery_transition_and_fresh_preparation_reject_the_same_malformed_history() {
    use forge_app::{
        RegistrationError,
        durable_execution::recovery::authorization::{Prepared, RecoveryHost, Verification},
    };
    struct NoExternalVerification;
    impl RecoveryHost for NoExternalVerification {
        fn verify(&mut self, _: &Candidate, _: &Claim) -> Result<Verification, RegistrationError> {
            panic!("malformed history must be rejected before external verification");
        }
    }
    let changes: &[fn(&mut Lease)] = &[
        |l| l.sequence = 0,
        |l| l.daemon_epoch.clear(),
        |l| l.owner.host_pid = 0,
        |l| l.owner.worker_start.clear(),
        |l| l.expires_boottime_millis += 1,
    ];
    for change in changes {
        let mut control = preparation();
        change(&mut control.candidate.last_lease);
        rebind(&mut control);
        assert!(
            control
                .grant
                .validate_candidate(&control.candidate, 1_000)
                .is_err()
        );
        assert!(
            Prepared::verify(
                control.candidate,
                control.grant.claim,
                &mut NoExternalVerification,
            )
            .is_err()
        );
    }
}

#[test]
fn receiver_valid_expired_renewal_and_prior_boot_history_remain_recoverable() {
    for prior_boot in [false, true] {
        let mut control = preparation();
        control.candidate.last_lease.sequence = 6;
        if prior_boot {
            control.candidate.last_lease.owner.boot_id = "previous-boot".into();
            // Old and new boottime values have no ordering across a reboot.
            control.candidate.last_lease.issued_boottime_millis = 90_000;
            control.candidate.last_lease.expires_boottime_millis = 120_000;
        }
        control.grant.lease.issued_boottime_millis = 40_000;
        control.grant.lease.expires_boottime_millis = 70_000;
        rebind(&mut control);
        let mut state = State::new(control.grant.lease.owner.clone(), true);
        state.prepare_recovery(control.clone(), 40_000).unwrap();
        assert!(state.policy.last_lease().is_none());
        state
            .policy
            .accept_recovery(&control.grant, 40_001)
            .unwrap();
        assert_eq!(state.policy.last_lease(), Some(&control.grant.lease));
    }
}

#[test]
fn receiver_preparation_retries_remain_unarmed_and_cannot_reset_after_delivery() {
    let control = preparation();
    let mut state = State::new(control.grant.lease.owner.clone(), true);
    state.prepare_recovery(control.clone(), 1_000).unwrap();
    state.prepare_recovery(control.clone(), 1_001).unwrap();
    assert!(state.policy.last_lease().is_none());
    state.policy.accept_recovery(&control.grant, 1_002).unwrap();
    assert_eq!(state.policy.last_lease(), Some(&control.grant.lease));
    assert!(state.prepare_recovery(control.clone(), 1_003).is_err());
    assert!(state.policy.accept_recovery(&control.grant, 1_004).is_err());
    assert_eq!(state.policy.last_lease(), Some(&control.grant.lease));
}

#[test]
fn receiver_ordinary_renewals_require_exact_recovery_handshake_first() {
    let control = preparation();
    let mut unarmed = State::new(control.grant.lease.owner.clone(), true);
    unarmed.prepare_recovery(control.clone(), 1_000).unwrap();
    assert!(
        unarmed
            .accept_lease(
                SupervisorControl {
                    host_control: 1,
                    lease: control.grant.lease.clone(),
                },
                1_001
            )
            .is_err()
    );
    assert!(
        unarmed
            .policy
            .accept_recovery(&control.grant, 1_002)
            .is_err()
    );
    let mut active = State::new(control.grant.lease.owner.clone(), true);
    active.prepare_recovery(control.clone(), 1_000).unwrap();
    active
        .policy
        .accept_recovery(&control.grant, 1_001)
        .unwrap();
    let mut renewed = control.grant.lease.clone();
    renewed.sequence += 1;
    renewed.issued_boottime_millis += 10_000;
    renewed.expires_boottime_millis += 10_000;
    active
        .accept_lease(
            SupervisorControl {
                host_control: 1,
                lease: renewed.clone(),
            },
            11_000,
        )
        .unwrap();
    assert_eq!(active.policy.last_lease(), Some(&renewed));
    assert!(
        active
            .policy
            .accept_recovery(&control.grant, 11_001)
            .is_err()
    );
    assert!(active.prepare_recovery(control, 11_002).is_err());
    assert_eq!(active.policy.last_lease(), Some(&renewed));
}

#[test]
fn receiver_preparation_never_waives_fresh_fault_shutdown_clock_or_expiry_fences() {
    let control = preparation();
    for fault in 0..6 {
        let mut state = State::new(control.grant.lease.owner.clone(), fault != 0);
        match fault {
            0 | 5 => {}
            1 => state.fence("previous-host-fault"),
            2 => {
                state.policy.check(2_000).unwrap();
            }
            3 => {
                state.shutdown = Some(Shutdown::Quiescence(Box::new(
                    forge_app::durable_execution::recovery::quiescence::Operation::for_candidate(
                        &control.candidate,
                        control.grant.claim.clone(),
                    )
                    .unwrap(),
                )));
            }
            4 => state.owner.worker_start.push('x'),
            _ => unreachable!(),
        }
        let now = if fault == 5 { 31_000 } else { 1_000 };
        assert!(state.prepare_recovery(control.clone(), now).is_err());
        assert!(state.prepare_recovery(control.clone(), 2_001).is_err());
        assert!(state.policy.last_lease().is_none());
    }
}

#[test]
fn receiver_conflicting_preparation_retains_first_expectation_and_terminal_fence() {
    let first = preparation();
    for fault in 0..3 {
        let mut state = State::new(first.grant.lease.owner.clone(), true);
        state.prepare_recovery(first.clone(), 1_000).unwrap();
        let mut changed = first.clone();
        match fault {
            0 => changed.grant.evidence_sha256 = "c".repeat(64),
            1 => changed.candidate.attempt.body.push('x'),
            2 => changed.host_prepare_recovery = 2,
            _ => unreachable!(),
        }
        assert!(state.prepare_recovery(changed, 1_001).is_err());
        assert_eq!(state.recovery.as_ref(), Some(&first));
        assert!(state.prepare_recovery(first.clone(), 1_002).is_err());
        assert!(state.policy.accept_recovery(&first.grant, 1_002).is_err());
    }
}
