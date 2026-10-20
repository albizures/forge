//! Daemon composition root and HTTP adapters.
//!
//! The daemon listens on loopback by default. Binding it to a remote address
//! exposes an unauthenticated service, so use that only on a trusted private network.

use std::{env, net::SocketAddr, str::FromStr, sync::Arc};

use askama::Template;
use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{StatusCode, header},
    response::Html,
    routing::{get, post},
};
use forge_app::{Application, ApplicationStatus};
use forge_rpc::{SystemApiServer, SystemStatusResponse};
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
#[derive(Clone, Debug, Default)]
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

#[async_trait]
impl SystemApiServer for Daemon {
    async fn status(&self) -> RpcResult<SystemStatusResponse> {
        Ok(self.application.status().into())
    }
}

/// Builds the single listener's router, including the browser and typed RPC services.
pub fn router(daemon: Daemon) -> Router {
    let page_daemon = daemon.clone();
    let status_daemon = daemon.clone();
    let methods = daemon.into_rpc();
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

    axum::serve(listener, router(Daemon::new(Application::new())))
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
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use forge_app::Application;
    use forge_rpc::SystemApiClient;
    use jsonrpsee::http_client::HttpClientBuilder;
    use tower::ServiceExt;

    use super::{DEFAULT_BIND_ADDR, Daemon, DaemonConfig, router};

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
        let response = router(Daemon::new(Application::new()))
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

        let response = router(Daemon::new(Application::new()))
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
        let response = router(Daemon::new(Application::new()))
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
            axum::serve(listener, router(Daemon::new(Application::new())))
                .await
                .unwrap();
        });

        let client = HttpClientBuilder::default()
            .build(format!("http://{address}/rpc"))
            .unwrap();
        let status = client.status().await.unwrap();
        assert_eq!(status.name, "forge");
        assert!(status.ready);

        server.abort();
    }
}
