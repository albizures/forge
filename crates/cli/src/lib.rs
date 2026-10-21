//! Command-line adapter that calls the running Forge daemon.

use forge_rpc::{
    OpenRecordRequest, ProjectApiClient, ProjectHereRequest, ProjectHereResponse,
    ProjectScopeRequest, RecordApiClient, RecordResponse, RegisterProjectRequest,
    RegisterProjectResponse, SystemApiClient, SystemStatusResponse,
};
use jsonrpsee::http_client::HttpClientBuilder;
use std::{env, time::Duration};
pub const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:45003/rpc";
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[must_use]
pub fn server_url(flag: Option<&str>, environment: Option<&str>) -> String {
    flag.or(environment)
        .unwrap_or(DEFAULT_SERVER_URL)
        .to_owned()
}
#[must_use]
pub fn server_url_from_process(flag: Option<&str>) -> String {
    server_url(flag, env::var("FORGE_SERVER_URL").ok().as_deref())
}
fn client(url: &str) -> Result<jsonrpsee::http_client::HttpClient, String> {
    HttpClientBuilder::default()
        .request_timeout(REQUEST_TIMEOUT)
        .build(url)
        .map_err(|error| format!("could not configure Forge daemon endpoint `{url}`: {error}"))
}

/// Requests Project registration through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon error.
pub async fn request_register_project(
    url: &str,
    folder: &str,
    slug: &str,
) -> Result<RegisterProjectResponse, String> {
    client(url)?
        .register(RegisterProjectRequest {
            folder: folder.into(),
            slug: slug.into(),
        })
        .await
        .map_err(|error| format!("could not register Project through Forge daemon: {error}"))
}
#[must_use]
pub fn render_project_registered(project: &RegisterProjectResponse) -> String {
    format!(
        "Registered Project `{}` at `{}`. Run `forge here` to confirm it.",
        project.slug, project.registered_folder
    )
}
/// Requests the Project containing `folder` through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon error.
pub async fn request_project_here(url: &str, folder: &str) -> Result<ProjectHereResponse, String> {
    client(url)?
        .here(ProjectHereRequest {
            folder: folder.into(),
        })
        .await
        .map_err(|error| format!("could not find Project through Forge daemon: {error}"))
}
#[must_use]
pub fn render_project_here(project: &ProjectHereResponse) -> String {
    format!(
        "Project `{}` is registered at `{}` (origin: `{}`).",
        project.slug, project.registered_folder, project.origin_url
    )
}
/// Opens a generic Record through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_open_record(
    url: &str,
    body: &str,
    project_slugs: Vec<String>,
) -> Result<RecordResponse, String> {
    client(url)?
        .open(OpenRecordRequest {
            body: body.into(),
            project_scope: ProjectScopeRequest { project_slugs },
        })
        .await
        .map_err(|error| format!("could not open Record through Forge daemon: {error}"))
}
#[must_use]
pub fn render_record_opened(record: &RecordResponse) -> String {
    format!("Opened Record {} ({})", record.id, record.status)
}
/// Requests daemon status.
///
/// # Errors
///
/// Returns an endpoint or daemon error.
pub async fn request_status(url: &str) -> Result<SystemStatusResponse, String> {
    client(url)?.status().await.map_err(|error| {
        format!("could not reach Forge daemon at `{url}` within 10 seconds: {error}")
    })
}
#[must_use]
pub fn render_human_status(status: &SystemStatusResponse) -> String {
    format!(
        "{} {} ({})",
        status.name,
        status.version,
        if status.ready { "ready" } else { "not ready" }
    )
}
/// Renders the stable JSON representation of daemon status.
///
/// # Panics
///
/// Panics only if serializing the fixed response type unexpectedly fails.
#[must_use]
pub fn render_json_status(status: &SystemStatusResponse) -> String {
    serde_json::to_string(status).expect("SystemStatusResponse must serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_rpc::RecordResponse;
    #[test]
    fn endpoint_configuration_prioritizes_flag_then_environment_then_default() {
        assert_eq!(
            server_url(Some("http://flag/rpc"), Some("http://environment/rpc")),
            "http://flag/rpc"
        );
        assert_eq!(
            server_url(None, Some("http://environment/rpc")),
            "http://environment/rpc"
        );
        assert_eq!(server_url(None, None), DEFAULT_SERVER_URL);
    }
    #[test]
    fn record_output_identifies_the_new_record() {
        assert_eq!(
            render_record_opened(&RecordResponse {
                id: 4,
                body: "body".into(),
                project_scope: ProjectScopeRequest {
                    project_slugs: vec![]
                },
                status: "open".into()
            }),
            "Opened Record 4 (open)"
        );
    }
}
