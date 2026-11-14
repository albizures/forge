//! Schema-7's contract is schema-8 without `task_stacks`: no Task backfill is allowed.
use forge_app::{NewTypedRecord, RecordRepository, RecordType, WorkspaceRepository};
use forge_store::{SqliteProjectRepository, upgrade};
use rusqlite::Connection;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    source: PathBuf,
}
impl Fixture {
    fn new(schema: u32) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "forge-upgrade-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let source = root.join("source.db");
        let repo = SqliteProjectRepository::open(&source).unwrap();
        repo.create_workspace("personal", "Personal", &root.canonicalize().unwrap())
            .unwrap();
        let repo = repo.in_record_workspace("personal").unwrap();
        let task = repo
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "Keep my old Task".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        repo.add_record_comment(task.id, "Keep this comment", Some("operator"))
            .unwrap();
        drop(repo);
        let c = Connection::open(&source).unwrap();
        if schema == 7 {
            c.execute_batch("DROP TABLE task_stacks; PRAGMA user_version = 7;")
                .unwrap();
        }
        drop(c);
        Self { root, source }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn frozen_historical_schema_seven_fixture_upgrades_without_current_initializers() {
    let f = Fixture::new(7);
    let legacy = f.root.join("historical.db");
    let c = Connection::open(&legacy).unwrap();
    c.execute_batch(include_str!("../src/upgrade/schema7.sql"))
        .unwrap();
    c.execute_batch("PRAGMA application_id=1179603527; PRAGMA user_version=7;
        INSERT INTO workspaces(id,name,root) VALUES('legacy','Legacy','/legacy');
        INSERT INTO records(id,record_type,body,status,workspace_id) VALUES(42,'generic','Historical body','open','legacy');
        INSERT INTO record_comments(id,record_id,created_at_millis,author,body) VALUES(99,42,100,'human','Historical comment');
        INSERT INTO record_updates(id,record_id,created_at_millis,kind) VALUES(77,42,100,'created');").unwrap();
    drop(c);
    let before = upgrade::inspect(&legacy).unwrap();
    let after = upgrade::migrate(&legacy).unwrap();
    for table in before.tables {
        assert!(after.tables.contains(&table));
    }
    let repo = SqliteProjectRepository::open(&legacy)
        .unwrap()
        .in_record_workspace("legacy")
        .unwrap();
    assert_eq!(repo.show_record(42).unwrap().body, "Historical body");
}

#[test]
fn explicit_seven_to_eight_preserves_every_old_table_and_source_bytes() {
    let f = Fixture::new(7);
    let original = fs::read(&f.source).unwrap();
    let before = upgrade::inspect(&f.source).unwrap();
    assert!(!before.pristine);
    assert!(
        SqliteProjectRepository::open(&f.source).is_err(),
        "ordinary opening must still reject schema 7"
    );
    let copy = f.root.join("snapshot.db");
    assert_eq!(upgrade::snapshot(&f.source, &copy).unwrap(), before);
    let after = upgrade::migrate(&copy).unwrap();
    assert_eq!(after.schema, Some(8));
    for table in before.tables {
        assert!(after.tables.contains(&table), "lost {}", table.name);
    }
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "task_stacks")
            .unwrap()
            .rows,
        0
    );
    assert_eq!(fs::read(&f.source).unwrap(), original);
    drop(SqliteProjectRepository::open(&copy).unwrap());
    assert_eq!(
        upgrade::migrate(&copy).unwrap(),
        after,
        "same-schema migration is idempotent"
    );
}

#[test]
fn same_schema_copy_keeps_ids_comments_and_history() {
    let f = Fixture::new(8);
    let before = upgrade::inspect(&f.source).unwrap();
    let copy = f.root.join("copy.db");
    upgrade::snapshot(&f.source, &copy).unwrap();
    assert_eq!(upgrade::migrate(&copy).unwrap(), before);
    assert!(
        upgrade::snapshot(&f.source, &copy).is_err(),
        "must not overwrite a backup"
    );
}

#[test]
fn unknown_newer_foreign_and_falsely_stamped_schemas_are_read_only_failures() {
    for version in [0, 6, 9, 1000] {
        let f = Fixture::new(8);
        Connection::open(&f.source)
            .unwrap()
            .pragma_update(None, "user_version", version)
            .unwrap();
        let bytes = fs::read(&f.source).unwrap();
        assert!(upgrade::inspect(&f.source).is_err());
        assert!(upgrade::migrate(&f.source).is_err());
        assert_eq!(fs::read(&f.source).unwrap(), bytes);
    }
    let f = Fixture::new(8);
    Connection::open(&f.source)
        .unwrap()
        .execute_batch("PRAGMA user_version=7")
        .unwrap();
    assert!(
        upgrade::inspect(&f.source)
            .unwrap_err()
            .message
            .contains("definitions")
    );
    Connection::open(&f.source)
        .unwrap()
        .execute_batch("PRAGMA user_version=8; PRAGMA application_id=0")
        .unwrap();
    assert!(upgrade::inspect(&f.source).is_err());
}

