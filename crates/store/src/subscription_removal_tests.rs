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
