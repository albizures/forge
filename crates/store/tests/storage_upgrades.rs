//! Explicit upgrades preserve historical evidence and convert prerequisite intent.
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
        if schema < 20 {
            c.execute_batch("DROP TABLE assignment_prompt_intents; DROP TABLE assignment_nudges;")
                .unwrap();
        }
        if schema < 17 {
            c.execute_batch("DROP TABLE retained_work_recoveries;")
                .unwrap();
        }
        if schema == 15 {
            c.execute_batch("DROP TABLE check_continuations;").unwrap();
            c.execute_batch(include_str!("../src/upgrade/continuations.sql"))
                .unwrap();
        }
        if schema < 16 {
            c.execute_batch("DROP TABLE check_continuation_resets;")
                .unwrap();
        }
        if schema < 15 {
            c.execute_batch("DROP TABLE check_continuations;").unwrap();
        }
        if schema < 14 {
            c.execute_batch("DROP TABLE assignment_sessions;").unwrap();
        }
        if schema < 12 {
            c.execute_batch("DROP TABLE builds_on_dependencies;")
                .unwrap();
        }
        if schema < 11 {
            c.execute_batch("DROP TABLE forced_completions;").unwrap();
        }
        if schema < 10 {
            c.execute_batch("DROP TABLE task_unchanged_results;")
                .unwrap();
        }
        if schema < 9 {
            c.execute_batch("DROP TABLE ordering_only_dependencies;")
                .unwrap();
        }
        if schema == 7 {
            c.execute_batch("DROP TABLE task_stacks;").unwrap();
        }
        c.pragma_update(None, "user_version", schema).unwrap();
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
fn explicit_seven_to_current_preserves_every_old_table_and_source_bytes() {
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
    assert_eq!(after.schema, Some(20));
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
fn explicit_eight_to_eleven_preserves_all_old_facts_without_inventing_intent() {
    let f = Fixture::new(8);
    let c = Connection::open(&f.source).unwrap();
    c.execute_batch("INSERT INTO records(id,record_type,body,status,workspace_id) VALUES(42,'task','Prerequisite','done','personal');
        INSERT INTO record_relationships(from_id,to_id,kind) VALUES(1,42,'depends-on');").unwrap();
    drop(c);
    let bytes = fs::read(&f.source).unwrap();
    let before = upgrade::inspect(&f.source).unwrap();
    assert!(SqliteProjectRepository::open(&f.source).is_err());
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
    let copy = f.root.join("nine.db");
    upgrade::snapshot(&f.source, &copy).unwrap();
    let after = upgrade::migrate(&copy).unwrap();
    assert_eq!(after.schema, Some(20));
    for table in before.tables {
        assert!(after.tables.contains(&table), "changed {}", table.name);
    }
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "ordering_only_dependencies")
            .unwrap()
            .rows,
        1
    );
    let repo = SqliteProjectRepository::open(&copy).unwrap();
    let record = repo.show_record(1).unwrap();
    assert_eq!(record.relationships.len(), 1);
    assert!(record.relationships[0].ordering_only);
    assert_eq!(record.relationships[0].to_id, 42);
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
}

#[test]
fn nine_to_eleven_preserves_every_table_without_inventing_unchanged_results() {
    let f = Fixture::new(9);
    let bytes = fs::read(&f.source).unwrap();
    let before = upgrade::inspect(&f.source).unwrap();
    assert!(SqliteProjectRepository::open(&f.source).is_err());
    let copy = f.root.join("ten.db");
    upgrade::snapshot(&f.source, &copy).unwrap();
    let after = upgrade::migrate(&copy).unwrap();
    assert_eq!(after.schema, Some(20));
    for table in before.tables {
        assert!(after.tables.contains(&table));
    }
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "task_unchanged_results")
            .unwrap()
            .rows,
        0
    );
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
    drop(SqliteProjectRepository::open(&copy).unwrap());
}

