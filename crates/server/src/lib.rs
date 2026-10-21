//! Daemon composition root and HTTP adapters.
//!
//! The daemon listens on loopback by default. Binding it to a remote address
//! exposes an unauthenticated service, so use that only on a trusted private network.

use std::{
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::Arc,
};

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
    Application, ApplicationStatus, Project, RecordStatus, RegistrationError, RepositoryInspector,
};
use forge_rpc::{
    DoneRecordRequest, EditRecordRequest, ListRecordsRequest, ListRecordsResponse,
    OpenRecordRequest, ProjectApiServer, ProjectHereRequest, ProjectHereResponse, RecordApiServer,
    RecordIdRequest, RecordResponse, RegisterProjectRequest, RegisterProjectResponse,
    RelateRecordRequest, SystemApiServer, SystemStatusResponse,
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

/// Daemon startup configuration resolved before the listener is opened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonConfig {
    /// Address owned by the one daemon listener.
    pub bind_addr: SocketAddr,
}

impl DaemonConfig {
    /// Resolves configuration with an explicit flag taking precedence over the environment.
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
        SocketAddr::from_str(value)
            .map(|bind_addr| Self { bind_addr })
            .map_err(|error| format!("invalid daemon bind address `{value}`: {error}"))
    }

    /// Resolves process configuration, including `FORGE_BIND_ADDR`.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected address is not a socket address.
    pub fn from_process(flag_bind_addr: Option<&str>) -> Result<Self, String> {
        Self::resolve(flag_bind_addr, env::var("FORGE_BIND_ADDR").ok().as_deref())
    }
}

/// Owns the application and exposes it through the daemon transport adapters.
#[derive(Clone)]
pub struct Daemon {
    application: Arc<Application>,
}

impl Daemon {
    /// Creates a daemon around the shared application boundary.
    #[must_use]
    pub fn new(application: Application) -> Self {
        Self {
            application: Arc::new(application),
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

fn application_from_process() -> Result<Application, String> {
    let path = env::var_os("FORGE_STORE_PATH")
        .map_or_else(|| PathBuf::from("forge.sqlite3"), PathBuf::from);
    let store = SqliteProjectRepository::open(&path).map_err(|error| error.message)?;
    Ok(Application::new(Box::new(store), Box::new(GitInspector)))
}

#[async_trait]
impl SystemApiServer for Daemon {
    async fn status(&self) -> RpcResult<SystemStatusResponse> {
        Ok(self.application.status().into())
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

    async fn show(&self, request: RecordIdRequest) -> RpcResult<RecordResponse> {
        self.application
            .show_record(request.id)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn list(&self, request: ListRecordsRequest) -> RpcResult<ListRecordsResponse> {
        let status = request.status.as_deref().map(record_status).transpose()?;
        self.application
            .list_records(request.project_slug, status)
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

    let application = application_from_process().map_err(std::io::Error::other)?;
    axum::serve(listener, router(Daemon::new(application)))
        .with_graceful_shutdown(shutdown_signal())
        .await
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
    };

    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use forge_app::{Application, Project, RegistrationError, RepositoryInspector};
    use forge_rpc::{
        DoneRecordRequest, EditRecordRequest, ListRecordsRequest, OpenRecordRequest,
        ProjectApiClient, ProjectHereRequest, ProjectScopeRequest, RecordApiClient,
        RecordIdRequest, RegisterProjectRequest, SystemApiClient,
    };
    use forge_store::SqliteProjectRepository;
    use jsonrpsee::http_client::HttpClientBuilder;
    use tower::ServiceExt;

    use super::{DEFAULT_BIND_ADDR, Daemon, DaemonConfig, router};

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
        assert_eq!(
            client
                .list(ListRecordsRequest {
                    project_slug: Some("server-test-project".into()),
                    status: Some("open".into()),
                })
                .await
                .unwrap()
                .records,
            vec![record.clone()]
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
