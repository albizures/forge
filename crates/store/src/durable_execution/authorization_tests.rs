//! Fake verified-host evidence exercises application/store policy only. This
//! does not demonstrate process cleanup, writable reopen or grant delivery.
use super::*;
use forge_app::{
    NewAgentAssignmentResource,
    authorization_lease::{LeaseWatchdog, Observation, Owner},
    durable_execution::recovery::authorization::{
        Prepared, RecoveryHost, Repository as _, Verification,
    },
};

#[path = "recovery_session_tests.rs"]
mod session_tests;

struct Host {
    owner: Owner,
    now: u64,
    calls: usize,
    unavailable: bool,
    evidence_sha256: String,
}
impl RecoveryHost for Host {
    fn verify(&mut self, _: &Candidate, _: &Claim) -> Result<Verification, RegistrationError> {
        self.calls += 1;
        if self.unavailable {
            return Err(RegistrationError::new(
                "fake-host-unverified",
                "no safety proof",
            ));
        }
        Ok(Verification {
            observation: Observation {
                owner: self.owner.clone(),
                boottime_millis: self.now,
            },
            evidence_sha256: self.evidence_sha256.clone(),
        })
    }
}
fn host(candidate: &Candidate) -> Host {
    let mut owner = candidate.last_lease.owner.clone();
    owner.host_pid += 100;
    owner.worker_pid += 100;
    owner.host_start.push_str("-new");
    owner.worker_start.push_str("-new");
    Host {
        owner,
        now: 40_000,
        calls: 0,
        unavailable: false,
        evidence_sha256: "a".repeat(64),
    }
}
fn reserved() -> (Fixture, Candidate, Claim) {
    let f = prepared();
    f.store.connection.lock().unwrap().execute(
        "UPDATE record_attempts SET attempt_json = json_set(attempt_json, '$.body', 'private original Task context for recovery')", [],
    ).unwrap();
    let original = candidate(&f).unwrap();
    let claim =
        Claim::for_candidate(&original, "recover-original".into(), "new-daemon".into()).unwrap();
    f.store.claim_recovery("test", &claim).unwrap();
    (f, original, claim)
}
fn proposal(candidate: &Candidate, claim: &Claim) -> Prepared {
    Prepared::verify(candidate.clone(), claim.clone(), &mut host(candidate)).unwrap()
}

#[test]
fn recovery_commits_fresh_owner_lease_audit_and_claim_consumption_without_new_attempt() {
    let (f, original, claim) = reserved();
    let prepared = proposal(&original, &claim);
    let expected = prepared.grant().clone();
    let unaffected = [
        "assignment_authorization_leases",
        "agent_assignment_resources",
    ];
    let before = snapshot_except(&f, &unaffected);
    assert_eq!(expected.lease.sequence, 2);
    assert_eq!(expected.lease.issued_boottime_millis, 40_000);
    assert_eq!(expected.lease.expires_boottime_millis, 70_000);
    assert_eq!(
        f.store.grant_recovery("test", &prepared, 40_000).unwrap(),
        expected
    );
    assert_eq!(snapshot_except(&f, &unaffected), before);
    let mut current = candidate(&f).unwrap();
    assert_eq!(current.last_lease, expected.lease);
    current.last_lease = original.last_lease.clone();
    assert_eq!(current, original);
    let info = f.reopen().agent_assignment_info("assignment").unwrap();
    assert!(info.capacity_reserved());
    let fence = info
        .resources
        .iter()
        .find(|r| r.resource_kind == "recovery-claim")
        .unwrap();
    let audit = info
        .resources
        .iter()
        .find(|r| r.resource_kind == "recovery-grant")
        .unwrap();
    assert_eq!(fence.retired_at_millis, audit.retired_at_millis);
    assert_eq!(audit.created_at_millis, audit.retired_at_millis.unwrap());
    assert!(fence.retired_at_millis.is_some());
    assert_eq!(
        serde_json::from_str::<forge_app::durable_execution::recovery::authorization::Grant>(
            &audit.resource_id
        )
        .unwrap(),
        expected
    );
    assert!(!audit.resource_id.contains(&original.attempt.body));
    assert!(!audit.resource_id.contains(&original.execution_directory));
    let accepted = snapshot(&f);
    for now in [40_000, 40_001, 69_999] {
        assert_eq!(
            f.reopen().grant_recovery("test", &prepared, now).unwrap(),
            expected
        );
        assert_eq!(f.reopen().claim_recovery("test", &claim).unwrap(), claim);
        assert_eq!(snapshot(&f), accepted);
    }
    // An ordinary fresh consumer cannot adopt this persisted sequence. Recovery
    // delivery still needs its own verified handshake, not a watchdog reset.
    let mut watchdog = LeaseWatchdog::new(expected.lease.owner.clone());
    assert!(watchdog.accept(expected.lease, 40_000).is_err());
}