#[test]
fn ten_to_eleven_preserves_history_without_inventing_forced_completions() {
    let f = Fixture::new(10);
    let before = upgrade::inspect(&f.source).unwrap();
    let bytes = fs::read(&f.source).unwrap();
    assert!(SqliteProjectRepository::open(&f.source).is_err());
    let copy = f.root.join("eleven.db");
    upgrade::snapshot(&f.source, &copy).unwrap();
    let after = upgrade::migrate(&copy).unwrap();
    assert_eq!(after.schema, Some(20));
    for table in before.tables {
        assert!(after.tables.contains(&table));
    }
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "forced_completions")
            .unwrap()
            .rows,
        0
    );
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
    let repo = SqliteProjectRepository::open(&copy).unwrap();
    assert!(repo.forced_completion(1).unwrap().is_none());
}

#[test]
fn eleven_expansion_preserves_old_intent_and_rejects_ordinary_open_before_writes() {
    let f = Fixture::new(11);
    let bytes = fs::read(&f.source).unwrap();
    assert!(SqliteProjectRepository::open(&f.source).is_err());
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
    let before = upgrade::inspect(&f.source).unwrap();
    let after = upgrade::migrate(&f.source).unwrap();
    assert_eq!(after.schema, Some(20));
    for table in before.tables {
        assert!(after.tables.contains(&table));
    }
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "builds_on_dependencies")
            .unwrap()
            .rows,
        0
    );
    drop(SqliteProjectRepository::open(&f.source).unwrap());
}

#[test]
fn semantic_twelve_requires_explicit_conversion_despite_identical_ddl() {
    let f = Fixture::new(12);
    let bytes = fs::read(&f.source).unwrap();
    let before = upgrade::inspect(&f.source).unwrap();
    assert!(SqliteProjectRepository::open(&f.source).is_err());
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
    let copy = f.root.join("thirteen.db");
    upgrade::snapshot(&f.source, &copy).unwrap();
    let after = upgrade::migrate(&copy).unwrap();
    assert_eq!(after.schema, Some(20));
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
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
    drop(SqliteProjectRepository::open(&copy).unwrap());
}

#[test]
fn fourteen_requires_explicit_upgrade_without_historical_continuation_backfill() {
    let f = Fixture::new(14);
    let bytes = fs::read(&f.source).unwrap();
    assert!(SqliteProjectRepository::open(&f.source).is_err());
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
    let before = upgrade::inspect(&f.source).unwrap();
    let copy = f.root.join("fifteen.db");
    upgrade::snapshot(&f.source, &copy).unwrap();
    let after = upgrade::migrate(&copy).unwrap();
    assert_eq!(after.schema, Some(20));
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
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
    drop(SqliteProjectRepository::open(&copy).unwrap());
}

#[test]
fn fifteen_upgrade_retains_accounting_and_adds_no_reset_history() {
    let f = Fixture::new(15);
    let c = Connection::open(&f.source).unwrap();
    c.execute_batch("INSERT INTO check_continuations SELECT id,20,1,'{\"attempt_id\":\"retained\",\"evidence\":{\"kind\":\"repository\",\"repository\":\"repo\",\"commit\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"},\"handoff\":{\"stopping_reason\":\"budget\",\"completed_work\":\"partial\",\"verification\":\"unrun\",\"remaining_work\":\"finish\"}}' FROM record_checks WHERE record_id=1 AND position=0;").unwrap();
    drop(c);
    let bytes = fs::read(&f.source).unwrap();
    assert!(SqliteProjectRepository::open(&f.source).is_err());
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
    let before = upgrade::inspect(&f.source).unwrap();
    let copy = f.root.join("sixteen.db");
    upgrade::snapshot(&f.source, &copy).unwrap();
    let after = upgrade::migrate(&copy).unwrap();
    assert_eq!(after.schema, Some(20));
    let old = before
        .tables
        .iter()
        .find(|t| t.name == "check_continuations")
        .unwrap();
    let new = after.tables.iter().find(|t| t.name == old.name).unwrap();
    assert_eq!((old.rows, &old.digest), (new.rows, &new.digest));
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "check_continuation_resets")
            .unwrap()
            .rows,
        0
    );
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
}

