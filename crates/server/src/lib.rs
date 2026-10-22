//! Daemon composition root and HTTP adapters.
//!
//! The daemon listens on loopback by default. Binding it to a remote address
//! exposes an unauthenticated service, so use that only on a trusted private network.

use std::{
    env, fs, io,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::Arc,
};

use tokio::io::AsyncReadExt;

use askama::Template;
use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{StatusCode, header},
    response::Html,
    routing::{get, post},
};
use forge_app::{
    Application, ApplicationStatus, Project, RecordStatus, RecordType, RegistrationError,
    RepositoryInspector,
};
use forge_rpc::{
    AddRecordCommentRequest, DeleteRecordCommentRequest, DoneRecordRequest, EditRecordRequest,
    ListRecordCommentsResponse, ListRecordUpdatesResponse, ListRecordsRequest, ListRecordsResponse,
    ManagementStatus, OpenRecordRequest, OpenSpecRequest, OpenTaskRequest,
    OpenTaskUnderSpecRequest, ProjectApiServer, ProjectHereRequest, ProjectHereResponse,
    RecordApiServer, RecordIdRequest, RecordResponse, RegisterProjectRequest,
    RegisterProjectResponse, RelateRecordRequest, RevisionIdentity, SystemApiServer,
    SystemStatusResponse,
};
use forge_store::SqliteProjectRepository;
use jsonrpsee::{
    core::{RpcResult, async_trait},
    server::{Server, stop_channel},
};
use tower::ServiceExt;
use tracing::{info, warn};

/// Loopback address used when neither supported configuration source is set.
pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:45003";

/// Explicit inputs supplied by Forgeup when it launches a Managed Daemon.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedLaunchInputs {
    pub bind_addr: String,
    pub store_path: String,
    pub runtime_record_path: String,
    pub lifecycle_control_endpoint: String,
    pub lifecycle_credential: String,
    pub daemon_instance_id: String,
}

/// Managed-only configuration retained at the server composition boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct ManagedDaemonConfig {
    pub runtime_record_path: PathBuf,
    pub lifecycle_control_endpoint: PathBuf,
    lifecycle_credential: String,
    pub daemon_instance_id: String,
}

impl std::fmt::Debug for ManagedDaemonConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagedDaemonConfig")
            .field("runtime_record_path", &self.runtime_record_path)
            .field(
                "lifecycle_control_endpoint",
                &self.lifecycle_control_endpoint,
            )
            .field("lifecycle_credential", &"[redacted]")
            .field("daemon_instance_id", &self.daemon_instance_id)
            .finish()
    }
}

impl ManagedDaemonConfig {
    /// Credential used only by the per-instance lifecycle listener.
    #[must_use]
    pub fn lifecycle_credential(&self) -> &str {
        &self.lifecycle_credential
    }
}

/// Daemon startup configuration resolved before the listener is opened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonConfig {
    /// Address owned by the one daemon listener.
    pub bind_addr: SocketAddr,
    /// Database path selected for this daemon instance.
    pub store_path: PathBuf,
    /// Present only when Forgeup explicitly owns this daemon launch.
    pub managed: Option<ManagedDaemonConfig>,
}

impl DaemonConfig {
    /// Resolves manual-launch configuration with an explicit flag taking precedence over the environment.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected address is not a socket address.
    pub fn resolve(
        flag_bind_addr: Option<&str>,
        env_bind_addr: Option<&str>,
    ) -> Result<Self, String> {
        let value = flag_bind_addr
            .or(env_bind_addr)
            .unwrap_or(DEFAULT_BIND_ADDR);
        let bind_addr = SocketAddr::from_str(value)
            .map_err(|error| format!("invalid daemon bind address `{value}`: {error}"))?;
        Ok(Self {
            bind_addr,
            store_path: env::var_os("FORGE_STORE_PATH")
                .map_or_else(|| PathBuf::from("forge.sqlite3"), PathBuf::from),
            managed: None,
        })
    }

