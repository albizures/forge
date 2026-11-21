use super::*;
use forge_app::durable_execution::{
    completion::Repository as _,
    followup::{Repository as _, Request},
};

fn request(op: &str, message: &str) -> Request {
    Request {
        assignment_id: "assignment".into(),
        operation_id: op.into(),
        message: message.into(),
    }
}
fn code<T: std::fmt::Debug>(r: Result<T, RegistrationError>) -> String {
    r.unwrap_err().code.to_string()
}
fn count(f: &Fixture) -> i64 {
    f.store
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM durable_assignment_followups",
            [],
            |r| r.get(0),
        )
        .unwrap()
}
fn sql(f: &Fixture, statement: &str) {
    f.store
        .connection
        .lock()
        .unwrap()
        .execute(statement, [])
        .unwrap();
}

#[test]
fn retry_after_dropped_reply_records_one_follow_up_and_survives_restart() {
    let f = Fixture::new();
    accept_runtime(&f);
    let first = f
        .store
        .record_followup("test", &request("op-1", "please recheck"))
        .unwrap();
    assert!(!first.replayed);
    assert_eq!(first.sequence, 1);
    let retry = f
        .reopen()
        .record_followup("test", &request("op-1", "please recheck"))
        .unwrap();
    assert!(retry.replayed);
    assert_eq!(
        (retry.sequence, retry.recorded_at_millis, &retry.binding),
        (1, first.recorded_at_millis, &first.binding)
    );
    assert_eq!(count(&f), 1);
    let second = f
        .store
        .record_followup("test", &request("op-2", "and again"))
        .unwrap();
    assert_eq!(second.sequence, 2);
    let listed = f.reopen().followups("test", "assignment").unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|r| r.operation_id.as_str())
            .collect::<Vec<_>>(),
        ["op-1", "op-2"]
    );
}

#[test]
fn conflicting_reuse_is_rejected_without_changing_the_record() {
    let f = Fixture::new();
    accept_runtime(&f);
    let first = f
        .store
        .record_followup("test", &request("op-1", "a"))
        .unwrap();
    assert_eq!(
        code(f.store.record_followup("test", &request("op-1", "b"))),
        "followup-operation-conflict"
    );
    assert_eq!(
        f.store.followups("test", "assignment").unwrap(),
        vec![first]
    );
}

#[test]
fn rebinding_request_keeps_original_check_profile_attempt_and_generation() {
    let f = Fixture::new();
    accept_runtime(&f);
    let receipt = f
        .store
        .record_followup("test", &request("op-1", "use another commit"))
        .unwrap();
    assert_eq!(receipt.binding.attempt_id, "attempt");
    assert_eq!(receipt.binding.profile_revision, 2);
    assert_eq!(receipt.binding.generation, 1);
    let c = f.store.connection.lock().unwrap();
    let attempt: String = c
        .query_row(
            "SELECT attempt_json FROM record_attempts WHERE id = 'attempt'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        attempt.contains(&"a".repeat(40)),
        "original evidence commit is unchanged"
    );
    // A structured rebind is not even representable.
    assert!(
        serde_json::from_str::<Request>(
            r#"{"assignment_id":"assignment","operation_id":"o","message":"m","commit":"b"}"#
        )
        .is_err()
    );
}

#[test]
fn revoked_stale_unadmitted_and_foreign_targets_reject_but_retries_replay() {
    let f = Fixture::new();
    assert_eq!(
        code(f.store.record_followup("test", &request("op-0", "m"))),
        "durable-intent-missing"
    );
    f.store.reserve("test", &f.launch).unwrap();
    assert_eq!(
        code(f.store.record_followup("test", &request("op-0", "m"))),
        "followup-admission-unconfirmed"
    );
    accept_runtime(&f);
    assert_eq!(
        code(f.store.record_followup("other", &request("op-0", "m"))),
        "durable-scope-denied"
    );
    f.store
        .record_followup("test", &request("op-1", "m"))
        .unwrap();
    sql(
        &f,
        "INSERT OR REPLACE INTO record_work(record_id, generation) SELECT record_id, 2 FROM agent_assignments WHERE id = 'assignment'",
    );
    assert_eq!(
        code(f.store.record_followup("test", &request("op-2", "m"))),
        "durable-binding-stale"
    );
    sql(
        &f,
        "UPDATE record_work SET generation = 1 WHERE record_id = (SELECT record_id FROM agent_assignments WHERE id = 'assignment')",
    );
    sql(
        &f,
        "UPDATE agent_assignments SET closed_at_millis = 1, closure_reason = 'retired' WHERE id = 'assignment'",
    );
    assert_eq!(
        code(f.store.record_followup("test", &request("op-2", "m"))),
        "followup-assignment-revoked"
    );
    // Revocation between authorization and a lost reply: the original is still discoverable.
    assert!(
        f.store
            .record_followup("test", &request("op-1", "m"))
            .unwrap()
            .replayed
    );
    assert_eq!(count(&f), 1);
}

