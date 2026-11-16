use super::*;
use crate::browser::router;
use crate::browser::tests::Fixture;
use forge_app::{NewRecord, Project, ProjectRepository, RecordRepository};
use std::sync::Arc;
use tower::ServiceExt;

fn seed(fixture: &Fixture, workspace: &str, body: String, projects: Vec<String>) -> u64 {
    fixture
        .store
        .records_in_workspace(workspace)
        .unwrap()
        .open_record(&NewRecord {
            body,
            project_slugs: projects,
            generated_by_ids: vec![],
        })
        .unwrap()
        .id
}

#[tokio::test]
async fn complete_detail_metadata_resolution_return_and_read_only() {
    let fixture = Fixture::new();
    for slug in ["one", "two"] {
        std::fs::create_dir_all(fixture.root.join("a").join(slug)).unwrap();
        fixture
            .store
            .register(&Project {
                reference: format!("a/{slug}"),
                workspace_id: "a".into(),
                slug: slug.into(),
                project_root: fixture.root.join("a").join(slug),
                origin_url: "https://example.test/p".into(),
            })
            .unwrap();
    }
    let body = format!(
        "# Detail title\n\n**Readable** and `code`\n\n{}\n\nLAST BODY LINE",
        "Long paragraph. ".repeat(4000)
    );
    let id = seed(&fixture, "a", body, vec!["a/one".into(), "a/two".into()]);
    let scoped = fixture.store.records_in_workspace("a").unwrap();
    scoped.start_record(id).unwrap();
    scoped
        .done_record(id, Some("<img src=x onerror=alert(1)>\nfinished"))
        .unwrap();
    fixture.connection().execute("UPDATE record_updates SET created_at_millis = CASE WHEN kind = 'created' THEN 1000 ELSE 2000 END WHERE record_id = ?1", [id.cast_signed()]).unwrap();
    let before = scoped.show_record(id).unwrap();
    let logs = scoped.list_record_updates(id).unwrap();
    let state =
        "workspace=a&type=generic&type=task&status=open&status=done&sort=oldest-created&page=3";
    let (status, html) = fixture.page(&format!("/records/{id}?{state}")).await;
    assert_eq!(status, 200);
    for text in [
        "Detail title",
        "Generic Record",
        "Done",
        "a/one, a/two",
        "1970-01-01T00:00:01Z",
        "1970-01-01T00:00:02Z",
        "<strong>Readable</strong>",
        "<code>code</code>",
        "LAST BODY LINE",
        "Completion Resolution",
        "finished",
    ] {
        assert!(html.contains(text), "{text}");
    }
    assert!(html.contains(&format!("#{id}")));
    assert!(!html.contains("<img src=x"));
    let back = DashboardQuery::parse(state).unwrap().url(3);
    // Askama escapes ampersands, but the complete original state is retained.
    assert!(
        html.contains(&back.replace('&', "&#38;")) || html.contains(&back.replace('&', "&amp;"))
    );
    assert_eq!(before, scoped.show_record(id).unwrap());
    assert_eq!(logs, scoped.list_record_updates(id).unwrap());
    let empty = seed(&fixture, "a", "# No resolution".into(), vec![]);
    let html = fixture
        .page(&format!("/records/{empty}?workspace=a"))
        .await
        .1;
    assert!(html.contains("Projectless"));
    assert!(!html.contains("Completion Resolution"));
    fixture
        .connection()
        .execute(
            "DELETE FROM record_updates WHERE record_id = ?1",
            [empty.cast_signed()],
        )
        .unwrap();
    let historical = fixture
        .page(&format!("/records/{empty}?workspace=a"))
        .await
        .1;
    assert_eq!(
        historical.matches("Unknown (no recorded Update)").count(),
        2
    );
    let list = fixture
        .page("/records?workspace=a&type=generic&sort=newest-created&page=1")
        .await
        .1;
    assert!(list.contains(&format!("/records/{empty}?workspace=a")));
    assert!(list.contains("sort=newest-created"));
}

#[tokio::test]
async fn guessed_ids_missing_and_backend_failures_are_distinct() {
    let fixture = Fixture::new();
    let id = seed(&fixture, "b", "FOREIGN SECRET".into(), vec![]);
    for uri in [
        format!("/records/{id}?workspace=a"),
        "/records/999999?workspace=a".into(),
        format!("/records/{id}?workspace=missing"),
    ] {
        let (status, html) = fixture.page(&uri).await;
        assert_eq!(status, 404);
        assert!(html.contains("Record not found"));
        assert!(!html.contains("FOREIGN SECRET"));
    }
    assert_eq!(fixture.page(&format!("/records/{id}")).await.0, 400);
    assert_eq!(
        fixture.page(&format!("/records/{id}?workspace=b")).await.0,
        200
    );
    let app = Arc::new(fixture.app());
    fixture
        .connection()
        .execute("DROP TABLE record_updates", [])
        .unwrap();
    let response = router(app)
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/records/{id}?workspace=b"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let html = String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("Record unavailable"));
    assert!(!html.contains("FOREIGN SECRET"));
}

#[tokio::test]
async fn malicious_markdown_metadata_and_return_inputs_are_inert() {
    let fixture = Fixture::new();
    let body = r#"# <script>alert('title')</script>

**safe bold** [safe](https://example.test "quoted & title")

<script>alert('raw')</script>
<img src=x onerror=alert(1)>
<iframe srcdoc="evil"></iframe>

[bad](javascript:alert%281%29) [mixed](JaVaScRiPt:evil) [data](data:text/html,evil) [vb](vbscript:evil)
[entity](jav&#x61;script:evil) [control](java&#x09;script:evil)
![bad image](data:image/svg+xml,evil)
[relative](/records) [mail](mailto:a@example.test)
"#;
    let id = seed(&fixture, "a", body.into(), vec![]);
    let (status, html) = fixture.page(&format!("/records/{id}?workspace=a")).await;
    assert_eq!(status, 200);
    for unsafe_markup in [
        "<script",
        "<img src=x",
        "<iframe",
        "href=\"javascript:",
        "href=\"JaVaScRiPt:",
        "href=\"data:",
        "href=\"vbscript:",
        "src=\"data:",
    ] {
        assert!(!html.contains(unsafe_markup), "{unsafe_markup}: {html}");
    }
    for safe in [
        "<strong>safe bold</strong>",
        "href=\"https://example.test\"",
        "href=\"/records\"",
        "href=\"mailto:a@example.test\"",
    ] {
        assert!(html.contains(safe), "{safe}");
    }
    for malicious in [
        "return=javascript%3Aalert(1)",
        "return=https%3A%2F%2Fevil.test",
        "return=%2F%2Fevil.test",
        "workspace=a&workspace=evil",
        "sort=%22%3E%3Cscript%3E",
    ] {
        let (status, html) = fixture
            .page(&format!("/records/{id}?workspace=a&{malicious}"))
            .await;
        assert_eq!(status, 400);
        assert!(!html.contains("<script"));
        assert!(!html.contains("href=\"https://evil"));
    }
    for destination in [
        "javascript:evil",
        "data:text/html,evil",
        "//evil.test",
        "\\evil.test",
        "java\tscript:evil",
    ] {
        assert!(!safe_destination(destination));
    }
}
