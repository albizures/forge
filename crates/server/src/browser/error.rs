//! Truthful browser failures with local, state-preserving recovery links.
use super::{IntoResponse, Response, StatusCode, Template, render};

pub(super) fn page(
    status: StatusCode,
    heading: &str,
    message: &str,
    retry: &str,
    back: &str,
) -> Response {
    let template = ErrorTemplate {
        heading,
        message,
        retry,
        back,
    };
    // Even a template failure must remain an error, never an empty success.
    let body = render(&template).unwrap_or_else(|_| axum::response::Html(
        "<!doctype html><html lang=\"en\"><title>Browser unavailable</title><main><h1>Browser unavailable</h1><p>Unable to render this page.</p><a href=\"/records\">Records</a> <a href=\"/\">Daemon status</a></main></html>".to_owned()
    ));
    (status, body).into_response()
}

#[derive(Template)]
#[template(path = "browser-error.html")]
struct ErrorTemplate<'a> {
    heading: &'a str,
    message: &'a str,
    retry: &'a str,
    back: &'a str,
}