    /// Resolves the explicit Managed Daemon configuration without consulting ambient settings.
    ///
    /// # Errors
    ///
    /// Returns an error when any managed input is empty or its bind address is malformed.
    pub fn managed(inputs: ManagedLaunchInputs) -> Result<Self, String> {
        let bind_addr = SocketAddr::from_str(&inputs.bind_addr).map_err(|error| {
            format!(
                "invalid managed daemon bind address `{}`: {error}",
                inputs.bind_addr
            )
        })?;
        let required = [
            ("store path", &inputs.store_path),
            ("runtime-record path", &inputs.runtime_record_path),
            (
                "lifecycle-control endpoint",
                &inputs.lifecycle_control_endpoint,
            ),
            ("lifecycle credential", &inputs.lifecycle_credential),
            ("daemon-instance ID", &inputs.daemon_instance_id),
        ];
        if let Some((name, _)) = required.iter().find(|(_, value)| value.trim().is_empty()) {
            return Err(format!("managed daemon {name} must not be empty"));
        }
        Ok(Self {
            bind_addr,
            store_path: PathBuf::from(inputs.store_path),
            managed: Some(ManagedDaemonConfig {
                runtime_record_path: PathBuf::from(inputs.runtime_record_path),
                lifecycle_control_endpoint: PathBuf::from(inputs.lifecycle_control_endpoint),
                lifecycle_credential: inputs.lifecycle_credential,
                daemon_instance_id: inputs.daemon_instance_id,
            }),
        })
    }

    /// Resolves process configuration, preserving manual environment compatibility.
    ///
    /// Managed inputs are explicit and therefore take precedence over all ambient settings.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected configuration is invalid.
    pub fn from_process(
        flag_bind_addr: Option<&str>,
        managed: Option<ManagedLaunchInputs>,
    ) -> Result<Self, String> {
        match managed {
            Some(inputs) => Self::managed(inputs),
            None => Self::resolve(flag_bind_addr, env::var("FORGE_BIND_ADDR").ok().as_deref()),
        }
    }
}

/// Owns the application and exposes it through the daemon transport adapters.
#[derive(Clone)]
pub struct Daemon {
    application: Arc<Application>,
    management: ManagementStatus,
}

impl Daemon {
    /// Creates a manual daemon around the shared application boundary.
    #[must_use]
    pub fn new(application: Application) -> Self {
        Self::with_daemon_instance_id(application, None)
    }

    /// Creates a daemon that reports the supplied managed-launch ownership ID.
    #[must_use]
    pub fn with_daemon_instance_id(
        application: Application,
        daemon_instance_id: Option<String>,
    ) -> Self {
        Self {
            application: Arc::new(application),
            management: ManagementStatus::new(
                RevisionIdentity::from(forge_build_info::current()),
                daemon_instance_id,
            ),
        }
    }
}

/// Git command implementation kept at the daemon composition edge.
pub struct GitInspector;

impl RepositoryInspector for GitInspector {
    fn inspect(&self, folder: &Path) -> Result<Project, RegistrationError> {
        let canonical = fs::canonicalize(folder).map_err(|_| {
            RegistrationError::new(
                "invalid-folder",
                format!("`{}` is not an existing folder", folder.display()),
            )
        })?;
        if !canonical.is_dir() {
            return Err(RegistrationError::new(
                "invalid-folder",
                format!("`{}` is not a folder", canonical.display()),
            ));
        }
        let root = git(&canonical, &["rev-parse", "--show-toplevel"]).map_err(|()| {
            RegistrationError::new(
                "not-git",
                format!(
                    "`{}` is not an eligible Git repository",
                    canonical.display()
                ),
            )
        })?;
        let root = fs::canonicalize(root).map_err(|_| {
            RegistrationError::new("not-git", "could not resolve Git worktree root")
        })?;
        if root != canonical {
            return Err(RegistrationError::new(
                "not-main-worktree",
                "the Project folder must be the Git main worktree root",
            ));
        }
        let git_dir = resolve_git_path(
            &root,
            &git(&root, &["rev-parse", "--git-dir"]).map_err(|()| {
                RegistrationError::new("not-git", "could not inspect Git directory")
            })?,
        );
        let common_dir = resolve_git_path(
            &root,
            &git(&root, &["rev-parse", "--git-common-dir"]).map_err(|()| {
                RegistrationError::new("not-git", "could not inspect Git common directory")
            })?,
        );
        if git_dir != common_dir {
            return Err(RegistrationError::new(
                "not-main-worktree",
                "only a Git main worktree may be registered as a Project",
            ));
        }
        let origin_url = git(&root, &["remote", "get-url", "origin"]).map_err(|()| {
            RegistrationError::new(
                "missing-origin",
                "Git remote `origin` is required to register a Project",
            )
        })?;
        Ok(Project {
            slug: String::new(),
            registered_folder: root,
            origin_url,
        })
    }
}

