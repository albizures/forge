//! Dedicated read-only detail rendering and an explicit Markdown safety policy.
use super::{
    Application, Card, DashboardQuery, IntoResponse, RecordSort, Response, StatusCode, Template,
    display_date, invalid_query, render, warn,
};
use pulldown_cmark::{Event, Parser, Tag, html};

// Carry structured dashboard state, never an arbitrary return destination.
pub(super) fn url(id: u64, query: &DashboardQuery) -> String {
    query
        .url(query.page)
        .replacen("/records?", &format!("/records/{id}?"), 1)
}

pub(super) fn page(application: &Application, id: u64, raw: &str) -> Response {
    let mut response = match DashboardQuery::parse(raw) {
        Ok(query) if !query.workspace.is_empty() => load(application, id, &query),
        Ok(_) => invalid_query("Select a Workspace to view a Record"),
        Err(message) => invalid_query(message),
    };
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

fn load(application: &Application, id: u64, query: &DashboardQuery) -> Response {
    match application.record_detail(&query.workspace, id) {
        Ok(dated) => {
            let created = display_date(dated.dates.opened_at_millis);
            let updated = display_date(dated.dates.updated_at_millis);
            let body = markdown(&dated.record.body);
            let resolution = dated.record.completion_resolution.clone();
            let card = Card::new(dated, RecordSort::RecentlyUpdated);
            render(&DetailTemplate {
                card: &card,
                workspace: &query.workspace,
                created: &created,
                updated: &updated,
                body: &body,
                resolution: resolution.as_deref(),
                back: &query.url(query.page),
            })
            .map_or_else(
                |_| {
                    super::error::page(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Record unavailable",
                        "Unable to render this Record. Try again.",
                        &url(id, query),
                        &query.url(query.page),
                    )
                },
                IntoResponse::into_response,
            )
        }
        Err(error) => {
            warn!(?error, "failed to load Record detail");
            let (status, heading, message) = if matches!(
                error.code,
                "record-not-found" | "workspace-not-found" | "cross-workspace-record"
            ) {
                (
                    StatusCode::NOT_FOUND,
                    "Record not found",
                    "No such Record in the selected Workspace.",
                )
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Record unavailable",
                    "Unable to load this Record. Try again.",
                )
            };
            super::error::page(
                status,
                heading,
                message,
                &url(id, query),
                &query.url(query.page),
            )
        }
    }
}

fn safe_destination(destination: &str) -> bool {
    // Reject controls/whitespace and backslashes before interpreting a URL. A colon
    // in the first path segment is a scheme; allow only these explicit schemes.
    if destination
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
        || destination.starts_with("//")
    {
        return false;
    }
    let first = destination
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    match first.split_once(':') {
        Some((scheme, _)) => matches!(
            scheme.to_ascii_lowercase().as_str(),
            "https" | "http" | "mailto"
        ),
        None => true,
    }
}

fn markdown(source: &str) -> String {
    let events = Parser::new(source).map(|event| match event {
        // Raw HTML remains readable text, never trusted markup. Code is escaped
        // by the renderer too. No extensions admitting additional HTML are enabled.
        Event::Html(text) | Event::InlineHtml(text) => Event::Text(text),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: if safe_destination(&dest_url) {
                dest_url
            } else {
                "".into()
            },
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: if safe_destination(&dest_url) {
                dest_url
            } else {
                "".into()
            },
            title,
            id,
        }),
        other => other,
    });
    let mut output = String::new();
    html::push_html(&mut output, events);
    output
}

#[derive(Template)]
#[template(path = "record.html")]
struct DetailTemplate<'a> {
    card: &'a Card,
    workspace: &'a str,
    created: &'a str,
    updated: &'a str,
    body: &'a str,
    resolution: Option<&'a str>,
    back: &'a str,
}

#[cfg(test)]
mod tests;
