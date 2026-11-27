use rusqlite::Connection;

#[test]
fn observations_require_scoped_project_and_exactly_one_valid_state() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys = ON; CREATE TABLE record_projects (record_id INTEGER NOT NULL, project_slug TEXT NOT NULL, PRIMARY KEY(record_id, project_slug)); INSERT INTO record_projects VALUES (1, 'w/p');").unwrap();
    db.execute_batch(include_str!("../src/planning_context.sql"))
        .unwrap();
    assert!(db.execute("INSERT INTO record_planning_context VALUES (1, 'w/other', 'available', '/repo', 'main', NULL)", []).is_err());
    assert!(db.execute("INSERT INTO record_planning_context VALUES (1, 'w/p', 'available', '/repo', ' ', NULL)", []).is_err());
    assert!(
        db.execute(
            "INSERT INTO record_planning_context VALUES (1, 'w/p', 'unavailable', NULL, NULL, '')",
            []
        )
        .is_err()
    );
    assert!(db.execute("INSERT INTO record_planning_context VALUES (1, 'w/p', 'unavailable', '/repo', NULL, 'detached')", []).is_err());
    db.execute("INSERT INTO record_planning_context VALUES (1, 'w/p', 'unavailable', NULL, NULL, 'detached HEAD')", []).unwrap();
    let reason: String = db
        .query_row(
            "SELECT reason FROM record_planning_context WHERE record_id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reason, "detached HEAD");
}
