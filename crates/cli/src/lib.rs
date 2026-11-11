//! Command-line adapter that calls the running Forge daemon.

mod record_selection;
pub use record_selection::RecordEndpoint;
use record_selection::record_client;

use forge_rpc::{
    AddRecordCommentRequest, AgentDispatchApiClient, AgentDispatchProfileResponse,
    AgentDispatchReconcileResponse, AgentDispatchStatusRequest, AgentDispatchStatusResponse,
    DeleteRecordCommentRequest, DispatchRecordRequest, DispatchRecordResponse, DoneRecordRequest,
    EditRecordRequest, ListManagedWorktreesResponse, ListRecordCommentsResponse,
    ListRecordUpdatesResponse, ListRecordsRequest, ListRecordsResponse, ManagedWorktreeResponse,
    OpenGrillingUnderWayfinderRequest, OpenRecordRequest, OpenSpecRequest, OpenTaskRequest,
    OpenTaskUnderSpecRequest, OpenTaskUnderWayfinderRequest, OpenWayfinderRequest,
    ProjectApiClient, ProjectHereRequest, ProjectHereResponse, ProjectScopeRequest,
    RecordIdRequest, RecordResponse, RegisterProjectRequest, RegisterProjectResponse,
    RelateRecordRequest, RetireAgentAssignmentRequest, StartAssignmentTaskRequest, SystemApiClient,
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
fn read_assignment_secret(file: std::ffi::OsString) -> Result<String, String> {
    use std::io::Read;
    let assignment = env::var("FORGE_ASSIGNMENT_ID").map_err(|_| "missing Assignment ID")?;
    if assignment.is_empty()
        || !assignment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("invalid Assignment ID".into());
    }
    let endpoint =
        env::var("FORGE_ASSIGNMENT_DAEMON_URL").map_err(|_| "missing Assignment daemon URL")?;
    let uri: hyper::Uri = endpoint
        .parse()
        .map_err(|_| "invalid Assignment daemon URL")?;
    let authority = uri.authority().ok_or("invalid Assignment daemon URL")?;
    let host = uri
        .host()
        .ok_or("invalid Assignment daemon URL")?
        .trim_matches(['[', ']']);
    if uri.scheme_str() != Some("http")
        || uri.path() != "/"
        || uri.query().is_some()
        || authority.as_str().contains('@')
    {
        return Err("invalid Assignment daemon URL".into());
    }
    if host != "localhost"
        && !host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    {
        return Err("Assignment daemon must be loopback".into());
    }
    let path = std::path::PathBuf::from(file);
    if !path.is_absolute() {
        return Err("Assignment secret file must be protected".into());
    }
    #[cfg(unix)]
    let mut file = std::fs::File::from(
        rustix::fs::open(
            &path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| "Assignment secret file must be protected")?,
    );
    #[cfg(not(unix))]
    let mut file = fs::File::open(path).map_err(|_| "could not read Assignment secret file")?;
    let metadata = file
        .metadata()
        .map_err(|_| "could not read Assignment secret file")?;
    if !metadata.is_file() {
        return Err("Assignment secret file must be protected".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err("Assignment secret file must be protected".into());
        }
    }
    let mut secret = String::new();
    file.read_to_string(&mut secret)
        .map_err(|_| "could not read Assignment secret file")?;
    let secret = secret.trim_end_matches('\n').to_owned();
    if secret.is_empty() {
        return Err("empty Assignment secret".into());
    }
    Ok(secret)
}

/// Complete only the Record bound to the delivered Assignment.
#[allow(clippy::missing_errors_doc)]
pub async fn assignment_done(
    resolution: Option<String>,
    human: Option<(String, String)>,
) -> Result<serde_json::Value, String> {
    use jsonrpsee::{core::client::ClientT, rpc_params};
    let assignment_id = env::var("FORGE_ASSIGNMENT_ID").map_err(|_| "missing Assignment ID")?;
    let endpoint =
        env::var("FORGE_ASSIGNMENT_DAEMON_URL").map_err(|_| "missing Assignment daemon URL")?;
    let file = env::var_os("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE")
        .ok_or("missing Assignment secret file")?;
    let secret = read_assignment_secret(file)?;
    let client = client(&format!("{}/rpc", endpoint.trim_end_matches('/')))
        .map_err(|_| "invalid Assignment daemon URL")?;
    let human = human.map(|(confirmation, agreed_outcome)| serde_json::json!({"confirmed": true, "confirmation": confirmation, "agreedOutcome": agreed_outcome}));
    client.request("assignment_done", rpc_params![serde_json::json!({"assignmentId": assignment_id, "secret": secret, "resolution": resolution, "human": human})]).await.map_err(|_| "Assignment completion failed".into())
}

