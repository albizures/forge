//! Persistence-policy fixtures, not supervised execution or writable recovery.
use super::*;
use forge_app::{
    authorization_lease::Repository as _,
    durable_execution::recovery::{Candidate, Claim, Repository as _},
};
use std::fmt::Write as _;

fn prepared() -> Fixture {
    let f = prepared_without_grant();
    f.store.grant_lease("test", &lease(&f), 1_000).unwrap();
    f
}

fn prepared_without_grant() -> Fixture {
    let f = Fixture::new();
    accept_runtime(&f);
    let c = f.store.connection.lock().unwrap();
    c.execute(
        "INSERT INTO assignment_execution_bindings VALUES ('assignment', '/retained/worktree')",
        [],
    )
    .unwrap();
    c.execute("INSERT INTO worktree_claims SELECT record_id, project_slug, '/retained/worktree', 'active' FROM agent_assignments WHERE id = 'assignment'", []).unwrap();
    drop(c);
    f
}

fn claim(f: &Fixture) -> Claim {
    Claim::for_candidate(
        &candidate(f).unwrap(),
        "recover-original".into(),
        "restarted-daemon".into(),
    )
    .unwrap()
}

fn candidate(f: &Fixture) -> Result<Candidate, RegistrationError> {
    f.reopen().recovery_candidate("test", &f.launch)
}

fn candidate_body(f: &Fixture) -> String {
    let c = f.store.connection.lock().unwrap();
    crate::task_attempts::binding(&c, "assignment")
        .unwrap()
        .unwrap()
        .body
}

fn snapshot(f: &Fixture) -> String {
    // A logical snapshot includes every table: even a seemingly unrelated
    // counter/history/resource change would violate read-only preflight.
    let c = f.store.connection.lock().unwrap();
    let mut out = String::new();
    let names = c
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for name in names {
        out.push_str(&name);
        let mut stmt = c
            .prepare(&format!("SELECT * FROM \"{}\"", name.replace('"', "\"\"")))
            .unwrap();
        let count = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        let mut values = Vec::new();
        while let Some(row) = rows.next().unwrap() {
            values.push(
                (0..count)
                    .map(|i| format!("{:?}", row.get_ref(i).unwrap()))
                    .collect::<Vec<_>>(),
            );
        }
        values.sort();
        write!(out, "{values:?}").unwrap();
    }
    out
}

#[test]
fn recovery_preflight_preserves_original_attempt_admission_and_grant_across_restart() {
    let f = prepared();
    let before = snapshot(&f);
    let first = candidate(&f).unwrap();
    assert_eq!(first.launch, f.launch);
    assert_eq!(first.admission_id, "admitted");
    assert_eq!(first.attempt.id, f.launch.attempt_id);
    assert_eq!(first.execution_directory, "/retained/worktree");
    assert_eq!(first.last_lease, lease(&f));
    for _ in 0..3 {
        assert_eq!(candidate(&f).unwrap(), first);
        assert_eq!(snapshot(&f), before);
    }
    // Preflight never authorizes a replacement owner or revives an old deadline.
    let mut replacement = first.last_lease.clone();
    replacement.daemon_epoch = "restarted-daemon".into();
    replacement.owner.worker_pid += 1;
    replacement.issued_boottime_millis = 40_000;
    replacement.expires_boottime_millis = 70_000;
    assert!(
        f.reopen()
            .grant_lease("test", &replacement, 40_000)
            .is_err()
    );
    assert_eq!(snapshot(&f), before);
}

#[test]
fn recovery_preflight_revalidates_scope_and_every_immutable_launch_field() {
    let f = prepared();
    let before = snapshot(&f);
    assert!(f.reopen().recovery_candidate("foreign", &f.launch).is_err());
    let original = serde_json::to_value(&f.launch).unwrap();
    for key in original.as_object().unwrap().keys() {
        let mut altered = original.clone();
        altered[key] = match &original[key] {
            serde_json::Value::String(s) => serde_json::Value::String(format!("{s}-foreign")),
            serde_json::Value::Number(n) => serde_json::json!(n.as_u64().unwrap() + 1),
            _ => unreachable!(),
        };
        let changed: Launch = serde_json::from_value(altered).unwrap();
        assert!(
            f.reopen().recovery_candidate("test", &changed).is_err(),
            "{key}"
        );
    }
    assert_eq!(snapshot(&f), before);
}

