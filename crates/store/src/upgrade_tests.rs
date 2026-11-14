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
    c.execute_batch("DROP TABLE task_stacks; PRAGMA user_version=7")
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
fn unresolved_execution_resources_block_snapshot_and_migration() {
    let repo = SqliteProjectRepository::in_memory().unwrap();
    let mut c = repo.connection.lock().unwrap();
    // Active leases are retained, not reset as an incidental migration effect.
    c.execute_batch("DROP TABLE task_stacks; PRAGMA user_version=7; INSERT INTO agent_dispatch_configuration(singleton, scan_lease_owner, scan_lease_until_millis) VALUES(1,'old-daemon',9223372036854775807)").unwrap();
    let before = inspection(&c).unwrap();
    assert!(
        before
            .blockers
            .contains(&"active dispatch scan lease".into())
    );
    assert!(migrate_connection(&mut c).is_err());
    assert_eq!(inspection(&c).unwrap(), before);
}