#[test]
fn pristine_requires_seed_only_state_and_valid_schema() {
    let f = Fixture::new(8);
    let empty = f.root.join("empty.db");
    fs::write(&empty, []).unwrap();
    assert!(upgrade::inspect(&empty).unwrap().pristine);
    drop(SqliteProjectRepository::open(&empty).unwrap());
    assert!(upgrade::inspect(&empty).unwrap().pristine);
    assert!(!upgrade::inspect(&f.source).unwrap().pristine);
    Connection::open(&empty)
        .unwrap()
        .execute_batch("CREATE TABLE surprising (id INTEGER)")
        .unwrap();
    assert!(upgrade::inspect(&empty).is_err());
}

#[test]
fn snapshot_includes_committed_wal_rows() {
    let f = Fixture::new(7);
    let c = Connection::open(&f.source).unwrap();
    c.pragma_update(None, "journal_mode", "WAL").unwrap();
    c.execute_batch("INSERT INTO record_comments(record_id, created_at_millis, author, body) VALUES (1, 2, 'operator', 'WAL history')").unwrap();
    let before = upgrade::inspect(&f.source).unwrap();
    let copy = f.root.join("wal-copy.db");
    assert_eq!(upgrade::snapshot(&f.source, &copy).unwrap(), before);
    upgrade::migrate(&copy).unwrap();
    assert_eq!(
        Connection::open(&copy)
            .unwrap()
            .query_row("SELECT count(*) FROM record_comments", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    drop(c);
}

#[test]
fn unresolved_assignments_and_resources_are_retained_as_blockers() {
    let f = Fixture::new(7);
    let c = Connection::open(&f.source).unwrap();
    c.execute("INSERT INTO projects(slug,registered_folder,origin_url,workspace_id,local_slug) VALUES('p',?1,'https://example.com/p','personal','p')", [f.root.to_string_lossy().as_ref()]).unwrap();
    c.execute_batch("INSERT INTO agent_assignments(id,record_id,project_slug,profile_record_type,profile_revision,attempt_number,claimed_at_millis) VALUES('old-agent',1,'p','task',1,1,0);
        INSERT INTO agent_assignment_resources(assignment_id,resource_kind,resource_id,created_at_millis) VALUES('old-agent','agent','retained',0);").unwrap();
    let report = upgrade::inspect(&f.source).unwrap();
    assert!(report.blockers.contains(&"unresolved assignments".into()));
    assert!(
        report
            .blockers
            .contains(&"unretired assignment resources".into())
    );
    assert!(upgrade::snapshot(&f.source, &f.root.join("refused.db")).is_err());
    assert!(upgrade::migrate(&f.source).is_err());
    c.execute_batch("UPDATE agent_assignments SET closed_at_millis=1, closure_reason='retired';")
        .unwrap();
    assert!(
        upgrade::inspect(&f.source)
            .unwrap()
            .blockers
            .contains(&"unretired assignment resources".into())
    );
    assert!(upgrade::migrate(&f.source).is_err());
    c.execute_batch("UPDATE agent_assignment_resources SET retired_at_millis=2")
        .unwrap();
    assert!(upgrade::inspect(&f.source).unwrap().blockers.is_empty());
    upgrade::migrate(&f.source).unwrap();
}

#[test]
fn cross_workspace_relationships_fail_even_when_foreign_keys_are_valid() {
    let f = Fixture::new(7);
    let c = Connection::open(&f.source).unwrap();
    c.execute_batch("INSERT INTO workspaces(id,name,root) VALUES('other','Other','/other');
        INSERT INTO records(id,record_type,body,status,workspace_id) VALUES(2,'generic','Other record','open','other');").unwrap();
    c.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER,
        false,
    )
    .unwrap();
    c.execute_batch(
        "INSERT INTO record_relationships(from_id,to_id,kind) VALUES(1,2,'depends-on')",
    )
    .unwrap();
    drop(c);
    let bytes = fs::read(&f.source).unwrap();
    assert!(
        upgrade::migrate(&f.source)
            .unwrap_err()
            .message
            .contains("cross-Workspace")
    );
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
}

#[test]
fn corrupt_relations_are_rejected_before_conversion() {
    let f = Fixture::new(7);
    let c = Connection::open(&f.source).unwrap();
    c.execute_batch("PRAGMA foreign_keys=OFF; INSERT INTO record_comments(record_id, created_at_millis, body) VALUES (9999, 0, 'orphan')").unwrap();
    drop(c);
    let original = fs::read(&f.source).unwrap();
    assert!(
        upgrade::migrate(&f.source)
            .unwrap_err()
            .message
            .contains("foreign key")
    );
    assert_eq!(fs::read(&f.source).unwrap(), original);
}