#[test]
fn recovery_preflight_rejects_mutable_authority_and_binding_drift_without_writes() {
    for sql in [
        "UPDATE agent_assignments SET closed_at_millis = 1, closure_reason = 'retired'",
        "UPDATE agent_assignments SET state = 'unknown'",
        "UPDATE agent_assignments SET state = 'blocked'",
        "UPDATE record_attempts SET closed = 1",
        "UPDATE records SET status = 'done'",
        "UPDATE records SET execution_mode = 'manual'",
        "UPDATE record_attempts SET attempt_json = json_set(attempt_json, '$.actor', 'foreign')",
        "INSERT INTO record_work(record_id, generation) SELECT record_id, 2 FROM agent_assignments WHERE id = 'assignment' ON CONFLICT(record_id) DO UPDATE SET generation = 2",
        "INSERT INTO dispatch_holds SELECT record_id, 'investigate' FROM agent_assignments",
        "UPDATE worktree_claims SET state = 'cleanup-required'",
        "UPDATE worktree_claims SET path = '/drifted/worktree'",
        "DELETE FROM assignment_execution_bindings",
        "UPDATE durable_assignment_runtimes SET admission_json = '{\"state\":\"unknown\"}'",
        "UPDATE durable_assignment_runtimes SET admission_json = '{\"state\":\"pending\"}'",
        "UPDATE durable_assignment_runtimes SET admission_json = '{\"state\":\"rejected\",\"code\":\"denied\"}'",
        "UPDATE durable_assignment_runtimes SET admission_json = '{\"state\":\"accepted\",\"admission_id\":\"\"}'",
    ] {
        let f = prepared();
        f.store.connection.lock().unwrap().execute(sql, []).unwrap();
        let before = snapshot(&f);
        assert!(candidate(&f).is_err(), "{sql}");
        assert_eq!(snapshot(&f), before, "{sql}");
    }
}

#[test]
fn effect_missing_outcome_authorization_and_unknown_resources_block_even_without_hold() {
    for kind in [
        "interrupted-effect",
        "missing-outcome",
        "authorization-uncertain",
        "retirement-pending",
        "unknown",
    ] {
        let f = prepared();
        f.store.connection.lock().unwrap().execute(
            "INSERT INTO agent_assignment_resources VALUES ('assignment', ?1, 'retained', 1, NULL)", [kind],
        ).unwrap();
        let before = snapshot(&f);
        assert!(candidate(&f).is_err(), "{kind}");
        assert_eq!(snapshot(&f), before);
    }
}

#[test]
fn closed_predecessor_with_unreconciled_resources_blocks_candidate() {
    let f = prepared();
    let c = f.store.connection.lock().unwrap();
    c.execute("INSERT INTO agent_assignments(id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, closed_at_millis, closure_reason) SELECT 'predecessor', record_id, project_slug, profile_record_type, profile_revision, 0, 1, 2, 'retired' FROM agent_assignments WHERE id = 'assignment'", []).unwrap();
    c.execute("INSERT INTO agent_assignment_resources VALUES ('predecessor', 'pane', 'previous-pane', 1, NULL)", []).unwrap();
    drop(c);
    let before = snapshot(&f);
    assert!(candidate(&f).is_err());
    assert_eq!(snapshot(&f), before);
    f.store.connection.lock().unwrap().execute("UPDATE agent_assignment_resources SET retired_at_millis = 3 WHERE assignment_id = 'predecessor'", []).unwrap();
    assert!(candidate(&f).is_ok());
}

#[test]
fn missing_runtime_and_original_grant_are_not_reconstructed_by_preflight() {
    for f in [Fixture::new(), prepared_without_grant()] {
        let before = snapshot(&f);
        assert!(candidate(&f).is_err());
        assert_eq!(snapshot(&f), before);
    }
}

#[test]
fn unfinished_dependency_blocks_recovery_without_changing_evidence() {
    let f = prepared();
    let dependency = f
        .store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "prerequisite".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    f.store.connection.lock().unwrap().execute(
        "INSERT INTO record_relationships(from_id, to_id, kind) SELECT record_id, ?1, 'depends-on' FROM agent_assignments WHERE id = 'assignment'",
        [dependency.id.cast_signed()],
    ).unwrap();
    let before = snapshot(&f);
    assert!(candidate(&f).is_err());
    assert_eq!(snapshot(&f), before);
}

