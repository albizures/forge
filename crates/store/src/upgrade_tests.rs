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
    c.execute_batch(
        "DROP TABLE builds_on_dependencies; DROP TABLE forced_completions; DROP TABLE task_unchanged_results; DROP TABLE ordering_only_dependencies; DROP TABLE task_stacks; PRAGMA user_version=7",
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
fn unresolved_execution_resources_block_snapshot_and_migration() {
    let repo = SqliteProjectRepository::in_memory().unwrap();
    let mut c = repo.connection.lock().unwrap();
    // Active leases are retained, not reset as an incidental migration effect.
    c.execute_batch("DROP TABLE builds_on_dependencies; DROP TABLE forced_completions; DROP TABLE task_unchanged_results; DROP TABLE ordering_only_dependencies; DROP TABLE task_stacks; PRAGMA user_version=7; INSERT INTO agent_dispatch_configuration(singleton, scan_lease_owner, scan_lease_until_millis) VALUES(1,'old-daemon',9223372036854775807)").unwrap();
    let before = inspection(&c).unwrap();
    assert!(
        before
            .blockers
            .contains(&"active dispatch scan lease".into())
    );
    assert!(migrate_connection(&mut c).is_err());
    assert_eq!(inspection(&c).unwrap(), before);
}
