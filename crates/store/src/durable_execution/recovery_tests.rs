//! Persistence-policy fixtures, not supervised execution or writable recovery.
use super::*;
use forge_app::{
    authorization_lease::Repository as _,
    durable_execution::recovery::{Candidate, Repository as _},
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

fn candidate(f: &Fixture) -> Result<Candidate, RegistrationError> {
    f.reopen().recovery_candidate("test", &f.launch)
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