#[test]
fn latest_retained_grant_is_evidence_and_live_states_do_not_create_new_attempts() {
    let f = prepared();
    let mut renewed = lease(&f);
    renewed.sequence = 2;
    renewed.issued_boottime_millis = 11_000;
    renewed.expires_boottime_millis = 41_000;
    f.store.grant_lease("test", &renewed, 11_000).unwrap();
    for state in ["provisioning", "working", "idle"] {
        f.store
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE agent_assignments SET state = ?1", [state])
            .unwrap();
        let before = snapshot(&f);
        assert_eq!(candidate(&f).unwrap().last_lease, renewed);
        assert_eq!(snapshot(&f), before);
    }
}

#[test]
fn concurrent_candidate_reads_are_not_claims_or_lease_history_forks() {
    let f = prepared();
    let before = snapshot(&f);
    let a = f.reopen();
    let b = f.reopen();
    let barrier = Barrier::new(2);
    let run = |store: &SqliteProjectRepository| {
        barrier.wait();
        store.recovery_candidate("test", &f.launch).unwrap()
    };
    std::thread::scope(|s| {
        let first = s.spawn(|| run(&a));
        let second = s.spawn(|| run(&b));
        assert_eq!(first.join().unwrap(), second.join().unwrap());
    });
    assert_eq!(snapshot(&f), before);
}

#[test]
fn recovery_claim_replays_original_fence_after_restart_and_revocation() {
    let f = prepared();
    let request = claim(&f);
    assert_eq!(f.store.claim_recovery("test", &request).unwrap(), request);
    let accepted = snapshot(&f);
    assert_eq!(
        f.reopen().claim_recovery("test", &request).unwrap(),
        request
    );
    assert_eq!(snapshot(&f), accepted);
    f.store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE agent_assignments SET closed_at_millis = 9, closure_reason = 'retired'",
            [],
        )
        .unwrap();
    let closed = snapshot(&f);
    assert!(
        f.reopen()
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    assert_eq!(
        f.reopen().claim_recovery("test", &request).unwrap(),
        request
    );
    assert_eq!(snapshot(&f), closed);
    assert!(f.reopen().claim_recovery("foreign", &request).is_err());
    let mut conflict = request.clone();
    conflict.daemon_epoch.push_str("-another");
    assert!(f.reopen().claim_recovery("test", &conflict).is_err());
    assert_eq!(snapshot(&f), closed);
}

#[test]
fn recovery_claim_is_not_a_lease_and_preserves_all_other_tables() {
    let f = prepared();
    let request = claim(&f);
    let before = snapshot(&f);
    f.store.claim_recovery("test", &request).unwrap();
    let accepted = snapshot(&f);
    assert!(candidate(&f).is_err());
    let mut renewal = lease(&f);
    renewal.sequence = 2;
    renewal.issued_boottime_millis = 11_000;
    renewal.expires_boottime_millis = 41_000;
    assert!(f.reopen().grant_lease("test", &renewal, 11_000).is_err());
    let id = serde_json::to_string(&request).unwrap();
    assert!(!id.contains(&candidate_body(&f)));
    assert!(!id.contains("/retained/worktree"));
    assert!(
        f.store
            .retire_agent_assignment_resource("assignment", "recovery-claim", &id)
            .is_err()
    );
    assert!(
        f.store
            .add_agent_assignment_resource(&forge_app::NewAgentAssignmentResource {
                assignment_id: "assignment".into(),
                resource_kind: "recovery-claim".into(),
                resource_id: "forged".into(),
            })
            .is_err()
    );
    assert_eq!(snapshot(&f), accepted);
    // Test-only deletion proves no attempt, counter, grant, hold, evidence or
    // runtime admission changed. Production has no generic release operation.
    f.store
        .connection
        .lock()
        .unwrap()
        .execute(
            "DELETE FROM agent_assignment_resources WHERE resource_kind = 'recovery-claim'",
            [],
        )
        .unwrap();
    assert_eq!(snapshot(&f), before);
}

