use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use forge_app::{
    NewTypedRecord, Project, ProjectRepository, RecordRepository, RecordType, RepositoryInspector,
    WorkspaceRepository,
};
use forge_store::SqliteProjectRepository;
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

struct Inspector;
impl RepositoryInspector for Inspector {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        Err(RegistrationError::new("unused", "unused"))
    }
}

pub(super) struct Fixture {
    pub(super) root: std::path::PathBuf,
    pub(super) store: SqliteProjectRepository,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forge-browser-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        let store = SqliteProjectRepository::open(&root.join("store.sqlite")).unwrap();
        for id in ["a", "b"] {
            store.create_workspace(id, id, &root.join(id)).unwrap();
        }
        Self { root, store }
    }
    pub(super) fn app(&self) -> Application {
        Application::new(
            Box::new(SqliteProjectRepository::open(&self.root.join("store.sqlite")).unwrap()),
            Box::new(Inspector),
        )
        .with_workspace_repository(Box::new(
            SqliteProjectRepository::open(&self.root.join("store.sqlite")).unwrap(),
        ))
    }
    pub(super) fn connection(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.join("store.sqlite")).unwrap()
    }
    pub(super) async fn page(&self, uri: &str) -> (u16, String) {
        let response = crate::router(crate::Daemon::new(self.app()))
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let body = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        (status, body)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

fn seed_inventory(fixture: &Fixture) -> Vec<u64> {
    fixture
        .store
        .register(&Project {
            reference: "a/p".into(),
            workspace_id: "a".into(),
            slug: "p".into(),
            project_root: fixture.root.join("a"),
            origin_url: "https://example.test/p".into(),
        })
        .unwrap();
    let scoped = fixture.store.records_in_workspace("a").unwrap();
    let mut ids = Vec::new();
    for (index, kind) in [
        RecordType::Generic,
        RecordType::Spec,
        RecordType::Task,
        RecordType::Wayfinder,
        RecordType::Grilling,
        RecordType::Initiative,
    ]
    .into_iter()
    .enumerate()
    {
        let body = format!("## <script>alert({index})</script>\nHidden body");
        let record = if kind == RecordType::Generic {
            scoped.open_record(&forge_app::NewRecord {
                body,
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
        } else {
            scoped.open_typed_record(&NewTypedRecord {
                record_type: kind,
                body,
                project_slugs: if kind == RecordType::Spec {
                    vec!["a/p".into()]
                } else {
                    vec![]
                },
                generated_by_ids: vec![],
            })
        }
        .unwrap();
        ids.push(record.id);
    }
    fixture
        .store
        .records_in_workspace("b")
        .unwrap()
        .open_record(&forge_app::NewRecord {
            body: "FOREIGN RECORD".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    fixture
        .connection()
        .execute("UPDATE record_updates SET created_at_millis = 1000", [])
        .unwrap();
    fixture
        .connection()
        .execute(
            "UPDATE record_updates SET created_at_millis = 2000 WHERE record_id = ?1",
            [ids[2].cast_signed()],
        )
        .unwrap();
    fixture
        .connection()
        .execute(
            "UPDATE records SET status = 'in_progress' WHERE id = ?1",
            [ids[1].cast_signed()],
        )
        .unwrap();
    fixture
        .connection()
        .execute(
            "UPDATE records SET status = 'done' WHERE id = ?1",
            [ids[2].cast_signed()],
        )
        .unwrap();
    ids
}

#[tokio::test]
async fn dashboard_is_scoped_complete_ordered_escaped_and_read_only() {
    let fixture = Fixture::new();
    let ids = seed_inventory(&fixture);
    let scoped = fixture.store.records_in_workspace("a").unwrap();
    let app = fixture.app();
    let before = app.recent_records("a").unwrap();
    let logs: Vec<_> = ids
        .iter()
        .map(|id| scoped.list_record_updates(*id).unwrap())
        .collect();
    let (status, html) = fixture.page("/records?workspace=a").await;
    assert_eq!(status, 200);
    for label in [
        "Generic Record",
        "Spec",
        "Task",
        "Wayfinder",
        "Grilling",
        "Initiative",
        "Projectless",
        "a/p",
        "Record Status",
        "Open",
        "In Progress",
        "Done",
        "1970-01-01T00:00:02Z",
    ] {
        assert!(html.contains(label), "{label}");
    }
    assert!(!html.contains("FOREIGN RECORD"));
    assert!(!html.contains("<script>"));
    assert!(!html.contains("## "));
    assert!(
        html.contains("&#60;script&#62;") || html.contains("&lt;script&gt;"),
        "{html}"
    );
    assert!(!html.contains("Hidden body"));
    let ordered: Vec<_> = [ids[2], ids[0], ids[1], ids[3], ids[4], ids[5]]
        .into_iter()
        .map(|id| html.find(&format!("<h2 id=\"record-{id}\"")).unwrap())
        .collect();
    assert!(ordered.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(fixture.page("/records?workspace=a").await.1, html);
    assert!(
        fixture
            .page("/records?workspace=b")
            .await
            .1
            .contains("FOREIGN RECORD")
    );
    assert_eq!(before, app.recent_records("a").unwrap());
    for (id, log) in ids.iter().zip(logs) {
        assert_eq!(scoped.list_record_updates(*id).unwrap(), log);
    }
    assert_eq!(fixture.page("/records?workspace=missing").await.0, 404);
    let unselected = fixture.page("/records").await.1;
    assert!(unselected.contains("Select a Workspace"));
    assert!(!unselected.contains("<article"));
    fixture
        .connection()
        .execute(
            "DELETE FROM record_updates WHERE record_id = ?1",
            [ids[0].cast_signed()],
        )
        .unwrap();
    let unknown = fixture.page("/records?workspace=a").await.1;
    assert!(unknown.contains("Unknown (no recorded Update)"));
    assert!(
        unknown
            .find(&format!("<h2 id=\"record-{}\"", ids[5]))
            .unwrap()
            < unknown
                .find(&format!("<h2 id=\"record-{}\"", ids[0]))
                .unwrap()
    );
}

#[tokio::test]
async fn http_navigation_status_empty_and_failures_are_distinct() {
    let fixture = Fixture::new();
    for (uri, text) in [
        ("/", "href=\"/records\""),
        ("/status", "Refresh status"),
        ("/records", "Select a Workspace"),
        ("/records?workspace=a", "No Records in this Workspace"),
        ("/records?workspace=missing", "Records unavailable"),
    ] {
        let (status, body) = fixture.page(uri).await;
        assert_eq!(status, if uri.ends_with("missing") { 404 } else { 200 });
        assert!(body.contains(text));
    }
    fixture.store.remove_workspace("a").unwrap();
    fixture.store.remove_workspace("b").unwrap();
    assert!(
        fixture
            .page("/records")
            .await
            .1
            .contains("No registered Workspaces")
    );
    fixture
        .store
        .create_workspace("a", "a", &fixture.root.join("a"))
        .unwrap();
    fixture.connection().execute("INSERT INTO records(record_type,body,status,workspace_id) VALUES ('generic','failed','open','a')", []).unwrap();
    let app = Arc::new(fixture.app());
    fixture
        .connection()
        .execute("DROP TABLE record_updates", [])
        .unwrap();
    let response = router(app)
        .oneshot(
            Request::builder()
                .uri("/records?workspace=a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 500);
    let body = String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("Records unavailable"));
    assert!(!body.contains("No Records"));
}
