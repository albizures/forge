//! Purge journal eligibility, idempotence and retained audit. Filesystem deletion is
//! tested in the host; this checks Forge's authorization and immutable history.
use super::supervised_retirement_tests::{operation, revoke};
use super::*;
use forge_app::durable_execution::{
    purge::{Finish, Removal, Repository as _, State},
    retirement::{Receipt, Repository as _},
};

const REMOVED: Finish = Finish::Purged {
    removal: Removal::Removed,
};

fn retire(f: &Fixture) {
    revoke(f);
    let op = operation(f);
    f.store.begin_retirement("test", &op).unwrap();
    f.store
        .reconcile_retirement(
            "test",
            &Receipt {
                version: 1,
                operation: op,
                children_absent: true,
            },
        )
        .unwrap();
}

fn audit(f: &Fixture) -> Vec<String> {
    let c = f.store.connection.lock().unwrap();
    [
        "agent_assignments",
        "durable_assignment_runtimes",
        "durable_assignment_retirements",
        "agent_assignment_resources",
        "record_attempts",
    ]
    .iter()
    .map(|table| {
        let mut statement = c
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1, 2"))
            .unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |r| {
                Ok((0..columns)
                    .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                    .collect::<Vec<_>>()
                    .join("|"))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
            .join("\n")
    })
    .collect()
}

#[test]
fn ineligible_assignments_are_rejected_without_a_journal() {
    let f = Fixture::new();
    accept_runtime(&f);
    let code = |f: &Fixture| {
        f.store
            .begin_purge("test", &f.launch, "purge")
            .unwrap_err()
            .code
    };
    assert_eq!(code(&f), "runtime-purge-assignment-open");
    revoke(&f);
    // Closed but retirement is neither reconciled nor are resources retired.
    assert_eq!(code(&f), "runtime-purge-uncertainty-retained");
    assert!(
        f.store
            .purge_record("test", "assignment")
            .unwrap()
            .is_none()
    );
    assert!(f.store.begin_purge("foreign", &f.launch, "purge").is_err());
}

#[test]
fn retained_uncertainty_and_holds_block_even_after_reconciliation() {
    for seed in [
        "INSERT INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) VALUES ('assignment', 'unknown-thing', 'x', 1)",
        "INSERT INTO dispatch_holds(record_id, reason) SELECT record_id, 'held' FROM agent_assignments WHERE id = 'assignment'",
    ] {
        let f = Fixture::new();
        accept_runtime(&f);
        retire(&f);
        f.store
            .connection
            .lock()
            .unwrap()
            .execute(seed, [])
            .unwrap();
        assert_eq!(
            f.store
                .begin_purge("test", &f.launch, "purge")
                .unwrap_err()
                .code,
            "runtime-purge-uncertainty-retained"
        );
        assert!(
            f.store
                .purge_record("test", "assignment")
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn journal_is_idempotent_conflict_checked_final_and_leaves_audit_intact() {
    let f = Fixture::new();
    accept_runtime(&f);
    retire(&f);
    let before = audit(&f);
    let pending = f.store.begin_purge("test", &f.launch, "purge").unwrap();
    assert_eq!(pending.state, State::Pending);
    assert_eq!(
        f.store.begin_purge("test", &f.launch, "purge").unwrap(),
        pending
    );
    assert_eq!(
        f.store
            .begin_purge("test", &f.launch, "other")
            .unwrap_err()
            .code,
        "runtime-purge-conflict"
    );
    assert_eq!(
        f.store
            .finish_purge("test", &f.launch, "other", &REMOVED)
            .unwrap_err()
            .code,
        "runtime-purge-intent-missing"
    );
    let retained = f
        .store
        .finish_purge(
            "test",
            &f.launch,
            "purge",
            &Finish::Retained {
                code: "runtime-purge-owner-active".into(),
            },
        )
        .unwrap();
    assert_eq!(retained.state, State::Retained);
    assert_eq!(
        retained.diagnostic.as_deref(),
        Some("runtime-purge-owner-active")
    );
    assert_eq!(
        f.reopen()
            .purge_record("test", "assignment")
            .unwrap()
            .unwrap(),
        retained
    );
    // Retry clears the diagnostic and resumes; completion is final.
    let again = f.store.begin_purge("test", &f.launch, "purge").unwrap();
    assert_eq!((again.state, again.diagnostic), (State::Pending, None));
    let done = f
        .store
        .finish_purge("test", &f.launch, "purge", &REMOVED)
        .unwrap();
    assert_eq!(done.state, State::Purged);
    assert_eq!(done.removal, Some(Removal::Removed));
    assert!(done.completed_at_millis.is_some());
    assert_eq!(
        f.reopen().begin_purge("test", &f.launch, "purge").unwrap(),
        done
    );
    assert_eq!(
        f.store
            .finish_purge(
                "test",
                &f.launch,
                "purge",
                &Finish::Retained { code: "x".into() }
            )
            .unwrap(),
        done
    );
    {
        let c = f.store.connection.lock().unwrap();
        assert!(c.execute("DELETE FROM durable_runtime_purges", []).is_err());
        assert!(
            c.execute(
                "UPDATE durable_runtime_purges SET state = 'pending', completed_at_millis = NULL",
                []
            )
            .is_err()
        );
        assert!(
            c.execute("UPDATE durable_runtime_purges SET operation_id = 'x'", [])
                .is_err()
        );
    }
    assert_eq!(
        audit(&f),
        before,
        "Forge audit/history is untouched by purge"
    );
}

#[test]
fn already_absent_is_distinguishable_and_revision_23_completion_reads_unrecorded() {
    let f = Fixture::new();
    accept_runtime(&f);
    retire(&f);
    f.store.begin_purge("test", &f.launch, "purge").unwrap();
    {
        // A removal can be recorded only for a completed purge.
        let c = f.store.connection.lock().unwrap();
        assert!(
            c.execute(
                "INSERT INTO durable_runtime_purge_removals VALUES ('assignment', 'removed')",
                []
            )
            .is_err()
        );
    }
    let done = f
        .store
        .finish_purge(
            "test",
            &f.launch,
            "purge",
            &Finish::Purged {
                removal: Removal::AlreadyAbsent,
            },
        )
        .unwrap();
    // A later different filesystem result never rewrites the recorded removal.
    assert_eq!(
        f.store
            .finish_purge(
                "test",
                &f.launch,
                "purge",
                &Finish::Purged {
                    removal: Removal::Removed,
                },
            )
            .unwrap(),
        done
    );
    {
        let c = f.store.connection.lock().unwrap();
        assert!(
            c.execute("DELETE FROM durable_runtime_purge_removals", [])
                .is_err()
        );
        assert!(
            c.execute(
                "UPDATE durable_runtime_purge_removals SET removal = 'removed'",
                []
            )
            .is_err()
        );
    }
    assert_eq!(
        (done.state, done.removal),
        (State::Purged, Some(Removal::AlreadyAbsent))
    );
    assert_eq!(
        f.reopen()
            .purge_record("test", "assignment")
            .unwrap()
            .unwrap(),
        done
    );
    // A purge completed before revision 24 kept no removal row: it reads as
    // unrecorded, never as a fabricated deletion.
    let legacy = Fixture::new();
    accept_runtime(&legacy);
    retire(&legacy);
    legacy
        .store
        .begin_purge("test", &legacy.launch, "purge")
        .unwrap();
    legacy
        .store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE durable_runtime_purges SET state = 'purged', completed_at_millis = 1",
            [],
        )
        .unwrap();
    let record = legacy
        .reopen()
        .purge_record("test", "assignment")
        .unwrap()
        .unwrap();
    assert_eq!((record.state, record.removal), (State::Purged, None));
}