fn git(folder: &Path, arguments: &[&str]) -> Result<String, ()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(arguments)
        .output()
        .map_err(|_| ())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(())
    }
}

fn resolve_git_path(root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    fs::canonicalize(if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    })
    .unwrap_or_else(|_| root.join(path))
}

fn application_from_config(config: &DaemonConfig) -> Result<Application, String> {
    let store = SqliteProjectRepository::open(&config.store_path).map_err(|error| error.message)?;
    Ok(Application::new(Box::new(store), Box::new(GitInspector)))
}

#[async_trait]
impl SystemApiServer for Daemon {
    async fn status(&self) -> RpcResult<SystemStatusResponse> {
        Ok(SystemStatusResponse::new(
            &self.application.status(),
            self.management.clone(),
        ))
    }
}

#[async_trait]
impl RecordApiServer for Daemon {
    async fn open(&self, request: OpenRecordRequest) -> RpcResult<RecordResponse> {
        self.application
            .open_record(&request.body, request.project_scope.project_slugs)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn open_spec(&self, request: OpenSpecRequest) -> RpcResult<RecordResponse> {
        self.application
            .open_spec(&request.body, request.project_slug)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn open_task(&self, request: OpenTaskRequest) -> RpcResult<RecordResponse> {
        self.application
            .open_task(&request.body, request.project_slug)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn open_task_under_spec(
        &self,
        request: OpenTaskUnderSpecRequest,
    ) -> RpcResult<RecordResponse> {
        self.application
            .open_task_under_spec(&request.body, request.spec_id)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn show(&self, request: RecordIdRequest) -> RpcResult<RecordResponse> {
        self.application
            .show_record(request.id)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn list(&self, request: ListRecordsRequest) -> RpcResult<ListRecordsResponse> {
        let status = request.status.as_deref().map(record_status).transpose()?;
        let record_type = request
            .record_type
            .as_deref()
            .map(record_type)
            .transpose()?;
        self.application
            .list_records(request.project_slug, status, record_type)
            .map(|records| ListRecordsResponse {
                records: records.into_iter().map(Into::into).collect(),
            })
            .map_err(rpc_error)
    }

    async fn edit(&self, request: EditRecordRequest) -> RpcResult<RecordResponse> {
        self.application
            .edit_record(request.id, &request.body)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn start(&self, request: RecordIdRequest) -> RpcResult<RecordResponse> {
        self.application
            .start_record(request.id)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn done(&self, request: DoneRecordRequest) -> RpcResult<RecordResponse> {
        self.application
            .done_record(request.id, request.completion_resolution.as_deref())
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn relate(&self, request: RelateRecordRequest) -> RpcResult<RecordResponse> {
        relationship_kind(&request.kind)?;
        self.application
            .relate_records(request.child_id, request.parent_id)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn unrelate(&self, request: RelateRecordRequest) -> RpcResult<RecordResponse> {
        relationship_kind(&request.kind)?;
        self.application
            .unrelate_records(request.child_id, request.parent_id)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn updates(&self, request: RecordIdRequest) -> RpcResult<ListRecordUpdatesResponse> {
        self.application
            .list_record_updates(request.id)
            .map(|updates| ListRecordUpdatesResponse {
                updates: updates.into_iter().map(Into::into).collect(),
            })
            .map_err(rpc_error)
    }

    async fn comment(
        &self,
        request: AddRecordCommentRequest,
    ) -> RpcResult<forge_rpc::RecordCommentResponse> {
        self.application
            .add_record_comment(request.record_id, &request.body, request.author.as_deref())
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn comments(&self, request: RecordIdRequest) -> RpcResult<ListRecordCommentsResponse> {
        self.application
            .list_record_comments(request.id)
            .map(|comments| ListRecordCommentsResponse {
                comments: comments.into_iter().map(Into::into).collect(),
            })
            .map_err(rpc_error)
    }

    async fn comment_delete(&self, request: DeleteRecordCommentRequest) -> RpcResult<()> {
        self.application
            .delete_record_comment(request.record_id, request.comment_id)
            .map_err(rpc_error)
    }
}

fn record_status(status: &str) -> RpcResult<RecordStatus> {
    match status {
        "open" => Ok(RecordStatus::Open),
        "in_progress" => Ok(RecordStatus::InProgress),
        "done" => Ok(RecordStatus::Done),
        _ => Err(rpc_error(RegistrationError::new(
            "invalid-status",
            "Record status must be open, in_progress, or done",
        ))),
    }
}

fn record_type(record_type: &str) -> RpcResult<RecordType> {
    match record_type {
        "generic" => Ok(RecordType::Generic),
        "spec" => Ok(RecordType::Spec),
        "task" => Ok(RecordType::Task),
        _ => Err(rpc_error(RegistrationError::new(
            "invalid-record-type",
            "Record type must be generic, spec, or task",
        ))),
    }
}

fn relationship_kind(kind: &str) -> RpcResult<()> {
    if kind == "child-of" {
        Ok(())
    } else {
        Err(rpc_error(RegistrationError::new(
            "unsupported-relationship-kind",
            "only the child-of Relationship mutation kind is supported",
        )))
    }
}

#[async_trait]
impl ProjectApiServer for Daemon {
    async fn register(
        &self,
        request: RegisterProjectRequest,
    ) -> RpcResult<RegisterProjectResponse> {
        self.application
            .register_project(Path::new(&request.folder), &request.slug)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn here(&self, request: ProjectHereRequest) -> RpcResult<ProjectHereResponse> {
        self.application
            .project_here(Path::new(&request.folder))
            .map(Into::into)
            .map_err(rpc_error)
    }
}

fn rpc_error(error: RegistrationError) -> jsonrpsee::types::ErrorObjectOwned {
    jsonrpsee::types::ErrorObjectOwned::owned(-32001, error.message, Some(error.code))
}

/// Builds the single listener's router, including the browser and typed RPC services.
///
/// # Panics
///
/// Panics if the statically defined RPC method sets overlap.
pub fn router(daemon: Daemon) -> Router {
    let page_daemon = daemon.clone();
    let status_daemon = daemon.clone();
    let mut methods = SystemApiServer::into_rpc(daemon.clone());
    methods
        .merge(ProjectApiServer::into_rpc(daemon.clone()))
        .expect("RPC method names must not overlap");
    methods
        .merge(RecordApiServer::into_rpc(daemon))
        .expect("RPC method names must not overlap");
    let service_builder = Server::builder().to_service_builder();
    let (stop_handle, server_handle) = stop_channel();
    let server_handle = Arc::new(server_handle);

    Router::new()
        .route(
            "/",
            get(move || {
                let daemon = page_daemon.clone();
                async move { status_page(&daemon.application.status()) }
            }),
        )
        .route(
            "/status",
            get(move || {
                let daemon = status_daemon.clone();
                async move { status_fragment(&daemon.application.status()) }
            }),
        )
        .route(
            "/assets/htmx.min.js",
            get(|| async {
                (
                    [(
                        header::CONTENT_TYPE,
                        "application/javascript; charset=utf-8",
                    )],
                    include_str!("../assets/htmx.min.js"),
                )
            }),
        )
        .route(
            "/rpc",
            post(move |request: Request| {
                let rpc_service = service_builder
                    .clone()
                    .build(methods.clone(), stop_handle.clone());
                let server_handle = Arc::clone(&server_handle);
                async move {
                    let _keep_server_handle_alive = server_handle;
                    rpc_service
                        .oneshot(request)
                        .await
                        .map(|response| response.map(Body::new))
                        .map_err(|error| {
                            warn!(%error, "RPC service request failed");
                            StatusCode::INTERNAL_SERVER_ERROR
                        })
                }
            }),
        )
}

#[derive(Template)]
#[template(
    source = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Forge daemon status</title>
  <script src="/assets/htmx.min.js"></script>
</head>
<body>
  <main>
    <h1>Forge daemon</h1>
    {{ status_html|safe }}
  </main>
</body>
</html>"##,
    ext = "html"
)]
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

fn status_page(status: &ApplicationStatus) -> Result<Html<String>, StatusCode> {
    let status_html = StatusFragmentTemplate { status }
        .render()
        .map_err(|error| {
            warn!(%error, "failed to render status fragment");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    StatusPageTemplate {
        status_html: &status_html,
    }
    .render()
    .map(Html)
    .map_err(|error| {
        warn!(%error, "failed to render status page");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

fn status_fragment(status: &ApplicationStatus) -> Result<Html<String>, StatusCode> {
    StatusFragmentTemplate { status }
        .render()
        .map(Html)
        .map_err(|error| {
            warn!(%error, "failed to render status fragment");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

/// Runs the daemon until Ctrl-C or SIGTERM requests graceful shutdown.
///
/// # Errors
///
/// Returns an error when the listener cannot bind or serve connections.
pub async fn serve(config: DaemonConfig) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    let address = listener.local_addr()?;
    if !address.ip().is_loopback() {
        warn!(%address, "remote daemon binding is unauthenticated; use only a trusted private network");
    }
    info!(%address, "Forge daemon listening");

    let application = application_from_config(&config).map_err(std::io::Error::other)?;
    let daemon_instance_id = config
        .managed
        .as_ref()
        .map(|managed| managed.daemon_instance_id.clone());
    let lifecycle_shutdown = match config.managed.as_ref() {
        Some(managed) => Some(bind_lifecycle_control(managed)?),
        None => None,
    };
    axum::serve(
        listener,
        router(Daemon::with_daemon_instance_id(
            application,
            daemon_instance_id,
        )),
    )
    .with_graceful_shutdown(wait_for_shutdown(lifecycle_shutdown))
    .await
}

async fn wait_for_shutdown(lifecycle_shutdown: Option<LifecycleControl>) {
    match lifecycle_shutdown {
        Some(control) => tokio::select! {
            () = shutdown_signal() => {},
            result = control.wait_for_authenticated_shutdown() => match result {
                Ok(()) => info!("authenticated lifecycle shutdown received; shutting down Forge daemon"),
                Err(error) => warn!(%error, "lifecycle control endpoint failed; shutting down Forge daemon"),
            },
        },
        None => shutdown_signal().await,
    }
}

/// A per-managed-daemon local lifecycle listener. It is deliberately separate
/// from the public HTTP/JSON-RPC listener.
enum LifecycleControl {
    #[cfg(unix)]
    Unix {
        listener: tokio::net::UnixListener,
        endpoint: PathBuf,
        credential: String,
    },
    #[cfg(windows)]
    Windows {
        endpoint: String,
        credential: String,
    },
}

impl LifecycleControl {
    async fn wait_for_authenticated_shutdown(self) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            Self::Unix {
                listener,
                endpoint,
                credential,
            } => {
                struct Cleanup(PathBuf);
                impl Drop for Cleanup {
                    fn drop(&mut self) {
                        if let Err(error) = fs::remove_file(&self.0)
                            && error.kind() != io::ErrorKind::NotFound
                        {
                            warn!(%error, endpoint = %self.0.display(), "failed to remove lifecycle control endpoint");
                        }
                    }
                }
                let _cleanup = Cleanup(endpoint);
                loop {
                    let (mut stream, _) = listener.accept().await?;
                    if credential_matches(&mut stream, &credential).await? {
                        return Ok(());
                    }
                    warn!("rejected lifecycle shutdown request with invalid credential");
                }
            }
            #[cfg(windows)]
            Self::Windows {
                endpoint,
                credential,
            } => {
                use tokio::net::windows::named_pipe::ServerOptions;
                loop {
                    let mut pipe = ServerOptions::new().create(&endpoint)?;
                    pipe.connect().await?;
                    if credential_matches(&mut pipe, &credential).await? {
                        return Ok(());
                    }
                    warn!("rejected lifecycle shutdown request with invalid credential");
                }
            }
        }
    }
}

async fn credential_matches<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    credential: &str,
) -> io::Result<bool> {
    let mut request = Vec::with_capacity(credential.len() + 1);
    loop {
        let mut byte = [0_u8; 1];
        if reader.read(&mut byte).await? == 0 {
            return Ok(false);
        }
        if byte[0] == b'\n' {
            break;
        }
        if request.len() >= 4096 {
            return Ok(false);
        }
        request.push(byte[0]);
    }
    Ok(request == credential.as_bytes())
}

fn bind_lifecycle_control(managed: &ManagedDaemonConfig) -> io::Result<LifecycleControl> {
    #[cfg(unix)]
    {
        let listener = tokio::net::UnixListener::bind(&managed.lifecycle_control_endpoint)?;
        info!(endpoint = %managed.lifecycle_control_endpoint.display(), "Forge lifecycle control listening");
        Ok(LifecycleControl::Unix {
            listener,
            endpoint: managed.lifecycle_control_endpoint.clone(),
            credential: managed.lifecycle_credential.clone(),
        })
    }
    #[cfg(windows)]
    {
        let endpoint = managed
            .lifecycle_control_endpoint
            .to_string_lossy()
            .into_owned();
        if endpoint.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "lifecycle control endpoint must not be empty",
            ));
        }
        Ok(LifecycleControl::Windows {
            endpoint,
            credential: managed.lifecycle_credential.clone(),
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = managed;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "local lifecycle control is unsupported on this platform",
        ))
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("Ctrl-C received; shutting down Forge daemon"),
        () = terminate => info!("SIGTERM received; shutting down Forge daemon"),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::{Path, PathBuf},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[cfg(unix)]
    use tokio::io::AsyncWriteExt;

    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use forge_app::{Application, Project, RegistrationError, RepositoryInspector};
    use forge_rpc::{
        DoneRecordRequest, EditRecordRequest, ListRecordsRequest, OpenRecordRequest,
        OpenSpecRequest, OpenTaskRequest, OpenTaskUnderSpecRequest, ProjectApiClient,
        ProjectHereRequest, ProjectScopeRequest, RecordApiClient, RecordIdRequest,
        RegisterProjectRequest, SystemApiClient, SystemApiServer,
    };
    use forge_store::SqliteProjectRepository;
    use jsonrpsee::http_client::HttpClientBuilder;
    use tower::ServiceExt;

    use super::{DEFAULT_BIND_ADDR, Daemon, DaemonConfig, ManagedLaunchInputs, router, serve};

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
    fn bind_configuration_prioritizes_flag_then_environment_then_default() {
        assert_eq!(
            DaemonConfig::resolve(Some("127.0.0.1:45004"), Some("127.0.0.1:45005"))
                .unwrap()
                .bind_addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 45004)
        );
        assert_eq!(
            DaemonConfig::resolve(None, Some("127.0.0.1:45005"))
                .unwrap()
                .bind_addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 45005)
        );
        assert_eq!(
            DaemonConfig::resolve(None, None)
                .unwrap()
                .bind_addr
                .to_string(),
            DEFAULT_BIND_ADDR
        );
    }

    #[test]
    fn managed_configuration_is_explicit_and_rejects_invalid_inputs() {
        let inputs = ManagedLaunchInputs {
            bind_addr: "127.0.0.1:45004".into(),
            store_path: "managed.sqlite3".into(),
            runtime_record_path: "runtime.json".into(),
            lifecycle_control_endpoint: "control.sock".into(),
            lifecycle_credential: "credential".into(),
            daemon_instance_id: "instance-123".into(),
        };
        let config =
            DaemonConfig::from_process(Some("127.0.0.1:45005"), Some(inputs.clone())).unwrap();
        assert_eq!(config.bind_addr.to_string(), "127.0.0.1:45004");
        assert_eq!(config.store_path, PathBuf::from("managed.sqlite3"));
        let managed = config.managed.unwrap();
        assert_eq!(managed.daemon_instance_id, "instance-123");
        assert_eq!(managed.lifecycle_credential(), "credential");

        let error = DaemonConfig::managed(ManagedLaunchInputs {
            bind_addr: "not-an-address".into(),
            store_path: "managed.sqlite3".into(),
            runtime_record_path: "runtime.json".into(),
            lifecycle_control_endpoint: "control.sock".into(),
            lifecycle_credential: "credential".into(),
            daemon_instance_id: "instance-123".into(),
        })
        .unwrap_err();
        assert!(error.contains("invalid managed daemon bind address"));

        let error = DaemonConfig::managed(ManagedLaunchInputs {
            bind_addr: "127.0.0.1:45004".into(),
            store_path: String::new(),
            runtime_record_path: "runtime.json".into(),
            lifecycle_control_endpoint: "control.sock".into(),
            lifecycle_credential: "credential".into(),
            daemon_instance_id: "instance-123".into(),
        })
        .unwrap_err();
        assert_eq!(error, "managed daemon store path must not be empty");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn lifecycle_control_rejects_wrong_credentials_then_gracefully_stops_and_cleans_up() {
        let unique = format!(
            "forge-lifecycle-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let directory = std::env::temp_dir().join(unique);
        std::fs::create_dir(&directory).unwrap();
        let endpoint = directory.join("control.sock");
        let config = DaemonConfig::managed(ManagedLaunchInputs {
            bind_addr: "127.0.0.1:0".into(),
            store_path: directory
                .join("forge.sqlite3")
                .to_string_lossy()
                .into_owned(),
            runtime_record_path: directory
                .join("runtime.json")
                .to_string_lossy()
                .into_owned(),
            lifecycle_control_endpoint: endpoint.to_string_lossy().into_owned(),
            lifecycle_credential: "expected-credential".into(),
            daemon_instance_id: "instance-123".into(),
        })
        .unwrap();
        let mut server = tokio::spawn(serve(config));

        for _ in 0..100 {
            if endpoint.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(endpoint.exists(), "lifecycle endpoint was not created");
        let mut wrong = tokio::net::UnixStream::connect(&endpoint).await.unwrap();
        wrong.write_all(b"wrong-credential\n").await.unwrap();
        drop(wrong);
        assert!(endpoint.exists());
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut server)
                .await
                .is_err()
        );

        let mut accepted = tokio::net::UnixStream::connect(&endpoint).await.unwrap();
        accepted.write_all(b"expected-credential\n").await.unwrap();
        drop(accepted);
        server.await.unwrap().unwrap();
        assert!(!endpoint.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn managed_daemon_status_reports_the_launch_instance_id() {
        let daemon =
            Daemon::with_daemon_instance_id(test_application(), Some("instance-123".into()));
        let status = daemon.status().await.unwrap();
        assert_eq!(
            status.management.daemon_instance_id.as_deref(),
            Some("instance-123")
        );
        assert_eq!(
            status.management.revision_identity,
            forge_rpc::RevisionIdentity::from(forge_build_info::current())
        );
    }

    #[tokio::test]
    async fn browser_routes_render_shared_status_without_cors() {
        let response = router(Daemon::new(test_application()))
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
        let page = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(page.contains("src=\"/assets/htmx.min.js\""));
        assert!(page.contains("hx-get=\"/status\""));
        assert!(page.contains("forge"));

        let response = router(Daemon::new(test_application()))
            .oneshot(
                Request::builder()
                    .uri("/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
        let fragment = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(fragment.contains("id=\"status\""));
        assert!(fragment.contains("forge"));
        assert!(fragment.contains("0.1.0"));
    }

    #[tokio::test]
    async fn htmx_asset_is_served_locally_with_javascript_content_type() {
        let response = router(Daemon::new(test_application()))
            .oneshot(
                Request::builder()
                    .uri("/assets/htmx.min.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/javascript; charset=utf-8"
        );
        let asset = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(asset.starts_with("var htmx="));
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn typed_rpc_client_round_trips_through_the_listener() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router(Daemon::new(test_application())))
                .await
                .unwrap();
        });

        let client = HttpClientBuilder::default()
            .build(format!("http://{address}/rpc"))
            .unwrap();
        let status = client.status().await.unwrap();
        assert_eq!(status.name, "forge");
        assert!(status.ready);
        let folder = std::fs::canonicalize(format!("{}/../..", env!("CARGO_MANIFEST_DIR")))
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let project = client
            .register(RegisterProjectRequest {
                folder: folder.clone(),
                slug: "server-test-project".into(),
            })
            .await
            .unwrap();
        assert_eq!(project.slug, "server-test-project");
        assert_eq!(project.registered_folder, folder);
        let here = client
            .here(ProjectHereRequest {
                folder: format!("{folder}/crates"),
            })
            .await
            .unwrap();
        assert_eq!(here, project);

        let record = client
            .open(OpenRecordRequest {
                body: "A daemon-backed Record".into(),
                project_scope: ProjectScopeRequest {
                    project_slugs: vec!["server-test-project".into()],
                },
            })
            .await
            .unwrap();
        assert_eq!(record.id, 1);
        assert_eq!(record.status, "open");
        assert_eq!(record.project_scope.project_slugs, ["server-test-project"]);
        let updates = client
            .updates(RecordIdRequest { id: record.id })
            .await
            .unwrap();
        assert_eq!(updates.updates.len(), 1);
        assert_eq!(updates.updates[0].kind, "created");
        assert_eq!(
            client
                .list(ListRecordsRequest {
                    project_slug: Some("server-test-project".into()),
                    status: Some("open".into()),
                    record_type: None,
                })
                .await
                .unwrap()
                .records,
            vec![record.clone()]
        );
        let spec = client
            .open_spec(OpenSpecRequest {
                body: "A daemon-backed Spec".into(),
                project_slug: "server-test-project".into(),
            })
            .await
            .unwrap();
        assert_eq!(spec.record_type, "spec");
        let task = client
            .open_task_under_spec(OpenTaskUnderSpecRequest {
                body: "A daemon-backed Task".into(),
                spec_id: spec.id,
            })
            .await
            .unwrap();
        assert_eq!(task.record_type, "task");
        assert_eq!(task.project_scope, spec.project_scope);
        assert_eq!(task.relationships[0].parent_id, spec.id);
        assert_eq!(
            client
                .open_task(OpenTaskRequest {
                    body: "Standalone daemon-backed Task".into(),
                    project_slug: None,
                })
                .await
                .unwrap()
                .record_type,
            "task"
        );
        let specs = client
            .list(ListRecordsRequest {
                project_slug: None,
                status: None,
                record_type: Some("spec".into()),
            })
            .await
            .unwrap();
        assert_eq!(specs.records.len(), 1);
        assert_eq!(specs.records[0].id, spec.id);
        assert_eq!(specs.records[0].record_type, "spec");
        assert!(
            client
                .open_task_under_spec(OpenTaskUnderSpecRequest {
                    body: "Invalid child".into(),
                    spec_id: record.id,
                })
                .await
                .is_err()
        );
        assert!(
            client
                .list(ListRecordsRequest {
                    project_slug: None,
                    status: None,
                    record_type: Some("invalid".into()),
                })
                .await
                .is_err()
        );
        let edited = client
            .edit(EditRecordRequest {
                id: record.id,
                body: "Revised Record".into(),
            })
            .await
            .unwrap();
        assert_eq!(edited.body, "Revised Record");
        let in_progress = client
            .start(RecordIdRequest { id: record.id })
            .await
            .unwrap();
        assert_eq!(in_progress.status, "in_progress");
        let done = client
            .done(DoneRecordRequest {
                id: record.id,
                completion_resolution: Some("complete".into()),
            })
            .await
            .unwrap();
        assert_eq!(done.completion_resolution.as_deref(), Some("complete"));
        assert!(
            client
                .start(RecordIdRequest { id: record.id })
                .await
                .is_err()
        );
        assert_eq!(
            client
                .open(OpenRecordRequest {
                    body: "Unscoped".into(),
                    project_scope: ProjectScopeRequest {
                        project_slugs: vec![],
                    },
                })
                .await
                .unwrap()
                .project_scope
                .project_slugs,
            Vec::<String>::new()
        );

        server.abort();
    }
}
