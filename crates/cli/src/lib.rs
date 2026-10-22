//! Command-line adapter that calls the running Forge daemon.

use forge_rpc::{
    DoneRecordRequest, EditRecordRequest, ListRecordUpdatesResponse, ListRecordsRequest,
    ListRecordsResponse, OpenRecordRequest, ProjectApiClient, ProjectHereRequest,
    ProjectHereResponse, ProjectScopeRequest, RecordApiClient, RecordIdRequest, RecordResponse,
    RegisterProjectRequest, RegisterProjectResponse, RelateRecordRequest, SystemApiClient,
    SystemStatusResponse,
};
use jsonrpsee::http_client::HttpClientBuilder;
use sha2::{Digest, Sha256};
use std::{env, fs, time::Duration};
pub const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:45003/rpc";
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[must_use]
pub fn server_url(flag: Option<&str>, environment: Option<&str>) -> String {
    flag.or(environment)
        .unwrap_or(DEFAULT_SERVER_URL)
        .to_owned()
}
/// Resolves the endpoint for one ordinary CLI command.
///
/// An explicit flag or environment value is an Endpoint Override. It returns
/// immediately, so managed endpoint resolution added here remains entirely
/// below the no-override branch.
///
/// # Errors
///
/// Returns an actionable error when the active Revision cannot be read,
/// attested, or started.
#[allow(clippy::unused_async)] // Kept async for uniform command endpoint resolution.
pub async fn resolve_endpoint(flag: Option<&str>) -> Result<String, String> {
    if let Some(endpoint) = flag.or(env::var("FORGE_SERVER_URL").ok().as_deref()) {
        return Ok(endpoint.to_owned());
    }

    let installation = forgeup::active_installation().map_err(|error| managed_error(&error))?;
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not identify this Forge CLI executable: {error}"))?;
    let hash = sha256(&executable)?;
    let identity = forge_build_info::current();
    if hash != installation.cli_sha256
        || identity.commit != installation.identity.commit
        || identity.dirty != installation.identity.dirty
        || identity.management_protocol_revision
            != installation.identity.management_protocol_revision
        || identity.version != installation.identity.version
    {
        return Err(format!(
            "this Forge CLI is not the active Revision `{}`; run `forgeup use {}` or provide --server-url / FORGE_SERVER_URL",
            installation.selector, installation.selector
        ));
    }

    forgeup::ensure_active_daemon().map_err(|error| managed_error(&error))?;
    Ok(format!("http://{}/rpc", installation.endpoint))
}

