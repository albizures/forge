//! Read-only browser transport. Application calls are direct, never loopback RPC.
use std::sync::Arc;

use askama::Template;
use axum::{
    Router,
    extract::Query,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
};
use forge_app::{
    Application, ApplicationStatus, RecordStatus, RecordType, RegistrationError, Workspace,
    record_browsing::DatedRecord,
};
use serde::Deserialize;
use tracing::warn;

pub(super) fn router(application: Arc<Application>) -> Router {
    let status_application = application.clone();
    let fragment_application = application.clone();
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
            "/records",
            get(move |Query(query): Query<DashboardQuery>| {
                let app = application.clone();
                async move { dashboard(&app, query) }
            }),
        )
}

#[derive(Default, Deserialize)]
struct DashboardQuery {
    workspace: Option<String>,
}

fn dashboard(application: &Application, query: DashboardQuery) -> Response {
    match dashboard_page(application, query) {
        Ok(page) => page.into_response(),
        Err(error) => {
            warn!(?error, "failed to load Records dashboard");
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
    query: DashboardQuery,
) -> Result<Html<String>, RegistrationError> {
    let workspaces = application.workspaces()?;
    let selected = query.workspace.unwrap_or_default();
    // No implicit cwd or first-Workspace fallback: selection is explicit and reproducible.
    let cards = if selected.is_empty() {
        Vec::new()
    } else {
        application.workspace(&selected)?;
        application
            .recent_records(&selected)?
            .into_iter()
            .map(Card::from)
            .collect()
    };
    render(&DashboardTemplate {
        workspaces: &workspaces,
        selected: &selected,
        cards: &cards,
    })
    .map_err(|_| RegistrationError::new("render", "failed to render Records dashboard"))
}

struct Card {
    id: u64,
    title: String,
    kind: &'static str,
    status: &'static str,
    scope: String,
    updated: String,
}

impl From<DatedRecord> for Card {
    fn from(dated: DatedRecord) -> Self {
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
            updated: display_date(dated.dates.updated_at_millis),
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
}

#[cfg(test)]
mod tests;
