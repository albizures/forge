//! Read-only browser transport. Application calls are direct, never loopback RPC.
use std::sync::Arc;

use askama::Template;
use axum::{
    Router,
    extract::RawQuery,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
};
use forge_app::{
    Application, ApplicationStatus, RecordStatus, RecordType, RegistrationError, Workspace,
    record_browsing::{DatedRecord, RecordSort},
};
mod detail;
mod query;
use query::{Choice, DashboardQuery, SORTS, STATUSES, TYPES};
use tracing::warn;

pub(super) fn router(application: Arc<Application>) -> Router {
    let status_application = application.clone();
    let fragment_application = application.clone();
    let detail_application = application.clone();
    Router::new()
        .route(
            "/",
            get(move || {
                let app = status_application.clone();
                async move { status_page(&app.status()) }
            }),
        )
        .route(
            "/status",
            get(move || {
                let app = fragment_application.clone();
                async move {
                    render(&StatusFragmentTemplate {
                        status: &app.status(),
                    })
                }
            }),
        )
        .route(
            "/records/{id}",
            get(
                move |axum::extract::Path(id): axum::extract::Path<u64>,
                      RawQuery(raw): RawQuery| {
                    let app = detail_application.clone();
                    async move { detail::page(&app, id, raw.as_deref().unwrap_or_default()) }
                },
            ),
        )
        .route(
            "/records",
            get(move |RawQuery(raw): RawQuery| {
                let app = application.clone();
                async move {
                    let mut response =
                        match DashboardQuery::parse(raw.as_deref().unwrap_or_default()) {
                            Ok(query) => dashboard(&app, &query),
                            Err(message) => invalid_query(message),
                        };
                    // Explicit navigation/Refresh must read current data, not a cached page.
                    response.headers_mut().insert(
                        axum::http::header::CACHE_CONTROL,
                        axum::http::HeaderValue::from_static("no-store"),
                    );
                    response
                }
            }),
        )
}

fn invalid_query(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, format!("Invalid Records query: {message}. Return to /records and apply selections to start at page one.")).into_response()
}

fn dashboard(application: &Application, query: &DashboardQuery) -> Response {
    match dashboard_page(application, query) {
        Ok(page) => page.into_response(),
        Err(error) => {
            warn!(?error, "failed to load Records dashboard");
            if error.code == "invalid-page" {
                return invalid_query(&error.message);
            }
            let status = if error.code == "workspace-not-found" {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Html("<!doctype html><html lang=\"en\"><title>Records unavailable</title><main><h1>Records unavailable</h1><p>Unable to load the selected Workspace. Check the selection or try again.</p><a href=\"/records\">Choose a Workspace</a> <a href=\"/\">Daemon status</a></main></html>".to_owned())).into_response()
        }
    }
}

fn dashboard_page(
    application: &Application,
    query: &DashboardQuery,
) -> Result<Html<String>, RegistrationError> {
    let workspaces = application.workspaces()?;
    let selected = &query.workspace;
    // No implicit cwd or first-Workspace fallback: selection is explicit and reproducible.
    let result = if selected.is_empty() {
        None
    } else {
        application.workspace(selected)?;
        Some(application.browse_records(selected, &query.filters, query.sort, query.page)?)
    };
    let total = result.as_ref().map_or(0, |result| result.total);
    let pages = result.as_ref().map_or(1, |result| result.pages);
    let cards: Vec<_> = result
        .into_iter()
        .flat_map(|result| result.records)
        .map(|dated| {
            let mut card = Card::new(dated, query.sort);
            card.url = detail::url(card.id, query);
            card
        })
        .collect();
    let types: Vec<_> = TYPES
        .iter()
        .map(|(value, label, kind)| Choice {
            value,
            label,
            selected: query.filters.types.contains(kind),
        })
        .collect();
    let statuses: Vec<_> = STATUSES
        .iter()
        .map(|(value, label, status)| Choice {
            value,
            label,
            selected: query.filters.statuses.contains(status),
        })
        .collect();
    let sorts: Vec<_> = SORTS
        .iter()
        .map(|(value, label, sort)| Choice {
            value,
            label,
            selected: query.sort == *sort,
        })
        .collect();
    let previous = query.url(query.page.saturating_sub(1).max(1));
    let next = query.url(query.page.saturating_add(1));
    let refresh = query.url(query.page);
    let date_label = if query.sort.is_creation() {
        "Created (UTC)"
    } else {
        "Last updated (UTC)"
    };
    render(&DashboardTemplate {
        workspaces: &workspaces,
        selected,
        cards: &cards,
        types: &types,
        statuses: &statuses,
        sorts: &sorts,
        total,
        page: query.page,
        pages,
        previous: &previous,
        next: &next,
        refresh: &refresh,
        sort_label: query.sort_label(),
        date_label,
    })
    .map_err(|_| RegistrationError::new("render", "failed to render Records dashboard"))
}