#[test]
fn recovery_claim_revalidates_candidate_and_mutable_gates_before_commit() {
    for sql in [
        "UPDATE agent_assignments SET closed_at_millis = 9, closure_reason = 'retired'",
        "UPDATE record_attempts SET closed = 1",
        "UPDATE records SET execution_mode = 'manual'",
        "INSERT INTO dispatch_holds SELECT record_id, 'investigate' FROM agent_assignments",
        "UPDATE worktree_claims SET state = 'cleanup-required'",
        "UPDATE assignment_execution_bindings SET directory = '/changed'",
        "UPDATE record_attempts SET attempt_json = json_set(attempt_json, '$.body', 'changed bound context')",
        "INSERT INTO agent_assignment_resources VALUES ('assignment', 'interrupted-effect', 'unknown', 1, NULL)",
    ] {
        let f = prepared();
        let request = claim(&f);
        f.store.connection.lock().unwrap().execute(sql, []).unwrap();
        let before = snapshot(&f);
        assert!(
            f.reopen().claim_recovery("test", &request).is_err(),
            "{sql}"
        );
        assert_eq!(snapshot(&f), before);
    }
    let f = prepared();
    let request = claim(&f);
    let mut renewal = lease(&f);
    renewal.sequence = 2;
    renewal.issued_boottime_millis = 11_000;
    renewal.expires_boottime_millis = 41_000;
    f.store.grant_lease("test", &renewal, 11_000).unwrap();
    let before = snapshot(&f);
    assert!(f.reopen().claim_recovery("test", &request).is_err());
    assert_eq!(snapshot(&f), before);
}

#[test]
fn recovery_claim_rejects_changed_candidate_fields_and_invalid_identities() {
    let f = prepared();
    let original = claim(&f);
    let before = snapshot(&f);
    let mut changed = original.clone();
    changed.candidate_sha256 = "0".repeat(64);
    assert!(f.store.claim_recovery("test", &changed).is_err());
    changed = original.clone();
    changed.launch.operation_id.push_str("-foreign");
    assert!(f.store.claim_recovery("test", &changed).is_err());
    for value in [
        String::new(),
        " ".into(),
        "bad\nidentity".into(),
        "x".repeat(201),
    ] {
        changed = original.clone();
        changed.operation_id.clone_from(&value);
        assert!(f.store.claim_recovery("test", &changed).is_err());
        changed = original.clone();
        changed.daemon_epoch = value;
        assert!(f.store.claim_recovery("test", &changed).is_err());
    }
    assert_eq!(snapshot(&f), before);
    // Every original candidate field is content-bound, not just its Launch.
    let baseline = candidate(&f).unwrap();
    let mut altered = baseline.clone();
    altered.attempt.body.push_str(" changed");
    changed = Claim::for_candidate(
        &altered,
        original.operation_id.clone(),
        original.daemon_epoch.clone(),
    )
    .unwrap();
    assert!(f.store.claim_recovery("test", &changed).is_err());
    altered = baseline;
    altered.execution_directory.push_str("/other");
    changed = Claim::for_candidate(&altered, original.operation_id, original.daemon_epoch).unwrap();
    assert!(f.store.claim_recovery("test", &changed).is_err());
    assert_eq!(snapshot(&f), before);
}

#[test]
fn concurrent_recovery_claims_have_one_winner_without_implicit_takeover() {
    for exact_retry in [false, true] {
        let f = prepared();
        let first = claim(&f);
        let mut second = first.clone();
        if !exact_retry {
            second.operation_id.push_str("-other");
            second.daemon_epoch.push_str("-other");
        }
        let a = f.reopen();
        let b = f.reopen();
        let barrier = Barrier::new(2);
        std::thread::scope(|s| {
            let one = s.spawn(|| {
                barrier.wait();
                a.claim_recovery("test", &first)
            });
            let two = s.spawn(|| {
                barrier.wait();
                b.claim_recovery("test", &second)
            });
            let results = [one.join().unwrap(), two.join().unwrap()];
            assert_eq!(
                results.iter().filter(|r| r.is_ok()).count(),
                if exact_retry { 2 } else { 1 }
            );
        });
        let c = f.store.connection.lock().unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM agent_assignment_resources WHERE resource_kind = 'recovery-claim'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    }
}

#[test]
fn failed_recovery_claim_insert_rolls_back_and_retry_can_accept() {
    let f = prepared();
    let request = claim(&f);
    f.store.connection.lock().unwrap().execute_batch(
        "CREATE TRIGGER fail_claim BEFORE INSERT ON agent_assignment_resources WHEN NEW.resource_kind = 'recovery-claim' BEGIN SELECT RAISE(ABORT, 'injected'); END;",
    ).unwrap();
    let before = snapshot(&f);
    assert!(f.store.claim_recovery("test", &request).is_err());
    assert_eq!(snapshot(&f), before);
    f.store
        .connection
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_claim")
        .unwrap();
    assert_eq!(
        f.reopen().claim_recovery("test", &request).unwrap(),
        request
    );
}