#[test]
fn sixteen_upgrade_preserves_history_and_invents_no_salvage_authority() {
    let f = Fixture::new(16);
    let bytes = fs::read(&f.source).unwrap();
    assert!(SqliteProjectRepository::open(&f.source).is_err());
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
    let before = upgrade::inspect(&f.source).unwrap();
    let copy = f.root.join("seventeen.db");
    upgrade::snapshot(&f.source, &copy).unwrap();
    let after = upgrade::migrate(&copy).unwrap();
    assert_eq!(after.schema, Some(20));
    for table in before.tables {
        assert!(after.tables.contains(&table));
    }
    assert_eq!(
        after
            .tables
            .iter()
            .find(|t| t.name == "retained_work_recoveries")
            .unwrap()
            .rows,
        0
    );
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
    let repo = SqliteProjectRepository::open(&copy).unwrap();
    assert!(repo.record_work_state(1).unwrap().recoveries.is_empty());
}

#[test]
fn seventeen_requires_explicit_semantic_upgrade_without_rewriting_recovery_json() {
    verify_recovery_semantic_upgrade(17);
}

#[test]
fn eighteen_requires_explicit_upgrade_without_reauthorizing_old_fingerprints() {
    verify_recovery_semantic_upgrade(18);
}

fn verify_recovery_semantic_upgrade(schema: u32) {
    let f = Fixture::new(schema);
    let c = Connection::open(&f.source).unwrap();
    c.execute_batch("INSERT INTO projects VALUES('personal/recovery','/retained-repository','https://example.invalid/repo','personal','recovery'); INSERT INTO agent_assignments(id,record_id,project_slug,profile_record_type,profile_revision,attempt_number,claimed_at_millis,closed_at_millis,closure_reason) VALUES('attempt',1,'personal/recovery','task',1,1,1,2,'retired');").unwrap();
    let mut recovery: serde_json::Value = serde_json::from_str(r#"{"record_id":1,"check_id":1,"request":{"source_assignment":"attempt","generation":1,"evidence":{"kind":"repository","repository":"repo","commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"actor":"operator","message":"salvage","confirm_old_agent_cannot_act":true,"confirm_resources_safe":true},"worktree_path":"/retained","generation":2,"created_at_millis":1}"#).unwrap();
    if schema == 18 {
        recovery["retained_state"] = serde_json::json!({"sha256": "a".repeat(64)});
    }
    c.execute(
        "INSERT INTO retained_work_recoveries VALUES('attempt',1,?1)",
        [recovery.to_string()],
    )
    .unwrap();
    drop(c);
    let bytes = fs::read(&f.source).unwrap();
    assert!(SqliteProjectRepository::open(&f.source).is_err());
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
    let before = upgrade::inspect(&f.source).unwrap();
    let copy = f.root.join("eighteen.db");
    upgrade::snapshot(&f.source, &copy).unwrap();
    let after = upgrade::migrate(&copy).unwrap();
    assert_eq!(after.schema, Some(20));
    // Only the empty Nudge tables are new; every earlier table is byte-exact.
    assert!(before.tables.iter().all(|t| after.tables.contains(t)));
    let store = SqliteProjectRepository::open(&copy).unwrap();
    let recovery = &store.record_work_state(1).unwrap().recoveries[0];
    assert_eq!(recovery.retained_state.is_some(), schema == 18);
    if let Some(state) = &recovery.retained_state {
        assert_eq!(state.sha256, "a".repeat(64), "never rewrite old authority");
    }
    assert_eq!(fs::read(&f.source).unwrap(), bytes);
}

#[test]
fn same_schema_copy_keeps_ids_comments_and_history() {
    let f = Fixture::new(20);
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
    for version in [0, 6, 11, 1000] {
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

#[test]
fn nineteen_upgrades_with_empty_nudge_tables_and_no_invented_prompts() {
    let f = Fixture::new(19);
    let before = upgrade::inspect(&f.source).unwrap();
    assert!(
        before
            .tables
            .iter()
            .all(|t| !t.name.starts_with("assignment_nudges"))
    );
    let copy = f.root.join("twenty.db");
    upgrade::snapshot(&f.source, &copy).unwrap();
    let after = upgrade::migrate(&copy).unwrap();
    assert_eq!(after.schema, Some(20));
    for name in ["assignment_nudges", "assignment_prompt_intents"] {
        assert_eq!(
            after.tables.iter().find(|t| t.name == name).unwrap().rows,
            0,
            "{name}"
        );
    }
    assert!(before.tables.iter().all(|t| after.tables.contains(t)));
    SqliteProjectRepository::open(&copy).unwrap();
}