/// Add a comment to only the delivered Assignment's Record.
#[allow(clippy::missing_errors_doc)]
pub async fn assignment_comment(body: &str) -> Result<serde_json::Value, String> {
    use jsonrpsee::{core::client::ClientT, rpc_params};
    if body.trim().is_empty() {
        return Err("comment must not be blank".into());
    }
    let assignment_id = env::var("FORGE_ASSIGNMENT_ID").map_err(|_| "missing Assignment ID")?;
    let endpoint =
        env::var("FORGE_ASSIGNMENT_DAEMON_URL").map_err(|_| "missing Assignment daemon URL")?;
    let file = env::var_os("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE")
        .ok_or("missing Assignment secret file")?;
    let secret = read_assignment_secret(file)?;
    let client = client(&format!("{}/rpc", endpoint.trim_end_matches('/')))
        .map_err(|_| "invalid Assignment daemon URL")?;
    client
        .request(
            "assignment_comment",
            rpc_params![
                serde_json::json!({"assignmentId": assignment_id, "secret": secret, "body": body})
            ],
        )
        .await
        .map_err(|_| "Assignment comment failed".into())
}

/// Start only the Record bound to the delivered Assignment.
#[allow(clippy::missing_errors_doc)]
pub async fn assignment_start() -> Result<serde_json::Value, String> {
    use jsonrpsee::{core::client::ClientT, rpc_params};
    let assignment_id = env::var("FORGE_ASSIGNMENT_ID").map_err(|_| "missing Assignment ID")?;
    let endpoint =
        env::var("FORGE_ASSIGNMENT_DAEMON_URL").map_err(|_| "missing Assignment daemon URL")?;
    let file = env::var_os("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE")
        .ok_or("missing Assignment secret file")?;
    let secret = read_assignment_secret(file)?;
    let client = client(&format!("{}/rpc", endpoint.trim_end_matches('/')))
        .map_err(|_| "invalid Assignment daemon URL")?;
    client
        .request(
            "assignment_start",
            rpc_params![serde_json::json!({"assignmentId": assignment_id, "secret": secret})],
        )
        .await
        .map_err(|_| "Assignment start failed".into())
}

/// Fetch a bounded Assignment comment or history page.
#[allow(clippy::missing_errors_doc)]
pub async fn assignment_page(
    target: u64,
    limit: u32,
    cursor: Option<String>,
    history: bool,
) -> Result<serde_json::Value, String> {
    use jsonrpsee::{core::client::ClientT, rpc_params};
    if !(1..=100).contains(&limit) {
        return Err("page limit must be between 1 and 100".into());
    }
    let assignment_id = env::var("FORGE_ASSIGNMENT_ID").map_err(|_| "missing Assignment ID")?;
    let endpoint =
        env::var("FORGE_ASSIGNMENT_DAEMON_URL").map_err(|_| "missing Assignment daemon URL")?;
    let secret_file = env::var_os("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE")
        .ok_or("missing Assignment secret file")?;
    let secret = read_assignment_secret(secret_file)?;
    let client = client(&format!("{}/rpc", endpoint.trim_end_matches('/')))
        .map_err(|_| "invalid Assignment daemon URL")?;
    let method = if history {
        "assignment_history"
    } else {
        "assignment_comments"
    };
    client.request(method, rpc_params![serde_json::json!({ "assignmentId": assignment_id, "secret": secret, "target": target, "limit": limit, "cursor": cursor })]).await.map_err(|_| "Assignment page request failed".into())
}

/// Perform an Assignment read using the protected delivery contract.
#[allow(clippy::missing_errors_doc)]
pub async fn assignment_record(target: u64) -> Result<serde_json::Value, String> {
    use jsonrpsee::{core::client::ClientT, rpc_params};
    let assignment_id = env::var("FORGE_ASSIGNMENT_ID").map_err(|_| "missing Assignment ID")?;
    let endpoint =
        env::var("FORGE_ASSIGNMENT_DAEMON_URL").map_err(|_| "missing Assignment daemon URL")?;
    let secret_file = env::var_os("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE")
        .ok_or("missing Assignment secret file")?;
    let secret = read_assignment_secret(secret_file)?;
    let client = client(&format!("{}/rpc", endpoint.trim_end_matches('/')))
        .map_err(|_| "invalid Assignment daemon URL")?;
    client
        .request(
            "assignment_record",
            rpc_params![serde_json::json!({
                "assignmentId": assignment_id, "secret": secret, "target": target
            })],
        )
        .await
        .map_err(|_| "Assignment record request failed".into())
}

