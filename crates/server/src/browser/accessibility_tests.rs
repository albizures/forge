use super::{tests::Fixture, *};
use axum::body::to_bytes;
use forge_app::{NewRecord, Project, ProjectRepository, RecordRepository};

async fn body(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

fn accessible_navigation(html: &str) {
    for marker in [
        "lang=\"en\"",
        "name=\"viewport\"",
        "<main>",
        ":focus-visible",
        "<nav aria-label=",
    ] {
        assert!(html.contains(marker), "missing {marker}");
    }
    assert!(!html.contains("tabindex=\"-1\""));
    assert!(!html.contains("<script"));
}

#[tokio::test]
async fn accessible_controls_cards_and_details_retain_long_text() {
    let fixture = Fixture::new();
    let long = "long-unbroken-text".repeat(200);
    let scope = format!("a/{}", "project".repeat(100));
    fixture
        .store
        .register(&Project {
            reference: scope.clone(),
            workspace_id: "a".into(),
            slug: "project".repeat(100),
            project_root: fixture.root.join("a"),
            origin_url: "https://example.test/long".into(),
        })
        .unwrap();
    let record = fixture
        .store
        .records_in_workspace("a")
        .unwrap()
        .open_record(&NewRecord {
            body: format!("# {long}\n\n{long}\n\n```\n{long}\n```"),
            project_slugs: vec![scope.clone()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let (status, html) = fixture
        .page("/records?workspace=a&type=generic&status=open&sort=oldest-created")
        .await;
    assert_eq!(status, 200);
    accessible_navigation(&html);
    for marker in [
        "<label for=\"workspace\">Forge Workspace</label>",
        "<label for=\"sort\">Date order</label>",
        "<legend>Record Type",
        "<legend>Record Status",
        "<label><input type=\"checkbox\"",
        "@media (max-width: 40rem)",
        "grid-template-columns: 1fr",
        "fieldset { min-width: 0",
        "Record Identifier",
        "Record Type</dt><dd>Generic Record",
        "Record Status</dt><dd>Open",
        &scope,
        &long,
    ] {
        assert!(html.contains(marker), "missing {marker}");
    }
    let (_, detail) = fixture
        .page(&format!("/records/{}?workspace=a", record.id))
        .await;
    accessible_navigation(&detail);
    assert!(detail.contains(&long));
    assert!(detail.contains(&scope));
    assert!(detail.contains("pre { overflow-x: auto; }"));
    assert!(detail.contains("Return to Records"));
}

#[tokio::test]
async fn render_and_backend_failures_are_errors_with_escaped_stateful_recovery() {
    let query = DashboardQuery::parse(
        "workspace=%3Cscript%3E&type=task&status=done&sort=oldest-created&page=2",
    )
    .unwrap();
    // Inject only the failed boundary result, without global fault flags or live-store changes.
    for code in ["render", "storage"] {
        let response = dashboard_response(
            Err(RegistrationError::new(code, "PRIVATE diagnostic")),
            &query,
        );
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let html = body(response).await;
        accessible_navigation(&html);
        assert!(html.contains("Records unavailable"));
        assert!(html.contains("workspace=%3Cscript%3E&#38;type=task&#38;status=done&#38;sort=oldest-created&#38;page=2"));
        assert!(html.contains("page=1"));
        assert!(!html.contains("PRIVATE"));
        assert!(!html.contains("No Records"));
    }
}

#[tokio::test]
async fn missing_invalid_and_failed_details_have_usable_distinct_recovery() {
    let fixture = Fixture::new();
    let state = "workspace=a&type=generic&status=open&sort=newest-created&page=2";
    let (status, missing) = fixture.page(&format!("/records/999999?{state}")).await;
    assert_eq!(status, 404);
    accessible_navigation(&missing);
    assert!(missing.contains("Record not found"));
    assert!(missing.contains("/records/999999?workspace=a&#38;type=generic"));
    assert!(missing.contains("page=2"));
    let (status, invalid) = fixture
        .page("/records?workspace=a&type=generic&page=2")
        .await;
    assert_eq!(status, 400);
    accessible_navigation(&invalid);
    assert!(invalid.contains("Invalid Records query"));
    assert!(invalid.contains("type=generic&#38;sort=recently-updated&#38;page=1"));
    let (status, malformed) = fixture.page("/records?sort=bogus").await;
    assert_eq!(status, 400);
    accessible_navigation(&malformed);
    assert!(malformed.contains("href=\"/records\""));
}
