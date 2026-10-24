//! Command-line adapter that calls the running Forge daemon.

use forge_rpc::{
    AddRecordCommentRequest, AgentDispatchApiClient, AgentDispatchProfileResponse,
    AgentDispatchReconcileResponse, AgentDispatchStatusRequest, AgentDispatchStatusResponse,
    DeleteRecordCommentRequest, DoneRecordRequest, EditRecordRequest, ListManagedWorktreesResponse,
    ListRecordCommentsResponse, ListRecordUpdatesResponse, ListRecordsRequest, ListRecordsResponse,
    ManagedWorktreeResponse, OpenRecordRequest, OpenSpecRequest, OpenTaskRequest,
    OpenTaskUnderSpecRequest, OpenTaskUnderWayfinderRequest, OpenWayfinderRequest,
    ProjectApiClient, ProjectHereRequest, ProjectHereResponse, ProjectScopeRequest,
    RecordApiClient, RecordIdRequest, RecordResponse, RegisterProjectRequest,
    RegisterProjectResponse, RelateRecordRequest, SystemApiClient, SystemStatusResponse,
    WorktreeApiClient, WorktreeRootResponse,
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
/// Requests the daemon-owned dispatch endpoint configuration.
///
/// # Errors
///
/// Returns endpoint or daemon validation failures.
pub async fn request_agent_dispatch_endpoint(
    url: &str,
    socket_path: Option<String>,
) -> Result<AgentDispatchStatusResponse, String> {
    let client = client(url)?;
    match socket_path {
        Some(socket_path) => {
            client
                .endpoint_set(forge_rpc::SetAgentDispatchEndpointRequest {
                    socket_path: Some(socket_path),
                })
                .await
        }
        None => client.endpoint_get().await,
    }
    .map_err(|error| format!("could not configure Agent Dispatch through Forge daemon: {error}"))
}

/// Requests the seeded Agent Dispatch Profile.
///
/// # Errors
///
/// Returns endpoint or daemon failures.
pub async fn request_agent_dispatch_profile(
    url: &str,
) -> Result<AgentDispatchProfileResponse, String> {
    client(url)?.profile().await.map_err(|error| {
        format!("could not inspect Agent Dispatch profile through Forge daemon: {error}")
    })
}

/// Configures a registered Project's dispatch capacity.
///
/// # Errors
///
/// Returns endpoint or daemon validation failures.
pub async fn request_agent_dispatch_project(
    url: &str,
    project_slug: String,
    limit: Option<u16>,
) -> Result<AgentDispatchStatusResponse, String> {
    client(url)?
        .project_set(forge_rpc::SetAgentDispatchProjectRequest {
            project_slug,
            task_concurrency_limit: limit,
        })
        .await
        .map_err(|error| {
            format!("could not configure Agent Dispatch Project through Forge daemon: {error}")
        })
}

/// Requests stable dispatch diagnostics.
///
/// # Errors
///
/// Returns endpoint or daemon failures.
pub async fn request_agent_dispatch_status(
    url: &str,
    project_slug: Option<String>,
) -> Result<AgentDispatchStatusResponse, String> {
    client(url)?
        .dispatch_status(AgentDispatchStatusRequest { project_slug })
        .await
        .map_err(|error| {
            format!("could not inspect Agent Dispatch status through Forge daemon: {error}")
        })
}

/// Requests one daemon-owned reconciliation pass.
///
/// # Errors
///
/// Returns endpoint or daemon failures.
pub async fn request_agent_dispatch_reconcile(
    url: &str,
) -> Result<AgentDispatchReconcileResponse, String> {
    client(url)?.reconcile().await.map_err(|error| {
        format!("could not request Agent Dispatch reconciliation through Forge daemon: {error}")
    })
}

#[must_use]
pub fn render_agent_dispatch_status(status: &AgentDispatchStatusResponse) -> String {
    let endpoint = status.herdr_socket_path.as_deref().unwrap_or("unset");
    let capacity = status.project.as_ref().map_or(String::new(), |project| {
        format!(
            "\nProject `{}` capacity: {} ({})",
            project.project_slug,
            project.capacity,
            project
                .configured_capacity
                .map_or("default".into(), |limit| format!("configured: {limit}"))
        )
    });
    format!(
        "Herdr socket: {endpoint}\nEndpoint: {}{capacity}",
        status.endpoint_diagnostic
    )
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
/// Requests or configures the daemon-owned Worktree Root.
///
/// # Errors
///
/// Returns endpoint or daemon validation failures.
pub async fn request_worktree_root(
    url: &str,
    root: Option<String>,
    set: bool,
) -> Result<WorktreeRootResponse, String> {
    let client = client(url)?;
    if set {
        client
            .root_set(forge_rpc::SetWorktreeRootRequest { root })
            .await
    } else {
        client.root_get().await
    }
    .map_err(|error| format!("could not configure Worktree Root through Forge daemon: {error}"))
}
/// Configures a Project Setup Hook through the daemon.
///
/// # Errors
///
/// Returns endpoint or daemon validation failures.
pub async fn request_project_setup_hook(
    url: &str,
    project_slug: String,
    hook: Option<String>,
) -> Result<(), String> {
    client(url)?
        .setup_hook_set(forge_rpc::SetProjectSetupHookRequest { project_slug, hook })
        .await
        .map_err(|error| {
            format!("could not configure Project Setup Hook through Forge daemon: {error}")
        })
}
/// Creates a Managed Worktree through the daemon.
///
/// # Errors
///
/// Returns endpoint, lifecycle, or Git failures.
pub async fn request_managed_worktree_create(
    url: &str,
    record_id: u64,
    branch: String,
    base: String,
) -> Result<ManagedWorktreeResponse, String> {
    client(url)?
        .create(forge_rpc::CreateManagedWorktreeRequest {
            record_id,
            branch,
            base,
        })
        .await
        .map_err(record_error("create Managed Worktree"))
}
/// Inspects one Managed Worktree through the daemon.
///
/// # Errors
///
/// Returns endpoint or missing-claim failures.
pub async fn request_managed_worktree_info(
    url: &str,
    record_id: u64,
) -> Result<ManagedWorktreeResponse, String> {
    client(url)?
        .info(RecordIdRequest { id: record_id })
        .await
        .map_err(record_error("inspect Managed Worktree"))
}
/// Lists Managed Worktrees through the daemon.
///
/// # Errors
///
/// Returns endpoint or persistence failures.
pub async fn request_managed_worktree_list(
    url: &str,
    include_all: bool,
) -> Result<ListManagedWorktreesResponse, String> {
    WorktreeApiClient::list(
        &client(url)?,
        forge_rpc::ListManagedWorktreesRequest { include_all },
    )
    .await
    .map_err(record_error("list Managed Worktrees"))
}
/// Removes one Managed Worktree through the daemon.
///
/// # Errors
///
/// Returns endpoint or removal-safety failures.
pub async fn request_managed_worktree_remove(
    url: &str,
    record_id: u64,
    force: bool,
) -> Result<(), String> {
    client(url)?
        .remove(forge_rpc::RemoveManagedWorktreeRequest { record_id, force })
        .await
        .map_err(record_error("remove Managed Worktree"))
}
/// Inspects or repairs Managed Worktrees through the daemon.
///
/// # Errors
///
/// Returns endpoint or repair failures.
pub async fn request_managed_worktree_doctor(
    url: &str,
    repair: bool,
) -> Result<ListManagedWorktreesResponse, String> {
    client(url)?
        .doctor(forge_rpc::DoctorManagedWorktreesRequest { repair })
        .await
        .map_err(record_error("doctor Managed Worktrees"))
}
#[must_use]
pub fn render_managed_worktree(worktree: &ManagedWorktreeResponse) -> String {
    format!(
        "Task {} ({})\nPath: {}\nState: {}\nRemovable: {}\nFacts: {}\nBlockers: {}",
        worktree.record_id,
        worktree.project_slug,
        worktree.path,
        worktree.state,
        worktree.removable,
        worktree.facts.join(", "),
        worktree.blockers.join(", ")
    )
}
#[must_use]
pub fn render_managed_worktrees(worktrees: &ListManagedWorktreesResponse) -> String {
    worktrees
        .worktrees
        .iter()
        .map(render_managed_worktree)
        .collect::<Vec<_>>()
        .join("\n\n")
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
    format!(
        "Opened {} {} ({})",
        record.record_type, record.id, record.status
    )
}

/// Opens a Spec through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_open_spec(
    url: &str,
    body: &str,
    project_slug: String,
) -> Result<RecordResponse, String> {
    client(url)?
        .open_spec(OpenSpecRequest {
            body: body.into(),
            project_slug,
        })
        .await
        .map_err(record_error("open Spec"))
}

/// Opens a Wayfinder through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_open_wayfinder(
    url: &str,
    body: &str,
    project_slugs: Vec<String>,
) -> Result<RecordResponse, String> {
    client(url)?
        .open_wayfinder(OpenWayfinderRequest {
            body: body.into(),
            project_scope: ProjectScopeRequest { project_slugs },
        })
        .await
        .map_err(record_error("open Wayfinder"))
}

