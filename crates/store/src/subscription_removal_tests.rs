#[test]
#[allow(clippy::too_many_lines)] // One upgrade scenario covers both historical audit schemas.
fn removing_subscriptions_preserves_records_assignments_and_unrelated_audits() {
    for acknowledge in [false, true] {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.register(&project("alpha", "/repos/alpha")).unwrap();
        let record = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "retained task".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let assignment = store
            .create_agent_assignment(&NewAgentAssignment {
                id: "retained-assignment".into(),
                record_id: record.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        store
            .add_record_comment(record.id, "retained comment", None)
            .unwrap();
        let updates = store.list_record_updates(record.id).unwrap();
        let comments = store.list_record_comments(record.id).unwrap();
        let head = store.record_update_head().unwrap();
        let connection = std::sync::Arc::try_unwrap(store.connection)
            .unwrap()
            .into_inner()
            .unwrap();
        connection.execute_batch(
            "CREATE TABLE record_update_subscriptions (
                id INTEGER PRIMARY KEY, assignment_id TEXT REFERENCES agent_assignments(id),
                target_key TEXT, filter_key TEXT, start_cursor INTEGER, lease_expires_at_millis INTEGER,
                UNIQUE (assignment_id, target_key, filter_key));
             CREATE TABLE record_update_subscription_targets (
                subscription_id INTEGER REFERENCES record_update_subscriptions(id) ON DELETE CASCADE,
                record_id INTEGER REFERENCES records(id), PRIMARY KEY (subscription_id, record_id));
             CREATE TABLE record_update_subscription_filters (
                subscription_id INTEGER REFERENCES record_update_subscriptions(id) ON DELETE CASCADE,
                kind TEXT, PRIMARY KEY (subscription_id, kind));
             CREATE INDEX record_update_subscriptions_expiry ON record_update_subscriptions(lease_expires_at_millis);
             CREATE INDEX record_updates_global_replay ON record_updates(id);
             INSERT INTO record_update_subscriptions VALUES (1, 'retained-assignment', '', '', 0, 9999999999999);
             INSERT INTO record_update_subscription_filters VALUES (1, 'body-edited');"
        ).unwrap();
        connection
            .execute(
                "INSERT INTO record_update_subscription_targets VALUES (1, ?1)",
                [record.id.cast_signed()],
            )
            .unwrap();
        let schema: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'assignment_authorization_audits'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let schema = schema.replace(
            "'start-task'",
            "'start-task', 'create-subscription', 'renew-subscription', 'delete-subscription'",
        );
        let schema = if acknowledge {
            schema
        } else {
            schema.replace(", 'acknowledge'", "")
        };
        connection
            .execute_batch("DROP TABLE assignment_authorization_audits")
            .unwrap();
        connection.execute_batch(&schema).unwrap();
        for operation in [
            "read-context",
            "create-subscription",
            "renew-subscription",
            "delete-subscription",
        ] {
            connection.execute(
                "INSERT INTO assignment_authorization_audits (assignment_id, capability_id, created_at_millis, operation, target_record_id, outcome, reason, correlation_id)
                 VALUES ('retained-assignment', 'audit-identity', 1, ?1, ?2, 'allowed', 'retained reason', 'retained correlation')",
                rusqlite::params![operation, record.id.cast_signed()],
            ).unwrap();
        }
        // Run the real open-time migration twice to cover both upgrade and idempotency.
        let store = Repository::from_connection(connection).unwrap();
        let connection = std::sync::Arc::try_unwrap(store.connection)
            .unwrap()
            .into_inner()
            .unwrap();
        let store = Repository::from_connection(connection)
            .unwrap()
            .in_record_workspace("test")
            .unwrap();
        assert_eq!(store.show_record(record.id).unwrap(), record);
        assert_eq!(store.agent_assignment(&assignment.id).unwrap(), assignment);
        assert_eq!(store.list_record_updates(record.id).unwrap(), updates);
        assert_eq!(store.list_record_comments(record.id).unwrap(), comments);
        assert_eq!(store.record_update_head().unwrap(), head);
        let audits = store
            .list_assignment_authorization_audits(&assignment.id)
            .unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(
            audits[0].operation,
            AssignmentAuthorizationOperation::ReadContext
        );
        assert_eq!(audits[0].correlation_id, "retained correlation");
        {
            let connection = store.connection.lock().unwrap();
            let remaining: u32 = connection.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'record_update_subscription%' OR name = 'record_updates_global_replay'",
                [], |row| row.get(0),
            ).unwrap();
            assert_eq!(remaining, 0);
            let violations: u32 = connection
                .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(violations, 0);
        }
        store
            .close_agent_assignment(
                &assignment.id,
                forge_app::AgentAssignmentClosureReason::Retired,
            )
            .unwrap();
    }
}

#[test]
fn new_databases_have_no_subscription_schema_and_retain_update_freshness() {
    let store = SqliteProjectRepository::in_memory().unwrap();
    assert_eq!(store.record_update_head().unwrap().0, 0);
    let connection = store.connection.lock().unwrap();
    let remaining: u32 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'record_update_subscription%' OR name = 'record_updates_global_replay'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(remaining, 0);
}
