use super::{tests::Fixture, *};
use forge_app::{
    NewRecord, NewTypedRecord, Project, ProjectRepository, RecordRelationshipKind,
    RecordRepository, record_browsing::RecordFilters,
};

fn seed(fixture: &Fixture) -> Vec<forge_app::Record> {
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
    let app = fixture.app().records_in_workspace("a").unwrap();
    let mut records = Vec::new();
    for index in 0..180 {
        let record = app.open_record(&format!("Record {index}"), vec![]).unwrap();
        let record = match index % 3 {
            0 => record,
            1 => app.start_record(record.id).unwrap(),
            _ => {
                app.start_record(record.id).unwrap();
                app.done_record(record.id, None).unwrap()
            }
        };
        records.push(record);
    }
    for (_, _, kind) in TYPES.into_iter().skip(1) {
        records.push(
            fixture
                .store
                .records_in_workspace("a")
                .unwrap()
                .open_typed_record(&NewTypedRecord {
                    record_type: kind,
                    body: format!("{kind:?}"),
                    project_slugs: if kind == RecordType::Spec {
                        vec!["a/p".into()]
                    } else {
                        vec![]
                    },
                    generated_by_ids: vec![],
                })
                .unwrap(),
        );
    }
    fixture
        .store
        .records_in_workspace("b")
        .unwrap()
        .open_record(&NewRecord {
            body: "FOREIGN".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    // Only fixture clocks are controlled in SQL. All Record changes use public operations.
    fixture
        .connection()
        .execute(
            "UPDATE record_updates SET created_at_millis = 1000 + (record_id % 7) * 1000",
            [],
        )
        .unwrap();
    fixture.connection().execute("UPDATE record_updates SET created_at_millis = 9000 + (record_id % 5) * 1000 WHERE kind != 'created'", []).unwrap();
    records
}

fn ids(html: &str) -> Vec<u64> {
    html.split("<h2 id=\"record-")
        .skip(1)
        .map(|tail| tail.split('"').next().unwrap().parse().unwrap())
        .collect()
}

fn all_ids(app: &Application, filters: &RecordFilters, sort: RecordSort) -> Vec<u64> {
    let first = app.browse_records("a", filters, sort, 1).unwrap();
    (1..=first.pages)
        .flat_map(|page| {
            app.browse_records("a", filters, sort, page)
                .unwrap()
                .records
                .into_iter()
                .map(|record| record.record.id)
        })
        .collect()
}

#[test]
fn complete_inventory_filters_sorts_ties_and_pagination() {
    let fixture = Fixture::new();
    let inventory = seed(&fixture);
    let app = fixture.app();
    let mut type_sets = vec![
        vec![],
        vec![RecordType::Task, RecordType::Initiative],
        TYPES.iter().map(|(_, _, kind)| *kind).collect(),
    ];
    type_sets.extend(TYPES.iter().map(|(_, _, kind)| vec![*kind]));
    let status_sets = vec![
        vec![],
        vec![RecordStatus::Open],
        vec![RecordStatus::InProgress],
        vec![RecordStatus::Done],
        vec![RecordStatus::InProgress, RecordStatus::Done],
        vec![RecordStatus::Open, RecordStatus::InProgress],
    ];
    for types in &type_sets {
        for statuses in &status_sets {
            let filters = RecordFilters {
                types: types.clone(),
                statuses: statuses.clone(),
            };
            for (_, _, sort) in SORTS {
                let mut expected: Vec<_> = inventory
                    .iter()
                    .filter(|r| {
                        (types.is_empty() || types.contains(&r.record_type))
                            && (statuses.is_empty() || statuses.contains(&r.status))
                    })
                    .collect();
                // Independent expected clock values, including controlled ties.
                expected.sort_by_key(|r| {
                    let date = if sort.is_creation() || r.status == RecordStatus::Open {
                        1000 + (r.id % 7) * 1000
                    } else {
                        9000 + (r.id % 5) * 1000
                    };
                    (
                        if matches!(
                            sort,
                            RecordSort::RecentlyUpdated | RecordSort::NewestCreated
                        ) {
                            u64::MAX - date
                        } else {
                            date
                        },
                        r.id,
                    )
                });
                let first = app.browse_records("a", &filters, sort, 1).unwrap();
                assert_eq!(first.total, expected.len());
                assert_eq!(first.records.len(), expected.len().min(50));
                let actual = all_ids(&app, &filters, sort);
                assert_eq!(actual, expected.iter().map(|r| r.id).collect::<Vec<_>>());
                assert_eq!(
                    actual
                        .iter()
                        .collect::<std::collections::HashSet<_>>()
                        .len(),
                    actual.len()
                );
                assert!(
                    app.browse_records("a", &filters, sort, first.pages + 1)
                        .is_err()
                );
            }
        }
    }
    let filters = RecordFilters {
        types: vec![RecordType::Generic],
        statuses: vec![RecordStatus::Open, RecordStatus::InProgress],
    };
    let first = app
        .browse_records("a", &filters, RecordSort::default(), 1)
        .unwrap();
    assert_eq!((first.total, first.pages), (120, 3));
    assert_eq!(
        app.browse_records("a", &filters, RecordSort::default(), 3)
            .unwrap()
            .records
            .len(),
        20
    );
    assert!(
        app.browse_records("a", &filters, RecordSort::default(), 0)
            .is_err()
    );
}

#[tokio::test]
async fn urls_controls_reset_refresh_and_invalid_input() {
    let fixture = Fixture::new();
    seed(&fixture);
    let uri = "/records?workspace=a&type=generic&status=open&status=in_progress&sort=oldest-created&page=2";
    let (status, html) = fixture.page(uri).await;
    assert_eq!(status, 200);
    assert_eq!(ids(&html).len(), 50);
    assert!(html.contains("120 matching Records · Page 2 of 3"));
    assert!(html.contains("Created (UTC)"));
    assert!(html.contains("value=\"generic\" checked"));
    assert!(html.contains("value=\"open\" checked"));
    assert!(html.contains("value=\"in_progress\" checked"));
    assert!(html.contains("value=\"oldest-created\" selected"));
    let query = DashboardQuery::parse(uri.split_once('?').unwrap().1).unwrap();
    for page in [1, 2, 3] {
        let url = query.url(page);
        // Askama may escape ampersands either numerically or by named entity.
        let escaped = url.replace('&', "&#38;");
        assert!(
            html.contains(&escaped) || html.contains(&url.replace('&', "&amp;")),
            "{html}"
        );
        let roundtrip = DashboardQuery::parse(url.split_once('?').unwrap().1).unwrap();
        assert_eq!(roundtrip.url(page), url);
        assert_eq!(roundtrip.page, page);
    }
    let expected = all_ids(&fixture.app(), &query.filters, query.sort);
    let mut reached = Vec::new();
    for page in 1..=3 {
        let body = fixture.page(&query.url(page)).await.1;
        reached.extend(ids(&body));
    }
    assert_eq!(reached, expected);
    assert_eq!(fixture.page(uri).await.1, html); // reload/shared URL
    let form = html
        .split("<form")
        .nth(1)
        .unwrap()
        .split("</form>")
        .next()
        .unwrap();
    assert!(form.contains("method=\"get\""));
    assert!(!form.contains("name=\"page\"")); // applying changes always resets, never carries page 2
    assert!(!html.contains("hx-"));
    assert!(!html.contains("<script"));
    let before = fixture.app().recent_records("a").unwrap();
    let scoped = fixture.app().records_in_workspace("a").unwrap();
    let id = ids(&html)[0];
    scoped.edit_record(id, "REFRESHED TITLE").unwrap();
    let refreshed = fixture.page(&query.url(2)).await.1;
    assert!(refreshed.contains("REFRESHED TITLE"));
    assert!(refreshed.contains("120 matching Records · Page 2 of 3"));
    let opening_dates = |records: Vec<DatedRecord>| {
        records
            .into_iter()
            .map(|r| (r.record.id, r.dates.opened_at_millis))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    assert_eq!(
        opening_dates(before),
        opening_dates(fixture.app().recent_records("a").unwrap())
    );
    for query in [
        "page=0",
        "page=-1",
        "page=abc",
        "page=9999999999999999999999999999",
        "page=5",
        "sort=nope",
        "type=nope",
        "status=nope",
        "workspace=a&workspace=b",
        "page=1&page=2",
        "unknown=x",
        "type=%",
        "type=%FF",
    ] {
        assert_eq!(
            fixture
                .page(&format!("/records?workspace=a&{query}"))
                .await
                .0,
            400,
            "{query}"
        );
    }
    assert_eq!(
        fixture
            .page("/records?workspace=a&type=spec&status=done")
            .await
            .0,
        200
    );
    assert!(
        fixture
            .page("/records?workspace=a&type=spec&status=done")
            .await
            .1
            .contains("0 matching Records")
    );
}

#[test]
fn comment_and_relationship_updates_move_only_updated_orders() {
    let fixture = Fixture::new();
    let records = seed(&fixture);
    let app = fixture.app();
    let scoped = app.records_in_workspace("a").unwrap();
    let filters = RecordFilters::default();
    let created = all_ids(&app, &filters, RecordSort::NewestCreated);
    let oldest = all_ids(&app, &filters, RecordSort::OldestCreated);
    let first = records[0].id;
    let second = records[3].id;
    let third = records[6].id;
    let comment = scoped.add_record_comment(first, "a comment", None).unwrap();
    assert_eq!(
        all_ids(&app, &filters, RecordSort::RecentlyUpdated)[0],
        first
    );
    scoped.delete_record_comment(first, comment.id.0).unwrap();
    assert_eq!(
        all_ids(&app, &filters, RecordSort::RecentlyUpdated)[0],
        first
    );
    scoped
        .relate_records(second, third, RecordRelationshipKind::GeneratedBy)
        .unwrap();
    let updated = all_ids(&app, &filters, RecordSort::RecentlyUpdated);
    assert!(updated[..3].contains(&second));
    assert!(updated[..3].contains(&third));
    scoped
        .unrelate_records(second, third, RecordRelationshipKind::GeneratedBy)
        .unwrap();
    assert_eq!(created, all_ids(&app, &filters, RecordSort::NewestCreated));
    assert_eq!(oldest, all_ids(&app, &filters, RecordSort::OldestCreated));
}

#[test]
fn encoded_workspace_and_duplicate_selections_round_trip() {
    let query = DashboardQuery::parse("workspace=space%26%2B%3D%20%C3%A9&type=task&type=generic&type=task&status=done&sort=newest-created&page=3").unwrap();
    assert_eq!(query.workspace, "space&+= é");
    assert_eq!(query.filters.types.len(), 2);
    let url = query.url(3);
    assert_eq!(
        DashboardQuery::parse(url.split_once('?').unwrap().1)
            .unwrap()
            .url(3),
        url
    );
    assert!(DashboardQuery::parse("page=2").is_err());
}