/// Read the current Assignment context using protected delivery.
#[allow(clippy::missing_errors_doc)]
pub async fn assignment_context() -> Result<forge_rpc::AssignmentContextResponse, String> {
    use forge_rpc::AssignmentApiClient;
    let assignment_id = env::var("FORGE_ASSIGNMENT_ID").map_err(|_| "missing Assignment ID")?;
    let endpoint =
        env::var("FORGE_ASSIGNMENT_DAEMON_URL").map_err(|_| "missing Assignment daemon URL")?;
    let secret_file = env::var_os("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE")
        .ok_or("missing Assignment secret file")?;
    let secret = read_assignment_secret(secret_file)?;
    let url = format!("{}/rpc", endpoint.trim_end_matches('/'));
    let client = client(&url).map_err(|_| "invalid Assignment daemon URL")?;
    client
        .context(forge_rpc::AssignmentCredentials {
            assignment_id,
            secret,
        })
        .await
        .map_err(|_| "Assignment context request failed".into())
}

/// Authenticate and bootstrap the delivered Assignment through the unified server route.
#[allow(clippy::missing_errors_doc)]
pub async fn assignment_bootstrap() -> Result<forge_rpc::AssignmentBootstrapResponse, String> {
    use forge_rpc::AssignmentApiClient;
    let assignment_id = env::var("FORGE_ASSIGNMENT_ID").map_err(|_| "missing Assignment ID")?;
    let endpoint =
        env::var("FORGE_ASSIGNMENT_DAEMON_URL").map_err(|_| "missing Assignment daemon URL")?;
    let file = env::var_os("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE")
        .ok_or("missing Assignment secret file")?;
    let secret = read_assignment_secret(file)?;
    let client = client(&format!("{}/rpc", endpoint.trim_end_matches('/')))
        .map_err(|_| "invalid Assignment daemon URL")?;
    client
        .bootstrap(forge_rpc::AssignmentCredentials {
            assignment_id,
            secret,
        })
        .await
        .map_err(|_| "Assignment bootstrap failed or is uncertain; do not begin work".into())
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
    request_register_project_selected(url, folder, slug, None).await
}

/// Registers with explicit Workspace routing; cwd is always supplied by the client.
/// # Errors
/// Returns missing cwd, conflicting selectors or daemon validation failures.
pub async fn request_register_project_selected(
    url: &str,
    folder: &str,
    slug: &str,
    workspace: Option<String>,
) -> Result<RegisterProjectResponse, String> {
    client(url)?
        .register(RegisterProjectRequest {
            folder: folder.into(),
            slug: slug.into(),
            workspace,
            cwd: std::env::current_dir()
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .into_owned(),
        })
        .await
        .map_err(|error| format!("could not register Project through Forge daemon: {error}"))
}
/// Reads effective Dispatcher configuration through the daemon.
///
/// # Errors
/// Returns endpoint, key validation, or daemon failures.
pub async fn request_dispatcher_config_get(
    endpoint: impl Into<RecordEndpoint>,
    key: Option<String>,
    project: Option<String>,
) -> Result<forge_rpc::DispatcherConfigResponse, String> {
    record_client(endpoint)?
        .config_get(forge_rpc::DispatcherConfigGetRequest { key, project })
        .await
        .map_err(|error| format!("could not read Dispatcher configuration: {error}"))
}

/// Sets an explicit Dispatcher configuration value through the daemon.
///
/// # Errors
/// Returns endpoint, validation, provider verification, or resource-safety failures.
pub async fn request_dispatcher_config_set(
    endpoint: impl Into<RecordEndpoint>,
    key: String,
    value: String,
    project: Option<String>,
) -> Result<forge_rpc::DispatcherConfigResponse, String> {
    record_client(endpoint)?
        .config_set(forge_rpc::DispatcherConfigSetRequest {
            key,
            value,
            project,
        })
        .await
        .map_err(|error| format!("could not set Dispatcher configuration: {error}"))
}

/// Explicitly unsets a Dispatcher override through the daemon.
///
/// # Errors
/// Returns endpoint, validation, or resource-safety failures.
pub async fn request_dispatcher_config_unset(
    endpoint: impl Into<RecordEndpoint>,
    key: String,
    project: Option<String>,
) -> Result<forge_rpc::DispatcherConfigResponse, String> {
    record_client(endpoint)?
        .config_unset(forge_rpc::DispatcherConfigUnsetRequest { key, project })
        .await
        .map_err(|error| format!("could not unset Dispatcher configuration: {error}"))
}

/// Requests the Workspace-selected immutable Task or Grilling profile.
///
/// # Errors
///
/// Returns endpoint or daemon failures.
pub async fn request_agent_dispatch_profile(
    endpoint: impl Into<RecordEndpoint>,
    kind: String,
) -> Result<AgentDispatchProfileResponse, String> {
    record_client(endpoint)?
        .profile(forge_rpc::AgentDispatchProfileRequest { kind })
        .await
        .map_err(|error| {
            format!("could not inspect Agent Dispatch profile through Forge daemon: {error}")
        })
}