#[test]
fn external_verification_is_required_and_bad_commitments_do_not_call_the_host() {
    let (f, original, claim) = reserved();
    let before = snapshot(&f);
    let mut unavailable = host(&original);
    unavailable.unavailable = true;
    assert!(Prepared::verify(original.clone(), claim.clone(), &mut unavailable).is_err());
    assert_eq!(unavailable.calls, 1);
    let mut wrong = claim.clone();
    wrong.candidate_sha256 = "0".repeat(64);
    let mut observed = host(&original);
    assert!(Prepared::verify(original.clone(), wrong, &mut observed).is_err());
    assert_eq!(observed.calls, 0);
    assert_eq!(snapshot(&f), before);
}

#[test]
fn recovery_rejects_reused_owners_epochs_malformed_predecessors_and_clock_overflow() {
    let (f, original, claim) = reserved();
    let before = snapshot(&f);
    for fault in 0..10 {
        let mut observed = host(&original);
        let mut changed = original.clone();
        let mut requested = claim.clone();
        match fault {
            0 => observed.owner = original.last_lease.owner.clone(),
            1 => {
                observed.owner.host_pid = original.last_lease.owner.host_pid;
                observed.owner.host_start = original.last_lease.owner.host_start.clone();
            }
            2 => {
                observed.owner.worker_pid = original.last_lease.owner.worker_pid;
                observed.owner.worker_start = original.last_lease.owner.worker_start.clone();
            }
            3 => {
                requested = Claim::for_candidate(
                    &original,
                    claim.operation_id.clone(),
                    original.last_lease.daemon_epoch.clone(),
                )
                .unwrap();
            }
            4 => observed.now = u64::MAX,
            5 => changed.last_lease.sequence = 0,
            6 => changed.last_lease.expires_boottime_millis += 1,
            7 => changed.last_lease.sequence = u32::MAX,
            8 => observed.evidence_sha256 = "no-proof".into(),
            9 => observed.now = original.last_lease.issued_boottime_millis - 1,
            _ => unreachable!(),
        }
        if fault >= 5 {
            requested = Claim::for_candidate(
                &changed,
                claim.operation_id.clone(),
                claim.daemon_epoch.clone(),
            )
            .unwrap();
        }
        assert!(
            Prepared::verify(changed, requested, &mut observed).is_err(),
            "{fault}"
        );
    }
    assert_eq!(snapshot(&f), before);
    let mut rebooted = host(&original);
    rebooted.owner.boot_id.push_str("-new-boot");
    rebooted.now = 100;
    let proposed = Prepared::verify(original, claim, &mut rebooted).unwrap();
    assert_eq!(proposed.grant().lease.issued_boottime_millis, 100);
    // This tests clock-coordinate policy, not proof that a reboot occurred.
}

#[test]
fn recovery_revalidates_mutable_authority_context_and_all_resource_blockers() {
    for sql in [
        "UPDATE agent_assignments SET closed_at_millis = 1, closure_reason = 'retired'",
        "UPDATE agent_assignments SET state = 'unknown'",
        "UPDATE record_attempts SET closed = 1",
        "UPDATE records SET execution_mode = 'manual'",
        "UPDATE records SET status = 'done'",
        "INSERT INTO dispatch_holds SELECT record_id, 'investigate' FROM agent_assignments",
        "INSERT INTO record_work(record_id, generation) SELECT record_id, 2 FROM agent_assignments WHERE id = 'assignment' ON CONFLICT(record_id) DO UPDATE SET generation = 2",
        "UPDATE record_attempts SET attempt_json = json_set(attempt_json, '$.body', 'changed context')",
        "UPDATE assignment_execution_bindings SET directory = '/foreign/worktree'",
        "UPDATE worktree_claims SET state = 'cleanup-required'",
        "INSERT INTO assignment_authorization_leases SELECT assignment_id, 99, lease_json FROM assignment_authorization_leases WHERE sequence = 1",
        "UPDATE durable_assignment_runtimes SET admission_json = '{\"state\":\"accepted\",\"admission_id\":\"different\"}'",
        "INSERT INTO agent_assignment_resources VALUES ('assignment', 'interrupted-effect', 'effect', 1, NULL)",
        "INSERT INTO agent_assignment_resources VALUES ('assignment', 'authorization-uncertain', 'stopped', 1, NULL)",
        "INSERT INTO agent_assignment_resources VALUES ('assignment', 'missing-outcome', 'attempt', 1, NULL)",
        "INSERT INTO agent_assignment_resources VALUES ('assignment', 'unknown', 'identity', 1, NULL)",
        "UPDATE agent_assignment_resources SET retired_at_millis = 2 WHERE resource_kind = 'recovery-claim'",
        "DELETE FROM agent_assignment_resources WHERE resource_kind = 'recovery-claim'",
    ] {
        let (f, original, claim) = reserved();
        let prepared = proposal(&original, &claim);
        f.store.connection.lock().unwrap().execute(sql, []).unwrap();
        let before = snapshot(&f);
        assert!(
            f.reopen()
                .grant_recovery("test", &prepared, 40_000)
                .is_err(),
            "{sql}"
        );
        assert_eq!(snapshot(&f), before, "{sql}");
    }
}