#[test]
fn unresolved_effect_or_authorization_uncertainty_blocks_but_missing_outcome_allows() {
    let f = Fixture::new();
    accept_runtime(&f);
    assert_eq!(
        f.store.observe_answer("test", &f.launch).unwrap(),
        forge_app::durable_execution::completion::AnswerObservation::MissingOutcome
    );
    let ok = f
        .store
        .record_followup(
            "test",
            &request("intervene", "state the result via the Outcome tool"),
        )
        .unwrap();
    assert_eq!(ok.sequence, 1);
    // Intervention records input only: no Outcome, completion, or retired diagnostic.
    let c = f.store.connection.lock().unwrap();
    let (done, retired): (bool, bool) = c.query_row(
        "SELECT (SELECT closed OR outcome_json IS NOT NULL FROM record_attempts WHERE id = 'attempt'), (SELECT retired_at_millis IS NOT NULL FROM agent_assignment_resources WHERE resource_kind = 'missing-outcome')", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert!(!done && !retired);
    drop(c);
    for kind in [
        "interrupted-effect",
        "authorization-uncertain",
        "recovery-claim",
        "retirement-pending",
        "provision-intent",
    ] {
        sql(
            &f,
            &format!(
                "INSERT INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) VALUES ('assignment', '{kind}', 'x', 1)"
            ),
        );
        assert_eq!(
            code(
                f.store
                    .record_followup("test", &request(&format!("blocked-{kind}"), "m"))
            ),
            "followup-blocked-by-uncertainty",
            "{kind}"
        );
        sql(
            &f,
            &format!(
                "UPDATE agent_assignment_resources SET retired_at_millis = 2 WHERE resource_kind = '{kind}'"
            ),
        );
    }
    f.store
        .record_followup("test", &request("after", "m"))
        .unwrap();
    assert_eq!(count(&f), 2);
}

#[test]
fn follow_up_history_is_immutable_and_retained() {
    let f = Fixture::new();
    accept_runtime(&f);
    f.store
        .record_followup("test", &request("op-1", "m"))
        .unwrap();
    let c = f.store.connection.lock().unwrap();
    assert!(
        c.execute("UPDATE durable_assignment_followups SET message = 'x'", [])
            .is_err()
    );
    assert!(
        c.execute("DELETE FROM durable_assignment_followups", [])
            .is_err()
    );
}

#[test]
fn concurrent_duplicate_retries_create_one_record() {
    let f = std::sync::Arc::new(Fixture::new());
    accept_runtime(&f);
    let barrier = std::sync::Arc::new(Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let (f, b) = (f.clone(), barrier.clone());
            std::thread::spawn(move || {
                let repo = f.reopen();
                b.wait();
                repo.record_followup("test", &request("same", "m"))
                    .unwrap()
                    .replayed
            })
        })
        .collect();
    let replays = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|r| *r)
        .count();
    assert_eq!(replays, 3);
    assert_eq!(count(&f), 1);
}

#[test]
fn delivery_state_is_persisted_final_and_never_downgraded_or_fabricated() {
    use forge_app::durable_execution::followup::Delivery;
    let f = Fixture::new();
    accept_runtime(&f);
    f.store
        .record_followup("test", &request("op-1", "one"))
        .unwrap();
    f.store
        .record_followup("test", &request("op-2", "two"))
        .unwrap();
    let unknown = Delivery::Unknown;
    f.store
        .mark_followup("test", "assignment", "op-1", &unknown)
        .unwrap();
    let pending = f.store.undelivered_followups("test", "assignment").unwrap();
    assert_eq!(pending.len(), 2, "unknown is retried, never delivered");
    assert_eq!(
        f.store.followups("test", "assignment").unwrap()[0].delivery,
        "unknown"
    );
    let done = Delivery::Delivered {
        admission_id: "7".into(),
    };
    f.store
        .mark_followup("test", "assignment", "op-1", &done)
        .unwrap();
    f.store
        .mark_followup("test", "assignment", "op-1", &done)
        .unwrap();
    f.store
        .mark_followup("test", "assignment", "op-1", &unknown)
        .unwrap();
    let rows = f.reopen().followups("test", "assignment").unwrap();
    assert_eq!(
        (rows[0].delivery.as_str(), rows[0].admission_id.as_deref()),
        ("delivered", Some("7"))
    );
    assert_eq!(rows[1].delivery, "recorded");
    assert!(
        f.store
            .mark_followup(
                "test",
                "assignment",
                "op-1",
                &Delivery::Delivered {
                    admission_id: "8".into()
                }
            )
            .is_err()
    );
    assert_eq!(
        code(
            f.store
                .mark_followup("test", "assignment", "nope", &unknown)
        ),
        "followup-not-recorded"
    );
    assert_eq!(
        f.store
            .undelivered_followups("test", "assignment")
            .unwrap()
            .len(),
        1
    );
}
