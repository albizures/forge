use forge_app::{NewTypedRecord, RecordRepository, RecordType};
use forge_store::SqliteProjectRepository;
use rusqlite::Connection;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "forge-store-compatibility-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn incompatible_unversioned_store_is_rejected_without_modifying_it() {
    let database = Database::new();
    let connection = Connection::open(&database.0).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE records (id INTEGER PRIMARY KEY, body TEXT NOT NULL);
         INSERT INTO records VALUES (1, 'Retain this history');
         CREATE TABLE record_update_subscriptions (id INTEGER PRIMARY KEY);
         INSERT INTO record_update_subscriptions VALUES (42);",
        )
        .unwrap();
    drop(connection);
    let before = std::fs::read(&database.0).unwrap();

    let error = SqliteProjectRepository::open(&database.0)
        .err()
        .expect("incompatible store");
    assert_eq!(error.code, "fresh-store-required");
    assert!(error.message.contains("fresh store"));
    assert_eq!(std::fs::read(&database.0).unwrap(), before);
}

#[test]
fn incompatible_schema_revisions_are_rejected_without_modifying_stores() {
    for revision in [0, 1, 2, 3, 4, 5, 2_000_000_000] {
        let database = Database::new();
        let store = SqliteProjectRepository::open(&database.0).unwrap();
        drop(store);
        let connection = Connection::open(&database.0).unwrap();
        connection
            .pragma_update(None, "user_version", revision)
            .unwrap();
        drop(connection);
        let before = std::fs::read(&database.0).unwrap();
        let error = SqliteProjectRepository::open(&database.0)
            .err()
            .expect("incompatible schema revision");
        assert_eq!(error.code, "fresh-store-required");
        assert_eq!(std::fs::read(&database.0).unwrap(), before);
    }
}

#[test]
fn unrelated_database_is_not_adopted_even_with_a_matching_revision_number() {
    let database = Database::new();
    let connection = Connection::open(&database.0).unwrap();
    connection
        .execute_batch(
            "PRAGMA user_version = 6; CREATE TABLE unrelated (body TEXT);
        INSERT INTO unrelated VALUES ('Do not adopt');",
        )
        .unwrap();
    drop(connection);
    let before = std::fs::read(&database.0).unwrap();
    let error = SqliteProjectRepository::open(&database.0).err().unwrap();
    assert_eq!(error.code, "fresh-store-required");
    assert_eq!(std::fs::read(&database.0).unwrap(), before);
}

#[test]
fn concurrent_first_open_initializes_one_durable_store() {
    let database = Database::new();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let barrier = barrier.clone();
            let path = database.0.clone();
            std::thread::spawn(move || {
                barrier.wait();
                SqliteProjectRepository::open(&path).unwrap()
            })
        })
        .collect();
    for handle in handles {
        drop(handle.join().unwrap());
    }
    drop(SqliteProjectRepository::open(&database.0).unwrap());
}

#[test]
fn ordinary_restart_retains_new_store_record_history() {
    let database = Database::new();
    let store = SqliteProjectRepository::open(&database.0).unwrap();
    let workspace = std::env::temp_dir().canonicalize().unwrap();
    forge_app::WorkspaceRepository::create_workspace(
        &store,
        "compatibility",
        "Compatibility",
        &workspace,
    )
    .unwrap();
    let store = store.in_record_workspace("compatibility").unwrap();
    let record = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,

            body: "Durable record history".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    drop(store);

    let reopened = SqliteProjectRepository::open(&database.0)
        .unwrap()
        .in_record_workspace("compatibility")
        .unwrap();
    assert_eq!(reopened.show_record(record.id).unwrap(), record);
}