#[test]
fn recovery_scope_conflicting_claim_and_freshness_deadlines_cannot_change_state() {
    let (f, original, claim) = reserved();
    let prepared = proposal(&original, &claim);
    let before = snapshot(&f);
    assert!(
        f.store
            .grant_recovery("foreign", &prepared, 40_000)
            .is_err()
    );
    for now in [39_999, 70_000, 70_001] {
        assert!(f.store.grant_recovery("test", &prepared, now).is_err());
        assert_eq!(snapshot(&f), before);
    }
    let mut changed = claim.clone();
    changed.operation_id.push_str("-another");
    let conflicting = proposal(&original, &changed);
    assert!(
        f.store
            .grant_recovery("test", &conflicting, 40_000)
            .is_err()
    );
    assert_eq!(snapshot(&f), before);
    f.store.grant_recovery("test", &prepared, 40_000).unwrap();
    let accepted = snapshot(&f);
    let mut different_proof = host(&original);
    different_proof.evidence_sha256 = "b".repeat(64);
    let conflict = Prepared::verify(original.clone(), claim.clone(), &mut different_proof).unwrap();
    assert!(
        f.reopen()
            .grant_recovery("test", &conflict, 40_001)
            .is_err()
    );
    assert_eq!(snapshot(&f), accepted);
    assert!(
        f.reopen()
            .grant_recovery("test", &prepared, 70_000)
            .is_err()
    );
    assert_eq!(snapshot(&f), accepted);
    f.store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE agent_assignments SET closed_at_millis = 2, closure_reason = 'retired'",
            [],
        )
        .unwrap();
    let closed = snapshot(&f);
    assert!(
        f.reopen()
            .grant_recovery("test", &prepared, 40_001)
            .is_err()
    );
    assert_eq!(snapshot(&f), closed);
}

#[test]
fn accepted_recovery_audit_cannot_bypass_later_live_gates_or_corrupt_history() {
    for sql in [
        "INSERT INTO dispatch_holds SELECT record_id, 'investigate' FROM agent_assignments",
        "UPDATE record_attempts SET attempt_json = json_set(attempt_json, '$.body', 'changed after grant')",
        "UPDATE worktree_claims SET state = 'cleanup-required'",
        "INSERT INTO agent_assignment_resources VALUES ('assignment', 'interrupted-effect', 'late-effect', 1, NULL)",
        "UPDATE agent_assignment_resources SET resource_id = '{}' WHERE resource_kind = 'recovery-grant'",
        "DELETE FROM agent_assignment_resources WHERE resource_kind = 'recovery-grant'",
        "UPDATE agent_assignment_resources SET retired_at_millis = NULL WHERE resource_kind = 'recovery-grant'",
    ] {
        let (f, original, claim) = reserved();
        let prepared = proposal(&original, &claim);
        f.store.grant_recovery("test", &prepared, 40_000).unwrap();
        f.store.connection.lock().unwrap().execute(sql, []).unwrap();
        let before = snapshot(&f);
        assert!(
            f.reopen()
                .grant_recovery("test", &prepared, 40_001)
                .is_err(),
            "{sql}"
        );
        assert_eq!(snapshot(&f), before, "{sql}");
    }
}

#[test]
fn recovery_continues_global_sequence_after_predecessor_renewals() {
    let f = prepared();
    let mut renewal = lease(&f);
    renewal.sequence = 2;
    renewal.issued_boottime_millis = 11_000;
    renewal.expires_boottime_millis = 41_000;
    f.store.grant_lease("test", &renewal, 11_000).unwrap();
    let original = candidate(&f).unwrap();
    let claim =
        Claim::for_candidate(&original, "recover-renewed".into(), "new-epoch".into()).unwrap();
    f.store.claim_recovery("test", &claim).unwrap();
    let prepared = proposal(&original, &claim);
    let result = f.store.grant_recovery("test", &prepared, 40_000).unwrap();
    assert_eq!(result.lease.sequence, 3);
    assert_eq!(candidate(&f).unwrap().last_lease, result.lease);
    assert_eq!(
        f.reopen()
            .grant_recovery("test", &prepared, 40_001)
            .unwrap(),
        result
    );
}

