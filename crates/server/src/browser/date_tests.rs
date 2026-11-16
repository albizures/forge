use super::{tests::Fixture, *};
use axum::{body::Body, http::Request};
use forge_app::{RecordRelationshipKind, record_browsing::RecordFilters};
use tower::ServiceExt;

#[tokio::test]
async fn date_labels_unknowns_default_order_and_fresh_navigation() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let scoped = app.records_in_workspace("a").unwrap();
    let first = scoped.open_record("First", vec![]).unwrap();
    let unknown = scoped.open_record("Unknown", vec![]).unwrap();
    fixture
        .connection()
        .execute("UPDATE record_updates SET created_at_millis=1000", [])
        .unwrap();
    scoped.add_record_comment(first.id, "Update", None).unwrap();
    fixture
        .connection()
        .execute(
            "UPDATE record_updates SET created_at_millis=2000 WHERE kind='comment-added'",
            [],
        )
        .unwrap();
    fixture
        .connection()
        .execute(
            "DELETE FROM record_updates WHERE record_id=?1",
            [unknown.id.cast_signed()],
        )
        .unwrap();
    for (_, _, sort) in SORTS {
        let result = app
            .browse_records("a", &RecordFilters::default(), sort, 1)
            .unwrap();
        assert_eq!(
            result
                .records
                .iter()
                .map(|r| r.record.id)
                .collect::<Vec<_>>(),
            [first.id, unknown.id]
        );
        let mut query = DashboardQuery::parse("workspace=a").unwrap();
        query.sort = sort;
        let body = fixture.page(&query.url(1)).await.1;
        assert!(body.contains("Unknown (no recorded Update)"));
        assert!(body.contains(if sort.is_creation() {
            "Created (UTC)</dt><dd>1970-01-01T00:00:01Z"
        } else {
            "Last updated (UTC)</dt><dd>1970-01-01T00:00:02Z"
        }));
    }
    assert_eq!(
        fixture.page("/records?workspace=a").await.1,
        fixture
            .page("/records?workspace=a&sort=recently-updated&page=1")
            .await
            .1
    );
    let response = router(Arc::new(app))
        .oneshot(
            Request::builder()
                .uri("/records?workspace=a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.headers()[axum::http::header::CACHE_CONTROL],
        "no-store"
    );
}

#[test]
fn each_comment_and_relationship_change_can_move_updated_order() {
    for operation in [
        "add-comment",
        "delete-comment",
        "add-relationship",
        "delete-relationship",
    ] {
        let fixture = Fixture::new();
        let app = fixture.app();
        let scoped = app.records_in_workspace("a").unwrap();
        let first = scoped.open_record("First", vec![]).unwrap();
        let target = scoped.open_record("Target", vec![]).unwrap();
        let third = scoped.open_record("Third", vec![]).unwrap();
        let comment = if operation == "delete-comment" {
            Some(
                scoped
                    .add_record_comment(target.id, "Remove me", None)
                    .unwrap(),
            )
        } else {
            None
        };
        if operation == "delete-relationship" {
            scoped
                .relate_records(target.id, third.id, RecordRelationshipKind::GeneratedBy)
                .unwrap();
        }
        // Freeze setup clocks, then exercise each real operation without store edits.
        fixture
            .connection()
            .execute("UPDATE record_updates SET created_at_millis=1000", [])
            .unwrap();
        let filters = RecordFilters::default();
        let before = app
            .browse_records("a", &filters, RecordSort::RecentlyUpdated, 1)
            .unwrap();
        assert_eq!(before.records[0].record.id, first.id);
        let created_before = app
            .browse_records("a", &filters, RecordSort::NewestCreated, 1)
            .unwrap()
            .records;
        match operation {
            "add-comment" => {
                scoped.add_record_comment(target.id, "New", None).unwrap();
            }
            "delete-comment" => scoped
                .delete_record_comment(target.id, comment.unwrap().id.0)
                .unwrap(),
            "add-relationship" => {
                scoped
                    .relate_records(target.id, third.id, RecordRelationshipKind::GeneratedBy)
                    .unwrap();
            }
            "delete-relationship" => {
                scoped
                    .unrelate_records(target.id, third.id, RecordRelationshipKind::GeneratedBy)
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let recent = app
            .browse_records("a", &filters, RecordSort::RecentlyUpdated, 1)
            .unwrap();
        assert_eq!(recent.records[0].record.id, target.id, "{operation}");
        let least = app
            .browse_records("a", &filters, RecordSort::LeastRecentlyUpdated, 1)
            .unwrap();
        assert_eq!(least.records[0].record.id, first.id, "{operation}");
        let created_after = app
            .browse_records("a", &filters, RecordSort::NewestCreated, 1)
            .unwrap()
            .records;
        assert_eq!(
            created_before
                .iter()
                .map(|r| (r.record.id, r.dates.opened_at_millis))
                .collect::<Vec<_>>(),
            created_after
                .iter()
                .map(|r| (r.record.id, r.dates.opened_at_millis))
                .collect::<Vec<_>>()
        );
    }
}