/// Requests Workspace configuration completeness and live provider diagnostics.
///
/// # Errors
///
/// Returns endpoint or daemon failures.
pub async fn request_agent_dispatch_status(
    endpoint: impl Into<RecordEndpoint>,
    project_slug: Option<String>,
) -> Result<AgentDispatchStatusResponse, String> {
    record_client(endpoint)?
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

/// Explicitly dispatches a Task or interactive Grilling Assignment.
///
/// # Errors
/// Returns endpoint, eligibility, or daemon provisioning failures.
pub async fn request_dispatch_record(
    endpoint: impl Into<RecordEndpoint>,
    record_id: u64,
) -> Result<DispatchRecordResponse, String> {
    record_client(endpoint)?
        .dispatch_record(DispatchRecordRequest { record_id })
        .await
        .map_err(|error| format!("could not dispatch Record through Forge daemon: {error}"))
}

/// Starts the Task bound to an Assignment Capability.
///
/// # Errors
/// Returns endpoint, authorization, or Task-start failures from the daemon.
pub async fn request_start_assignment_task(
    url: &str,
    assignment_id: String,
    capability_id: String,
    correlation_id: String,
    task_id: u64,
) -> Result<RecordResponse, String> {
    client(url)?
        .task_start(StartAssignmentTaskRequest {
            assignment_id,
            capability_id,
            correlation_id,
            task_id,
        })
        .await
        .map_err(|error| format!("could not start Assignment Task through Forge daemon: {error}"))
}

/// Returns an unassigned Task to Open.
///
/// # Errors
/// Returns endpoint or daemon Task-reopen failures.
pub async fn request_reopen_dispatch_task(
    url: &str,
    task_id: u64,
) -> Result<RecordResponse, String> {
    client(url)?
        .task_reopen(RecordIdRequest { id: task_id })
        .await
        .map_err(|error| format!("could not reopen Task through Forge daemon: {error}"))
}

/// Clears a Task's Dispatch Hold.
///
/// # Errors
/// Returns endpoint or daemon hold-clearing failures.
pub async fn request_resume_dispatch_task(url: &str, task_id: u64) -> Result<(), String> {
    client(url)?
        .resume(RecordIdRequest { id: task_id })
        .await
        .map_err(|error| format!("could not resume Task dispatch through Forge daemon: {error}"))
}

/// Inspects retained Assignment evidence through the operator daemon interface.
///
/// # Errors
/// Returns a user-facing error when the daemon is unreachable or inspection fails.
pub async fn request_agent_assignment_info(
    url: &str,
    assignment_id: String,
) -> Result<forge_rpc::AgentAssignmentInfoResponse, String> {
    client(url)?
        .assignment_info(forge_rpc::AssignmentInfoRequest { assignment_id })
        .await
        .map_err(|error| {
            format!("could not inspect Agent Assignment through Forge daemon: {error}")
        })
}

/// Renders persisted facts without claiming provider liveness or safe cleanup.
#[must_use]
pub fn render_agent_assignment_info(info: &forge_rpc::AgentAssignmentInfoResponse) -> String {
    use std::fmt::Write;
    let timestamp =
        |value: Option<u64>| value.map_or_else(|| "not recorded".into(), |time| time.to_string());
    let acknowledged = info.acknowledged_at_millis.map_or_else(
        || "not acknowledged".into(),
        |time| format!("acknowledged at {time}"),
    );
    let mut output = format!(
        "Assignment {} ({})\nTask {} ({}) — Project {}\nProvider: {} — Attempt {}\nClaimed: {} — State changed: {}\nClosed: {} — Closure reason: {}\nLaunch started: {}\nAcknowledgement: {}\nAcknowledgement deadline: {}\nDispatch Hold: {}\nCapacity reserved: {}\nRecovery blockers: {}\nResources (persisted evidence, not live provider verification):",
        info.id,
        info.state,
        info.record_id,
        info.task_status,
        info.project_slug,
        info.provider,
        info.attempt_number,
        info.claimed_at_millis,
        info.state_changed_at_millis,
        timestamp(info.closed_at_millis),
        info.closure_reason.as_deref().unwrap_or("none"),
        timestamp(info.launch_started_at_millis),
        acknowledged,
        timestamp(info.acknowledgement_deadline_millis),
        info.dispatch_hold.as_deref().unwrap_or("none"),
        info.capacity_reserved,
        if info.recovery_blockers.is_empty() {
            "none".into()
        } else {
            info.recovery_blockers.join(", ")
        },
    );
    if info.resources.is_empty() {
        output.push_str("\n  none recorded (not proof that no agent exists)");
    }
    for resource in &info.resources {
        let retirement = resource
            .retired_at_millis
            .map_or_else(|| "unretired".into(), |time| format!("retired at {time}"));
        write!(
            output,
            "\n  {} {} — created at {} — {}",
            resource.resource_kind, resource.resource_id, resource.created_at_millis, retirement
        )
        .unwrap();
    }
    if let Some(resolution) = &info.unknown_resource_resolution {
        write!(
            output,
            "\nUnknown-resource resolution at {} ({}): {}",
            resolution.confirmed_at_millis, resolution.authority, resolution.confirmation
        )
        .unwrap();
    }
    output.push_str("\nTimestamps are Unix milliseconds.\nOperations:\n");
    output.push_str(&info.operations.join("\n"));
    output.push_str("\nClearing a Dispatch Hold does not resolve recovery blockers or establish that an agent is absent. Investigate resources before retirement; hold clearing is separate.");
    output
}

/// Retires an Assignment and its provider resources.
///
/// # Errors
/// Returns endpoint or daemon retirement failures, including unsafe cleanup.
pub async fn request_retire_agent_assignment(
    url: &str,
    assignment_id: String,
) -> Result<(), String> {
    client(url)?
        .assignment_retire(RetireAgentAssignmentRequest { assignment_id })
        .await
        .map_err(|error| format!("could not retire Agent Assignment through Forge daemon: {error}"))
}

/// Sends the operator's two explicit safety assertions (never an agent credential).
///
/// # Errors
/// Returns transport, confirmation, revocation, or persistence errors.
pub async fn request_resolve_unknown_agent_resources(
    url: &str,
    assignment_id: String,
    confirm_old_agent_cannot_act: bool,
    confirm_unknown_resources_safe: bool,
) -> Result<forge_rpc::AgentAssignmentInfoResponse, String> {
    client(url)?
        .assignment_resolve_unknown(forge_rpc::ResolveUnknownAgentResourcesRequest {
            assignment_id,
            confirm_old_agent_cannot_act,
            confirm_unknown_resources_safe,
        })
        .await
        .map_err(|error| {
            format!("could not resolve unknown resources through Forge daemon: {error}")
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
    let blockers = status
        .blockers
        .iter()
        .fold(String::new(), |mut text, blocker| {
            use std::fmt::Write;
            write!(text, "\n- {}: {}", blocker.code, blocker.message)
                .expect("writing a String cannot fail");
            text
        });
    format!(
        "Workspace: {}\nConfigured: {}\nDispatchable now: {}\nHerdr socket (daemon): {endpoint}\nEndpoint: {}{capacity}{blockers}",
        status.workspace_id, status.configured, status.dispatchable, status.endpoint_diagnostic
    )
}

#[must_use]
pub fn render_project_registered(project: &RegisterProjectResponse) -> String {
    format!(
        "Registered Project `{}` at `{}`. Run `forge here` to confirm it.",
        project.reference, project.project_root
    )
}
/// Requests the Project containing `folder` through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon error.
pub async fn request_project_here(url: &str, folder: &str) -> Result<ProjectHereResponse, String> {
    request_select_project(url, folder, None, None).await
}

/// Resolves explicit selectors or the deepest containing Project root.
/// # Errors
/// Returns endpoint, selection or daemon failures.
pub async fn request_select_project(
    url: &str,
    folder: &str,
    workspace: Option<String>,
    project: Option<String>,
) -> Result<ProjectHereResponse, String> {
    client(url)?
        .here(ProjectHereRequest {
            folder: folder.into(),
            workspace,
            project,
        })
        .await
        .map_err(|error| format!("could not find Project through Forge daemon: {error}"))
}
/// Configures a Project Setup Hook through the daemon.
///
/// # Errors
///
/// Returns endpoint or daemon validation failures.
pub async fn request_project_setup_hook(
    url: impl Into<RecordEndpoint>,
    project_slug: String,
    hook: Option<String>,
) -> Result<(), String> {
    record_selection::record_client(url)?
        .worktree_setup_hook(forge_rpc::SetProjectSetupHookRequest { project_slug, hook })
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
    url: impl Into<RecordEndpoint>,
    record_id: u64,
    branch: String,
    base: String,
) -> Result<ManagedWorktreeResponse, String> {
    record_selection::record_client(url)?
        .worktree_create(forge_rpc::CreateManagedWorktreeRequest {
            record_id,
            branch,
            base,
        })
        .await
        .map_err(record_error("create Managed Worktree"))
}
/// Recovers an existing unclaimed Managed Worktree after operator confirmation.
///
/// # Errors
/// Returns endpoint, validation, or persistence failures.
pub async fn request_managed_worktree_recover(
    url: impl Into<RecordEndpoint>,
    record_id: u64,
    confirmed: bool,
) -> Result<ManagedWorktreeResponse, String> {
    record_selection::record_client(url)?
        .worktree_recover(forge_rpc::RecoverManagedWorktreeRequest {
            record_id,
            confirm_no_active_agent: confirmed,
        })
        .await
        .map_err(record_error("recover Managed Worktree"))
}
/// Inspects one Managed Worktree through the daemon.
///
/// # Errors
///
/// Returns endpoint or missing-claim failures.
pub async fn request_managed_worktree_info(
    url: impl Into<RecordEndpoint>,
    record_id: u64,
) -> Result<ManagedWorktreeResponse, String> {
    record_selection::record_client(url)?
        .worktree_info(RecordIdRequest { id: record_id })
        .await
        .map_err(record_error("inspect Managed Worktree"))
}
/// Lists Managed Worktrees through the daemon.
///
/// # Errors
///
/// Returns endpoint or persistence failures.
pub async fn request_managed_worktree_list(
    url: impl Into<RecordEndpoint>,
    include_all: bool,
) -> Result<ListManagedWorktreesResponse, String> {
    record_selection::record_client(url)?
        .worktree_list(forge_rpc::ListManagedWorktreesRequest { include_all })
        .await
        .map_err(record_error("list Managed Worktrees"))
}
/// Removes one Managed Worktree through the daemon.
///
/// # Errors
///
/// Returns endpoint or removal-safety failures.
pub async fn request_managed_worktree_remove(
    url: impl Into<RecordEndpoint>,
    record_id: u64,
    force: bool,
) -> Result<(), String> {
    record_selection::record_client(url)?
        .worktree_remove(forge_rpc::RemoveManagedWorktreeRequest { record_id, force })
        .await
        .map_err(record_error("remove Managed Worktree"))
}
/// Inspects or repairs Managed Worktrees through the daemon.
///
/// # Errors
///
/// Returns endpoint or repair failures.
pub async fn request_managed_worktree_doctor(
    url: impl Into<RecordEndpoint>,
    repair: bool,
) -> Result<ListManagedWorktreesResponse, String> {
    record_selection::record_client(url)?
        .worktree_doctor(forge_rpc::DoctorManagedWorktreesRequest { repair })
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
        project.reference, project.project_root, project.origin_url
    )
}
/// Opens a generic Record through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_open_record(
    url: impl Into<RecordEndpoint>,
    body: &str,
    project_slugs: Vec<String>,
    generated_by_ids: Vec<u64>,
) -> Result<RecordResponse, String> {
    record_client(url)?
        .open(OpenRecordRequest {
            body: body.into(),
            project_scope: ProjectScopeRequest { project_slugs },
            generated_by_ids,
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

/// Opens a Workspace-owned Initiative through the daemon.
/// # Errors
/// Returns selection, endpoint or validation errors.
pub async fn request_open_initiative(
    url: impl Into<RecordEndpoint>,
    body: &str,
    project_slugs: Vec<String>,
    generated_by_ids: Vec<u64>,
) -> Result<RecordResponse, String> {
    record_client(url)?
        .open_initiative(OpenRecordRequest {
            body: body.into(),
            project_scope: ProjectScopeRequest { project_slugs },
            generated_by_ids,
        })
        .await
        .map_err(record_error("open Initiative"))
}

/// Lists workflow readiness without requiring provider configuration.
/// # Errors
/// Returns selection, endpoint or validation errors.
pub async fn request_ready_records(
    url: impl Into<RecordEndpoint>,
    planning: bool,
    include_hitl: bool,
    blocked: bool,
) -> Result<forge_rpc::ReadyRecordsResponse, String> {
    record_client(url)?
        .ready(forge_rpc::ReadyRecordsRequest {
            planning,
            include_hitl,
            blocked,
        })
        .await
        .map_err(record_error("ready"))
}

/// Opens a Spec through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_open_spec(
    url: impl Into<RecordEndpoint>,
    body: &str,
    project_slug: String,
    generated_by_ids: Vec<u64>,
) -> Result<RecordResponse, String> {
    record_client(url)?
        .open_spec(OpenSpecRequest {
            body: body.into(),
            project_slug,
            generated_by_ids,
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
    url: impl Into<RecordEndpoint>,
    body: &str,
    project_slugs: Vec<String>,
    generated_by_ids: Vec<u64>,
) -> Result<RecordResponse, String> {
    record_client(url)?
        .open_wayfinder(OpenWayfinderRequest {
            body: body.into(),
            project_scope: ProjectScopeRequest { project_slugs },
            generated_by_ids,
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
    url: impl Into<RecordEndpoint>,
    body: &str,
    project_slug: Option<String>,
    task_kind: Option<String>,
    generated_by_ids: Vec<u64>,
) -> Result<RecordResponse, String> {
    record_client(url)?
        .open_task(OpenTaskRequest {
            body: body.into(),
            project_slug,
            task_kind,
            generated_by_ids,
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
    url: impl Into<RecordEndpoint>,
    body: &str,
    spec_id: u64,
    task_kind: Option<String>,
    generated_by_ids: Vec<u64>,
) -> Result<RecordResponse, String> {
    record_client(url)?
        .open_task_under_spec(OpenTaskUnderSpecRequest {
            body: body.into(),
            spec_id,
            task_kind,
            generated_by_ids,
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
    url: impl Into<RecordEndpoint>,
    body: &str,
    wayfinder_id: u64,
    project_slug: Option<String>,
    task_kind: Option<String>,
    generated_by_ids: Vec<u64>,
) -> Result<RecordResponse, String> {
    record_client(url)?
        .open_task_under_wayfinder(OpenTaskUnderWayfinderRequest {
            body: body.into(),
            wayfinder_id,
            project_slug,
            task_kind,
            generated_by_ids,
        })
        .await
        .map_err(record_error("open Task under Wayfinder"))
}

/// Opens a Grilling under a Wayfinder through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_open_grilling_under_wayfinder(
    url: impl Into<RecordEndpoint>,
    body: &str,
    wayfinder_id: u64,
    project_slugs: Option<Vec<String>>,
    generated_by_ids: Vec<u64>,
) -> Result<RecordResponse, String> {
    record_client(url)?
        .open_grilling_under_wayfinder(OpenGrillingUnderWayfinderRequest {
            body: body.into(),
            wayfinder_id,
            project_scope: project_slugs.map(|project_slugs| ProjectScopeRequest { project_slugs }),
            generated_by_ids,
        })
        .await
        .map_err(record_error("open Grilling under Wayfinder"))
}

/// Lists Records through the daemon using optional scope, status, and type filters.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_list_records(
    url: impl Into<RecordEndpoint>,
    project_slug: Option<String>,
    status: Option<String>,
    record_type: Option<String>,
    task_kind: Option<String>,
) -> Result<ListRecordsResponse, String> {
    record_client(url)?
        .list(ListRecordsRequest {
            project_slug,
            status,
            record_type,
            task_kind,
        })
        .await
        .map_err(record_error("list"))
}

/// Read immutable Task criteria through a Workspace-selected operator request.
///
/// # Errors
/// Returns endpoint, Workspace selection, or daemon validation errors.
pub async fn request_task_check_list(
    url: impl Into<RecordEndpoint>,
    task: u64,
) -> Result<Vec<forge_rpc::TaskCheck>, String> {
    record_client(url)?
        .task_check_list(RecordIdRequest { id: task })
        .await
        .map_err(record_error("task check list"))
}
/// Inspect one criterion and a bounded Assessment page.
///
/// # Errors
/// Returns endpoint, scope, Check ownership, or pagination errors.
pub async fn request_task_check_show(
    url: impl Into<RecordEndpoint>,
    request: forge_rpc::TaskCheckShowRequest,
) -> Result<forge_rpc::CheckPage, String> {
    record_client(url)?
        .task_check_show(request)
        .await
        .map_err(record_error("task check show"))
}
/// Append an attributed Assessment using the observed Check version.
///
/// # Errors
/// Returns endpoint, validation, stale-version, frozen-Task, or scope errors.
pub async fn request_task_check_assess(
    url: impl Into<RecordEndpoint>,
    request: forge_rpc::TaskCheckAssessRequest,
) -> Result<forge_rpc::TaskCheck, String> {
    record_client(url)?
        .task_check_assess(request)
        .await
        .map_err(record_error("task check assess"))
}

/// Requests a Record lifecycle operation through the daemon.
///
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_show_record(
    url: impl Into<RecordEndpoint>,
    id: u64,
) -> Result<RecordResponse, String> {
    record_client(url)?
        .show(RecordIdRequest { id })
        .await
        .map_err(record_error("show"))
}
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_edit_record(
    url: impl Into<RecordEndpoint>,
    id: u64,
    body: &str,
) -> Result<RecordResponse, String> {
    record_client(url)?
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
pub async fn request_start_record(
    url: impl Into<RecordEndpoint>,
    id: u64,
) -> Result<RecordResponse, String> {
    record_client(url)?
        .start(RecordIdRequest { id })
        .await
        .map_err(record_error("start"))
}
/// # Errors
///
/// Returns an endpoint or daemon validation error.
pub async fn request_done_record(
    url: impl Into<RecordEndpoint>,
    id: u64,
    completion_resolution: Option<String>,
) -> Result<RecordResponse, String> {
    record_client(url)?
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
    url: impl Into<RecordEndpoint>,
    from_id: u64,
    to_id: u64,
    kind: String,
) -> Result<RecordResponse, String> {
    record_client(url)?
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
    url: impl Into<RecordEndpoint>,
    from_id: u64,
    to_id: u64,
    kind: String,
) -> Result<RecordResponse, String> {
    record_client(url)?
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
    url: impl Into<RecordEndpoint>,
    record_id: u64,
    body: &str,
    author: Option<String>,
) -> Result<forge_rpc::RecordCommentResponse, String> {
    record_client(url)?
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
    url: impl Into<RecordEndpoint>,
    id: u64,
) -> Result<ListRecordCommentsResponse, String> {
    record_client(url)?
        .comments(RecordIdRequest { id })
        .await
        .map_err(record_error("comments"))
}
/// # Errors
/// Returns an endpoint or daemon validation error.
pub async fn request_delete_record_comment(
    url: impl Into<RecordEndpoint>,
    record_id: u64,
    comment_id: u64,
) -> Result<(), String> {
    record_client(url)?
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
    url: impl Into<RecordEndpoint>,
    id: u64,
) -> Result<ListRecordUpdatesResponse, String> {
    record_client(url)?
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
            let detail = if let Some(id) = update.assessment_id {
                format!(" Assessment {id}")
            } else {
                match (
                    &update.from_status,
                    &update.to_status,
                    update.related_record_id.or(update.comment_id),
                ) {
                    (Some(from), Some(to), _) => format!(" {from} -> {to}"),
                    (_, _, Some(id)) => format!(
                        " Record {id}{}",
                        update
                            .relationship_kind
                            .as_deref()
                            .map_or(String::new(), |kind| format!(" ({kind})"))
                    ),
                    _ => String::new(),
                }
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
                reference: String::new(),
                workspace_id: String::new(),
                slug: String::new(),
                project_root: PathBuf::from(folder),
                origin_url: "https://example.test/origin.git".into(),
            })
        }

        fn head_commit(&self, _: &Path) -> Result<String, RegistrationError> {
            Ok("test-base".into())
        }
    }

    fn test_application() -> Application {
        let db = std::env::temp_dir().join(format!(
            "forge-cli-rpc-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let workspace = SqliteProjectRepository::open(&db).unwrap();
        forge_app::WorkspaceRepository::create_workspace(
            &workspace,
            "test",
            "Test",
            Path::new("/"),
        )
        .unwrap();
        Application::new(
            Box::new(SqliteProjectRepository::open(&db).unwrap()),
            Box::new(TestInspector),
        )
        .with_workspace_repository(Box::new(workspace))
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
        request_register_project(&url, &folder, "test/cli-test-project")
            .await
            .unwrap();

        let spec = request_open_spec(&url, "Spec body", "test/cli-test-project".into(), vec![])
            .await
            .unwrap();
        assert_eq!(spec.record_type, "spec");
        assert_eq!(spec.project_scope.project_slugs, ["test/cli-test-project"]);
        let wayfinder = request_open_wayfinder(
            &url,
            "Wayfinder body",
            vec!["test/cli-test-project".into()],
            vec![],
        )
        .await
        .unwrap();
        assert_eq!(wayfinder.record_type, "wayfinder");
        let wayfinder_task = request_open_task_under_wayfinder(
            &url,
            "Scoped Child Task",
            wayfinder.id,
            None,
            None,
            vec![],
        )
        .await
        .unwrap();
        assert_eq!(wayfinder_task.project_scope, wayfinder.project_scope);

        let standalone = request_open_task(&url, "Standalone Task", None, None, vec![])
            .await
            .unwrap();
        assert_eq!(standalone.record_type, "task");
        assert!(standalone.project_scope.project_slugs.is_empty());
        let child = request_open_task_under_spec(&url, "Child Task", spec.id, None, vec![])
            .await
            .unwrap();
        assert_eq!(child.record_type, "task");
        assert_eq!(child.project_scope, spec.project_scope);
        assert_eq!(child.relationships[0].parent_id, spec.id);

        let listed = request_list_records(&url, None, None, Some("task".into()), None)
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
                workspace_id: "test".into(),
                id: 4,
                record_type: "generic".into(),
                task_kind: None,
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
                    assessment_id: None,
                    id: 7,
                    created_at_millis: 42,
                    kind: "status-transition".into(),
                    from_status: Some("open".into()),
                    to_status: Some("in_progress".into()),
                    related_record_id: None,
                    relationship_kind: None,
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
                workspace_id: "test".into(),
                id: 4,
                record_type: "generic".into(),
                task_kind: None,
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
