use super::*;
use crate::SqliteProjectRepository;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};

#[test]
fn frozen_contracts_and_advertised_target_match_ordinary_initialization() {
    let repo = SqliteProjectRepository::in_memory().unwrap();
    let c = repo.connection.lock().unwrap();
    let actual = inspection(&c).unwrap();
    assert_eq!(actual.schema, Some(capabilities().target_schema));
    assert!(actual.pristine);
}

#[test]
fn failed_schema_stamp_rolls_back_ddl_and_all_existing_data() {
    let repo = SqliteProjectRepository::in_memory().unwrap();
    let mut c = repo.connection.lock().unwrap();
    remove_durable_expansion(&c);
    c.execute_batch(
        "DROP TABLE retained_work_recoveries; DROP TABLE check_continuation_resets; DROP TABLE check_continuations; DROP TABLE assignment_sessions; DROP TABLE builds_on_dependencies; DROP TABLE forced_completions; DROP TABLE task_unchanged_results; DROP TABLE ordering_only_dependencies; DROP TABLE task_stacks; PRAGMA user_version=7",
    )
    .unwrap();
    let before = inspection(&c).unwrap();
    c.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some("8"),
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_connection(&mut c).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
    assert!(
        !c.prepare("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='task_stacks')")
            .unwrap()
            .query_row([], |r| r.get::<_, bool>(0))
            .unwrap()
    );
}

#[test]
fn failed_nine_stamp_rolls_back_designation_schema_without_rewriting_old_tables() {
    let mut c = reference(8).unwrap();
    c.execute_batch("PRAGMA application_id=1179603527; PRAGMA user_version=8;")
        .unwrap();
    let before = inspection(&c).unwrap();
    c.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some("9"),
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_connection(&mut c).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
}

#[test]
fn failed_ten_stamp_rolls_back_unchanged_schema_without_rewriting_history() {
    let mut c = reference(9).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    let before = inspection(&c).unwrap();
    c.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some("10"),
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_connection(&mut c).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
}

#[test]
fn failed_twelve_stamp_rolls_back_topology_schema_without_inventing_intent() {
    let mut c = reference(11).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    let before = inspection(&c).unwrap();
    c.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some("12"),
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_connection(&mut c).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
}

#[test]
fn failed_eleven_stamp_rolls_back_override_schema_without_rewriting_history() {
    let mut c = reference(10).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    let before = inspection(&c).unwrap();
    c.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some("11"),
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_connection(&mut c).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
}

#[test]
fn session_schema_expansion_preserves_history_and_stamp_failure_rolls_back() {
    let mut c = reference(13).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    let before = inspection(&c).unwrap();
    c.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some("14"),
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_connection(&mut c).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
    let after = migrate_connection(&mut c).unwrap();
    assert_eq!(after.schema, Some(23));
    for table in before.tables {
        assert!(after.tables.contains(&table));
    }
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "assignment_sessions")
            .unwrap()
            .rows,
        0
    );
    assert_eq!(migrate_connection(&mut c).unwrap(), after);
}

#[test]
fn continuation_reset_schema_stamp_failure_rolls_back_rebuild_and_audit_table() {
    let mut c = reference(15).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    let before = inspection(&c).unwrap();
    c.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some("16"),
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_connection(&mut c).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
    assert_eq!(migrate_connection(&mut c).unwrap().schema, Some(23));
}

#[test]
fn continuation_expansion_is_empty_and_stamp_failure_rolls_back() {
    let mut c = reference(14).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    let before = inspection(&c).unwrap();
    c.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some("15"),
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_connection(&mut c).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
    let after = migrate_connection(&mut c).unwrap();
    assert_eq!(after.schema, Some(23));
    for table in before.tables {
        assert!(after.tables.contains(&table));
    }
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "check_continuations")
            .unwrap()
            .rows,
        0
    );
    assert_eq!(migrate_connection(&mut c).unwrap(), after);
}

#[test]
fn dirty_recovery_semantic_stamp_failure_preserves_all_old_facts() {
    verify_semantic_stamp_rollback(17, "18");
}

#[test]
fn extended_index_flags_semantic_stamp_failure_preserves_all_old_facts() {
    verify_semantic_stamp_rollback(18, "19");
}