fn sha256(path: &std::path::Path) -> Result<String, String> {
    let contents = fs::read(path).map_err(|error| {
        format!(
            "could not hash Forge CLI executable `{}`: {error}",
            path.display()
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(contents)))
}

fn managed_error(error: &forgeup::ForgeupError) -> String {
    format!(
        "could not resolve the active Forge Revision: {error}. Run `forgeup use <selector>` or provide --server-url / FORGE_SERVER_URL"
    )
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

/// Lists Records through the daemon using optional scope and status filters.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_list_records(
    url: &str,
    project_slug: Option<String>,
    status: Option<String>,
) -> Result<ListRecordsResponse, String> {
    client(url)?
        .list(ListRecordsRequest {
            project_slug,
            status,
        })
        .await
        .map_err(record_error("list"))
}

/// Requests a Record lifecycle operation through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_show_record(url: &str, id: u64) -> Result<RecordResponse, String> {
    client(url)?
        .show(RecordIdRequest { id })
        .await
        .map_err(record_error("show"))
}
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_edit_record(url: &str, id: u64, body: &str) -> Result<RecordResponse, String> {
    client(url)?
        .edit(EditRecordRequest {
            id,
            body: body.into(),
        })
        .await
        .map_err(record_error("edit"))
}
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_start_record(url: &str, id: u64) -> Result<RecordResponse, String> {
    client(url)?
        .start(RecordIdRequest { id })
        .await
        .map_err(record_error("start"))
}
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_done_record(
    url: &str,
    id: u64,
    completion_resolution: Option<String>,
) -> Result<RecordResponse, String> {
    client(url)?
        .done(DoneRecordRequest {
            id,
            completion_resolution,
        })
        .await
        .map_err(record_error("done"))
}
/// Links a child Record to a parent Record through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_relate_record(
    url: &str,
    child_id: u64,
    parent_id: u64,
    kind: String,
) -> Result<RecordResponse, String> {
    client(url)?
        .relate(RelateRecordRequest {
            child_id,
            parent_id,
            kind,
        })
        .await
        .map_err(record_error("relate"))
}
/// Removes a child-to-parent Relationship through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_unrelate_record(
    url: &str,
    child_id: u64,
    parent_id: u64,
    kind: String,
) -> Result<RecordResponse, String> {
    client(url)?
        .unrelate(RelateRecordRequest {
            child_id,
            parent_id,
            kind,
        })
        .await
        .map_err(record_error("unrelate"))
}
/// Requests a Record's Forge-authored Update Log through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_record_updates(
    url: &str,
    id: u64,
) -> Result<ListRecordUpdatesResponse, String> {
    client(url)?
        .updates(RecordIdRequest { id })
        .await
        .map_err(record_error("updates"))
}
fn record_error(operation: &'static str) -> impl FnOnce(jsonrpsee::core::client::Error) -> String {
    move |error| format!("could not {operation} Record through Forge daemon: {error}")
}
#[must_use]
pub fn render_record(record: &RecordResponse) -> String {
    format!(
        "Record {} ({})\n\n{}",
        record.id, record.status, record.body
    )
}
#[must_use]
pub fn render_record_list(records: &ListRecordsResponse) -> String {
    records
        .records
        .iter()
        .map(render_record)
        .collect::<Vec<_>>()
        .join("\n\n")
}
#[must_use]
pub fn render_record_updates(updates: &ListRecordUpdatesResponse) -> String {
    updates
        .updates
        .iter()
        .map(|update| {
            let detail = match (
                &update.from_status,
                &update.to_status,
                update.related_record_id,
            ) {
                (Some(from), Some(to), _) => format!(" {from} -> {to}"),
                (_, _, Some(id)) => format!(" Record {id}"),
                _ => String::new(),
            };
            format!(
                "Update {} {} {}{}",
                update.id, update.created_at_millis, update.kind, detail
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    use forge_rpc::{ListRecordUpdatesResponse, RecordResponse, RecordUpdateResponse};
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
    #[tokio::test]
    async fn endpoint_override_resolves_without_managed_endpoint_work() {
        assert_eq!(
            resolve_endpoint(Some("http://manual-daemon/rpc"))
                .await
                .unwrap(),
            "http://manual-daemon/rpc"
        );
    }

    #[test]
    fn record_list_renders_records_in_response_order() {
        let records = ListRecordsResponse {
            records: vec![RecordResponse {
                id: 4,
                body: "body".into(),
                project_scope: ProjectScopeRequest {
                    project_slugs: vec![],
                },
                status: "open".into(),
                completion_resolution: None,
                relationships: vec![],
            }],
        };
        assert_eq!(render_record_list(&records), "Record 4 (open)\n\nbody");
    }

    #[test]
    fn update_log_rendering_keeps_response_order_and_omits_content() {
        assert_eq!(
            render_record_updates(&ListRecordUpdatesResponse {
                updates: vec![RecordUpdateResponse {
                    id: 7,
                    created_at_millis: 42,
                    kind: "status-transition".into(),
                    from_status: Some("open".into()),
                    to_status: Some("in_progress".into()),
                    related_record_id: None,
                }],
            }),
            "Update 7 42 status-transition open -> in_progress"
        );
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
                status: "open".into(),
                completion_resolution: None,
                relationships: vec![],
            }),
            "Opened Record 4 (open)"
        );
    }
}