struct Card {
    id: u64,
    url: String,
    title: String,
    kind: &'static str,
    status: &'static str,
    scope: String,
    updated: String,
}

impl Card {
    fn new(dated: DatedRecord, sort: RecordSort) -> Self {
        let date = sort.date(dated.dates);
        let record = dated.record;
        let first = record.body.lines().next().unwrap_or_default();
        let hashes = first.bytes().take_while(|byte| *byte == b'#').count();
        let title = if (1..=6).contains(&hashes)
            && (first.len() == hashes || first.as_bytes()[hashes].is_ascii_whitespace())
        {
            first[hashes..].trim_start()
        } else {
            first
        };
        Self {
            id: record.id,
            url: String::new(),
            title: title.to_owned(),
            kind: match record.record_type {
                RecordType::Generic => "Generic Record",
                RecordType::Spec => "Spec",
                RecordType::Task => "Task",
                RecordType::Wayfinder => "Wayfinder",
                RecordType::Grilling => "Grilling",
                RecordType::Initiative => "Initiative",
            },
            status: match record.status {
                RecordStatus::Open => "Open",
                RecordStatus::InProgress => "In Progress",
                RecordStatus::Done => "Done",
            },
            scope: if record.project_slugs.is_empty() {
                "Projectless".to_owned()
            } else {
                record.project_slugs.join(", ")
            },
            updated: display_date(date),
        }
    }
}

fn display_date(millis: Option<u64>) -> String {
    let Some(millis) = millis else {
        return "Unknown (no recorded Update)".to_owned();
    };
    let date = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000)
        .ok()
        .and_then(|date| {
            date.format(&time::format_description::well_known::Rfc3339)
                .ok()
        });
    // Out-of-range historical values are still shown truthfully, not replaced by now.
    match date {
        Some(date) => date,
        None => format!("{millis} milliseconds since Unix epoch"),
    }
}

fn render(template: &impl Template) -> Result<Html<String>, StatusCode> {
    template.render().map(Html).map_err(|error| {
        warn!(%error, "failed to render browser template");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

fn status_page(status: &ApplicationStatus) -> Result<Html<String>, StatusCode> {
    let status_html = render(&StatusFragmentTemplate { status })?.0;
    render(&StatusPageTemplate {
        status_html: &status_html,
    })
}

#[derive(Template)]
#[template(path = "status.html")]
struct StatusPageTemplate<'a> {
    status_html: &'a str,
}

#[derive(Template)]
#[template(
    source = r##"<section id="status" aria-live="polite">
  <p><strong>{{ status.name }}</strong> {{ status.version }}</p>
  <p>Ready: {{ status.ready }}</p>
  <button type="button" hx-get="/status" hx-target="#status" hx-swap="outerHTML">Refresh status</button>
</section>"##,
    ext = "html"
)]
struct StatusFragmentTemplate<'a> {
    status: &'a ApplicationStatus,
}

#[derive(Template)]
#[template(path = "records.html")]
struct DashboardTemplate<'a> {
    workspaces: &'a [Workspace],
    selected: &'a str,
    cards: &'a [Card],
    types: &'a [Choice],
    statuses: &'a [Choice],
    sorts: &'a [Choice],
    total: usize,
    page: usize,
    pages: usize,
    previous: &'a str,
    next: &'a str,
    refresh: &'a str,
    sort_label: &'a str,
    date_label: &'a str,
}

#[cfg(test)]
mod browsing_tests;
#[cfg(test)]
mod date_tests;
#[cfg(test)]
mod tests;