fn verify_semantic_stamp_rollback(source: u32, denied: &'static str) {
    let mut c = reference(source).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    let before = inspection(&c).unwrap();
    c.authorizer(Some(move |context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some(value),
        } if value == denied => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_connection(&mut c).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
    let after = migrate_connection(&mut c).unwrap();
    assert_eq!(after.schema, Some(23));
    for table in before.tables {
        assert!(after.tables.contains(&table));
    }
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "durable_assignment_runtimes")
            .unwrap()
            .rows,
        0
    );
}

#[test]
fn retained_work_schema_stamp_failure_rolls_back_all_authority_ddl() {
    let mut c = reference(16).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    let before = inspection(&c).unwrap();
    c.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some("17"),
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_connection(&mut c).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
    let after = migrate_connection(&mut c).unwrap();
    assert_eq!(after.schema, Some(23));
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "retained_work_recoveries")
            .unwrap()
            .rows,
        0
    );
}

#[test]
fn authorization_lease_expansion_is_empty_and_failed_stamp_rolls_back() {
    let mut c = reference(20).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    let before = inspection(&c).unwrap();
    c.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some("21"),
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_connection(&mut c).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
    let after = migrate_connection(&mut c).unwrap();
    assert_eq!(after.schema, Some(23));
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "assignment_authorization_leases")
            .unwrap()
            .rows,
        0
    );
    for table in before.tables {
        assert!(after.tables.contains(&table));
    }
    assert_eq!(migrate_connection(&mut c).unwrap(), after);
}

#[test]
fn durable_admission_expansion_is_empty_and_failed_stamp_rolls_back() {
    let mut c = reference(19).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    let before = inspection(&c).unwrap();
    c.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some("20"),
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_connection(&mut c).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
    let after = migrate_connection(&mut c).unwrap();
    assert_eq!(after.schema, Some(23));
    for table in before.tables {
        assert!(after.tables.contains(&table));
    }
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "durable_assignment_runtimes")
            .unwrap()
            .rows,
        0
    );
    assert_eq!(migrate_connection(&mut c).unwrap(), after);
}

#[test]
fn retirement_expansion_and_stamp_failure_preserve_schema_twenty_one() {
    verify_semantic_stamp_rollback(21, "22");
    let mut c = reference(21).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    let before = inspection(&c).unwrap();
    let after = migrate_connection(&mut c).unwrap();
    for table in before.tables {
        assert!(after.tables.contains(&table));
    }
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "durable_assignment_retirements")
            .unwrap()
            .rows,
        0
    );
}

#[test]
fn follow_up_expansion_is_empty_and_stamp_failure_preserves_schema_twenty_two() {
    verify_semantic_stamp_rollback(22, "23");
    let mut c = reference(22).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    let before = inspection(&c).unwrap();
    let after = migrate_connection(&mut c).unwrap();
    for table in before.tables {
        assert!(after.tables.contains(&table));
    }
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "durable_assignment_followups")
            .unwrap()
            .rows,
        0
    );
    // Historical Checks gain no fabricated migration history.
    for table in [
        "check_profile_migrations",
        "check_profile_migration_changes",
        "durable_runtime_purges",
    ] {
        assert_eq!(
            after.tables.iter().find(|t| t.name == table).unwrap().rows,
            0
        );
    }
}

#[test]
fn unresolved_execution_resources_block_snapshot_and_migration() {
    let repo = SqliteProjectRepository::in_memory().unwrap();
    let mut c = repo.connection.lock().unwrap();
    // Active leases are retained, not reset as an incidental migration effect.
    remove_durable_expansion(&c);
    c.execute_batch("DROP TABLE retained_work_recoveries; DROP TABLE check_continuation_resets; DROP TABLE check_continuations; DROP TABLE assignment_sessions; DROP TABLE builds_on_dependencies; DROP TABLE forced_completions; DROP TABLE task_unchanged_results; DROP TABLE ordering_only_dependencies; DROP TABLE task_stacks; PRAGMA user_version=7; INSERT INTO agent_dispatch_configuration(singleton, scan_lease_owner, scan_lease_until_millis) VALUES(1,'old-daemon',9223372036854775807)").unwrap();
    let before = inspection(&c).unwrap();
    assert!(
        before
            .blockers
            .contains(&"active dispatch scan lease".into())
    );
    assert!(migrate_connection(&mut c).is_err());
    assert_eq!(inspection(&c).unwrap(), before);
}
