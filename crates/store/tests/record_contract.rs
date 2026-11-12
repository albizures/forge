use forge_store::SqliteProjectRepository;
use rusqlite::{Connection, OpenFlags};

#[test]
fn fresh_schema_uses_record_ownership_and_preserves_neutral_and_git_names() {
    let path =
        std::env::temp_dir().join(format!("forge-record-schema-{}.sqlite", std::process::id()));
    let store = SqliteProjectRepository::open(&path).unwrap();
    drop(store);
    let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    for table in [
        "record_checks",
        "record_findings",
        "check_assessments",
        "check_invalidations",
        "check_activities",
        "finding_activities",
        "task_base_commits",
    ] {
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "missing supported table {table}");
    }
    let obsolete: u32 = connection.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE name IN ('task_checks', 'task_findings', 'findings_by_task')",
        [], |row| row.get(0)).unwrap();
    assert_eq!(obsolete, 0);
    for table in ["record_checks", "record_findings"] {
        let columns = columns(&connection, table);
        assert!(columns.iter().any(|name| name == "record_id"));
        assert!(!columns.iter().any(|name| name == "task_id"));
        let owner_fk: String = connection
            .query_row(
                "SELECT \"table\" FROM pragma_foreign_key_list(?1) WHERE \"from\" = 'record_id'",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owner_fk, "records");
    }
    assert!(
        !columns(&connection, "records")
            .iter()
            .any(|name| name == "task_kind")
    );
    for table in ["check_assessments", "check_activities", "record_findings"] {
        let check_fk: String = connection
            .query_row(
                "SELECT \"table\" FROM pragma_foreign_key_list(?1) WHERE \"from\" = 'check_id'",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(check_fk, "record_checks");
    }
    let index_columns = connection
        .prepare("SELECT name FROM pragma_index_info('findings_by_record') ORDER BY seqno")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(index_columns, ["record_id", "id"]);
    let unique_indices = connection
        .prepare("SELECT name FROM pragma_index_list('record_checks') WHERE \"unique\" = 1")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let unique_columns: Vec<Vec<String>> = unique_indices
        .iter()
        .map(|index| {
            connection
                .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
                .unwrap()
                .query_map([index], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        })
        .collect();
    assert!(
        unique_columns
            .iter()
            .any(|columns| columns == &["record_id", "position"])
    );
    assert!(
        unique_columns
            .iter()
            .any(|columns| columns == &["record_id", "name"])
    );
    assert_eq!(
        columns(&connection, "task_base_commits"),
        ["record_id", "base_commit"]
    );
    drop(connection);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn record_naming_does_not_authorize_non_task_owners() {
    use forge_app::{NewRecord, NewTypedRecord, RecordRepository, RecordType, WorkspaceRepository};
    let store = SqliteProjectRepository::in_memory().unwrap();
    store
        .create_workspace(
            "test",
            "Test",
            &std::env::temp_dir().canonicalize().unwrap(),
        )
        .unwrap();
    let store = store.in_record_workspace("test").unwrap();
    let generic = store
        .open_record(&NewRecord {
            body: "Generic".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    let wayfinder = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Wayfinder,
            body: "Map".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    let initiative = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Initiative,
            body: "Coordinate".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    let grilling = store
        .open_grilling_under_wayfinder("Decision", wayfinder.id, None, vec![])
        .unwrap();
    for record in [generic, wayfinder, initiative, grilling] {
        let updates = store.list_record_updates(record.id).unwrap();
        assert_eq!(
            store.list_record_checks(record.id).unwrap_err().code,
            "invalid-record-type"
        );
        assert_eq!(
            store
                .list_record_findings(record.id, None, 50)
                .unwrap_err()
                .code,
            "invalid-record-type"
        );
        let finding = forge_app::findings::FindingRaiseInput {
            severity: forge_app::findings::Severity::Major,
            blocking: None,
            check_id: None,
            assessment_id: None,
            actor: "operator".into(),
            message: "Unsupported owner".into(),
            revision: None,
            dirty: None,
        };
        assert_eq!(
            store
                .raise_record_finding(record.id, &finding)
                .unwrap_err()
                .code,
            "invalid-record-type"
        );
        assert_eq!(store.list_record_updates(record.id).unwrap(), updates);
    }
}

fn columns(connection: &Connection, table: &str) -> Vec<String> {
    connection
        .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
        .unwrap()
        .query_map([table], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}