#[test]
fn recovery_commit_failures_roll_back_lease_audit_and_claim_retirement_together() {
    for trigger in [
        "CREATE TRIGGER fail_recovery BEFORE INSERT ON assignment_authorization_leases BEGIN SELECT RAISE(ABORT, 'injected'); END;",
        "CREATE TRIGGER fail_recovery BEFORE INSERT ON agent_assignment_resources WHEN NEW.resource_kind = 'recovery-grant' BEGIN SELECT RAISE(ABORT, 'injected'); END;",
        "CREATE TRIGGER fail_recovery BEFORE UPDATE OF retired_at_millis ON agent_assignment_resources WHEN OLD.resource_kind = 'recovery-claim' BEGIN SELECT RAISE(ABORT, 'injected'); END;",
    ] {
        let (f, original, claim) = reserved();
        let prepared = proposal(&original, &claim);
        f.store
            .connection
            .lock()
            .unwrap()
            .execute_batch(trigger)
            .unwrap();
        let before = snapshot(&f);
        assert!(
            f.reopen()
                .grant_recovery("test", &prepared, 40_000)
                .is_err()
        );
        assert_eq!(snapshot(&f), before);
        f.store
            .connection
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_recovery")
            .unwrap();
        assert_eq!(
            f.reopen()
                .grant_recovery("test", &prepared, 40_000)
                .unwrap(),
            *prepared.grant()
        );
    }
}

#[test]
fn concurrent_recovery_grants_accept_only_one_exact_owner_deadline() {
    for conflict in [false, true] {
        let (f, original, claim) = reserved();
        let first = proposal(&original, &claim);
        let mut observed = host(&original);
        if conflict {
            observed.now += 1;
            observed.owner.worker_start.push_str("-other");
        }
        let second = Prepared::verify(original, claim, &mut observed).unwrap();
        let a = f.reopen();
        let b = f.reopen();
        let barrier = Barrier::new(2);
        std::thread::scope(|s| {
            let one = s.spawn(|| {
                barrier.wait();
                a.grant_recovery("test", &first, 40_001)
            });
            let two = s.spawn(|| {
                barrier.wait();
                b.grant_recovery("test", &second, 40_001)
            });
            let results = [one.join().unwrap(), two.join().unwrap()];
            assert_eq!(
                results.iter().filter(|r| r.is_ok()).count(),
                if conflict { 1 } else { 2 }
            );
        });
        let c = f.store.connection.lock().unwrap();
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM assignment_authorization_leases",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
        assert_eq!(c.query_row("SELECT count(*) FROM agent_assignment_resources WHERE resource_kind = 'recovery-grant'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    }
}

#[test]
fn recovery_audit_is_specialized_and_old_or_restarted_issuers_cannot_adopt_it() {
    let (f, original, claim) = reserved();
    let prepared = proposal(&original, &claim);
    let grant = f.store.grant_recovery("test", &prepared, 40_000).unwrap();
    let accepted = snapshot(&f);
    let id = serde_json::to_string(&grant).unwrap();
    assert!(
        f.store
            .add_agent_assignment_resource(&NewAgentAssignmentResource {
                assignment_id: "assignment".into(),
                resource_kind: "recovery-grant".into(),
                resource_id: "forged".into(),
            })
            .is_err()
    );
    assert!(
        f.store
            .retire_agent_assignment_resource("assignment", "recovery-grant", &id)
            .is_err()
    );
    let mut old = original.last_lease.clone();
    old.sequence = 3;
    old.issued_boottime_millis = 50_000;
    old.expires_boottime_millis = 80_000;
    assert!(f.store.grant_lease("test", &old, 50_000).is_err());
    let mut restarted = grant.lease.clone();
    restarted.sequence = 1;
    assert!(f.store.grant_lease("test", &restarted, 40_000).is_err());
    assert_eq!(snapshot(&f), accepted);
    // Ordinary renewals remain monotonic after a specialized initial recovery
    // commit, but no current controller/consumer yet performs this handshake.
    let mut renewal = grant.lease;
    renewal.sequence += 1;
    renewal.issued_boottime_millis += 10_000;
    renewal.expires_boottime_millis += 10_000;
    f.store.grant_lease("test", &renewal, 50_000).unwrap();
    let renewed = snapshot(&f);
    assert!(f.store.grant_recovery("test", &prepared, 50_000).is_err());
    assert_eq!(snapshot(&f), renewed);
}
