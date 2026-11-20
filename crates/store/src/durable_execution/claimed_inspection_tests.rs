//! Live claimed reads are not idempotent historical claim replay.
use super::*;

#[test]
fn claimed_candidate_requires_active_exact_reservation_and_preserves_every_table() {
    let f = prepared();
    let original = candidate(&f).unwrap();
    let claim = claim(&f);
    let before = snapshot(&f);
    assert!(f.store.claimed_recovery_candidate("test", &claim).is_err());
    assert_eq!(snapshot(&f), before);
    f.store.claim_recovery("test", &claim).unwrap();
    let before = snapshot(&f);
    for _ in 0..3 {
        assert_eq!(
            f.reopen()
                .claimed_recovery_candidate("test", &claim)
                .unwrap(),
            original
        );
        assert!(
            f.store
                .claimed_recovery_candidate("foreign", &claim)
                .is_err()
        );
        for field in ["operation_id", "daemon_epoch", "candidate_sha256"] {
            let mut value = serde_json::to_value(&claim).unwrap();
            value[field] = serde_json::json!(if field == "candidate_sha256" {
                "f".repeat(64)
            } else {
                "foreign".into()
            });
            let foreign = serde_json::from_value(value).unwrap();
            assert!(
                f.store
                    .claimed_recovery_candidate("test", &foreign)
                    .is_err()
            );
        }
        assert_eq!(snapshot(&f), before);
    }
}

#[test]
fn claimed_candidate_rejects_mutable_drift_even_when_claim_replay_still_succeeds() {
    for sql in [
        "UPDATE agent_assignments SET closed_at_millis = 9, closure_reason = 'retired'",
        "UPDATE record_attempts SET closed = 1",
        "UPDATE records SET execution_mode = 'manual'",
        "UPDATE record_attempts SET attempt_json = json_set(attempt_json, '$.body', 'changed context')",
        "UPDATE worktree_claims SET state = 'cleanup-required'",
        "UPDATE assignment_execution_bindings SET directory = '/changed'",
        "UPDATE agent_assignment_resources SET retired_at_millis = 9 WHERE resource_kind = 'recovery-claim'",
        "DELETE FROM agent_assignment_resources WHERE resource_kind = 'recovery-claim'",
        "INSERT INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) VALUES ('assignment', 'interrupted-effect', 'uncertain', 9)",
        "INSERT INTO dispatch_holds SELECT record_id, 'blocked' FROM agent_assignments WHERE id = 'assignment'",
    ] {
        let f = prepared();
        let claim = claim(&f);
        f.store.claim_recovery("test", &claim).unwrap();
        f.store
            .connection
            .lock()
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        let before = snapshot(&f);
        assert!(
            f.reopen()
                .claimed_recovery_candidate("test", &claim)
                .is_err(),
            "{sql}"
        );
        assert_eq!(snapshot(&f), before, "{sql}");
        if !sql.starts_with("DELETE") {
            assert_eq!(f.store.claim_recovery("test", &claim).unwrap(), claim);
            assert_eq!(snapshot(&f), before);
        }
    }
}

#[test]
fn claimed_candidate_rejects_duplicate_or_malformed_claim_history_without_repair() {
    for sql in [
        "UPDATE agent_assignment_resources SET resource_id = '{}' WHERE resource_kind = 'recovery-claim'",
        "INSERT INTO agent_assignment_resources SELECT assignment_id, resource_kind, resource_id || ' ', created_at_millis, retired_at_millis FROM agent_assignment_resources WHERE resource_kind = 'recovery-claim'",
    ] {
        let f = prepared();
        let claim = claim(&f);
        f.store.claim_recovery("test", &claim).unwrap();
        f.store
            .connection
            .lock()
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        let before = snapshot(&f);
        assert!(
            f.reopen()
                .claimed_recovery_candidate("test", &claim)
                .is_err()
        );
        assert_eq!(snapshot(&f), before);
    }
}