/// Opens a standalone Task through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_open_task(
    url: &str,
    body: &str,
    project_slug: Option<String>,
) -> Result<RecordResponse, String> {
    client(url)?
        .open_task(OpenTaskRequest {
            body: body.into(),
            project_slug,
        })
        .await
        .map_err(record_error("open Task"))
}

/// Opens a Task under a Spec through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_open_task_under_spec(
    url: &str,
    body: &str,
    spec_id: u64,
) -> Result<RecordResponse, String> {
    client(url)?
        .open_task_under_spec(OpenTaskUnderSpecRequest {
            body: body.into(),
            spec_id,
        })
        .await
        .map_err(record_error("open Task under Spec"))
}

/// Opens a Task under a Wayfinder through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_open_task_under_wayfinder(
    url: &str,
    body: &str,
    wayfinder_id: u64,
    project_slug: Option<String>,
) -> Result<RecordResponse, String> {
    client(url)?
        .open_task_under_wayfinder(OpenTaskUnderWayfinderRequest {
            body: body.into(),
            wayfinder_id,
            project_slug,
        })
        .await
        .map_err(record_error("open Task under Wayfinder"))
}

/// Lists Records through the daemon using optional scope, status, and type filters.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_list_records(
    url: &str,
    project_slug: Option<String>,
    status: Option<String>,
    record_type: Option<String>,
) -> Result<ListRecordsResponse, String> {
    RecordApiClient::list(
        &client(url)?,
        ListRecordsRequest {
            project_slug,
            status,
            record_type,
        },
    )
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
/// Links a directional Record Relationship through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_relate_record(
    url: &str,
    from_id: u64,
    to_id: u64,
    kind: String,
) -> Result<RecordResponse, String> {
    client(url)?
        .relate(RelateRecordRequest {
            from_id,
            to_id,
            kind,
        })
        .await
        .map_err(record_error("relate"))
}
/// Removes a directional Record Relationship through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_unrelate_record(
    url: &str,
    from_id: u64,
    to_id: u64,
    kind: String,
) -> Result<RecordResponse, String> {
    client(url)?
        .unrelate(RelateRecordRequest {
            from_id,
            to_id,
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
pub async fn request_add_record_comment(
    url: &str,
    record_id: u64,
    body: &str,
    author: Option<String>,
) -> Result<forge_rpc::RecordCommentResponse, String> {
    client(url)?
        .comment(AddRecordCommentRequest {
            record_id,
            body: body.into(),
            author,
        })
        .await
        .map_err(record_error("comment"))
}
/// # Errors
/// Returns an endpoint or daemon validation error.
pub async fn request_record_comments(
    url: &str,
    id: u64,
) -> Result<ListRecordCommentsResponse, String> {
    client(url)?
        .comments(RecordIdRequest { id })
        .await
        .map_err(record_error("comments"))
}
/// # Errors
/// Returns an endpoint or daemon validation error.
pub async fn request_delete_record_comment(
    url: &str,
    record_id: u64,
    comment_id: u64,
) -> Result<(), String> {
    client(url)?
        .comment_delete(DeleteRecordCommentRequest {
            record_id,
            comment_id,
        })
        .await
        .map_err(record_error("comment delete"))
}
/// # Errors
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
        "{} {} ({})\n\n{}",
        record.record_type, record.id, record.status, record.body
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
pub fn render_record_comments(comments: &ListRecordCommentsResponse) -> String {
    comments
        .comments
        .iter()
        .map(|comment| {
            format!(
                "Comment {} {}{}\n{}",
                comment.id,
                comment.created_at_millis,
                comment
                    .author
                    .as_deref()
                    .map_or(String::new(), |author| format!(" {author}")),
                comment.body
            )
        })
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
                update.related_record_id.or(update.comment_id),
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
    use forge_app::{Application, Project, RegistrationError, RepositoryInspector};
    use forge_rpc::{ListRecordUpdatesResponse, RecordResponse, RecordUpdateResponse};
    use forge_server::{Daemon, router};
    use forge_store::SqliteProjectRepository;
    use std::path::{Path, PathBuf};

    struct TestInspector;

    impl RepositoryInspector for TestInspector {
        fn inspect(&self, folder: &Path) -> Result<Project, RegistrationError> {
            Ok(Project {
                slug: String::new(),
                registered_folder: PathBuf::from(folder),
                origin_url: "https://example.test/origin.git".into(),
            })
        }
    }

    fn test_application() -> Application {
        Application::new(
            Box::new(SqliteProjectRepository::in_memory().unwrap()),
            Box::new(TestInspector),
        )
    }
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

    #[tokio::test]
    async fn typed_cli_requests_round_trip_through_the_daemon() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router(Daemon::new(test_application())))
                .await
                .unwrap();
        });
        let url = format!("http://{address}/rpc");
        let folder = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        request_register_project(&url, &folder, "cli-test-project")
            .await
            .unwrap();

        let spec = request_open_spec(&url, "Spec body", "cli-test-project".into())
            .await
            .unwrap();
        assert_eq!(spec.record_type, "spec");
        assert_eq!(spec.project_scope.project_slugs, ["cli-test-project"]);
        let wayfinder =
            request_open_wayfinder(&url, "Wayfinder body", vec!["cli-test-project".into()])
                .await
                .unwrap();
        assert_eq!(wayfinder.record_type, "wayfinder");
        let wayfinder_task =
            request_open_task_under_wayfinder(&url, "Scoped Child Task", wayfinder.id, None)
                .await
                .unwrap();
        assert_eq!(wayfinder_task.project_scope, wayfinder.project_scope);

        let standalone = request_open_task(&url, "Standalone Task", None)
            .await
            .unwrap();
        assert_eq!(standalone.record_type, "task");
        assert!(standalone.project_scope.project_slugs.is_empty());
        let child = request_open_task_under_spec(&url, "Child Task", spec.id)
            .await
            .unwrap();
        assert_eq!(child.record_type, "task");
        assert_eq!(child.project_scope, spec.project_scope);
        assert_eq!(child.relationships[0].parent_id, spec.id);

        let listed = request_list_records(&url, None, None, Some("task".into()))
            .await
            .unwrap();
        assert_eq!(listed.records.len(), 3);
        assert!(
            listed
                .records
                .iter()
                .all(|record| record.record_type == "task")
        );
        assert_eq!(render_record(&spec), "spec 1 (open)\n\nSpec body");
        assert_eq!(render_record_opened(&child), "Opened task 5 (open)");

        server.abort();
    }

    #[test]
    fn record_list_renders_records_in_response_order() {
        let records = ListRecordsResponse {
            records: vec![RecordResponse {
                id: 4,
                record_type: "generic".into(),
                body: "body".into(),
                project_scope: ProjectScopeRequest {
                    project_slugs: vec![],
                },
                status: "open".into(),
                completion_resolution: None,
                relationships: vec![],
            }],
        };
        assert_eq!(render_record_list(&records), "generic 4 (open)\n\nbody");
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
                    comment_id: None,
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
                record_type: "generic".into(),
                body: "body".into(),
                project_scope: ProjectScopeRequest {
                    project_slugs: vec![]
                },
                status: "open".into(),
                completion_resolution: None,
                relationships: vec![],
            }),
            "Opened generic 4 (open)"
        );
    }
}
