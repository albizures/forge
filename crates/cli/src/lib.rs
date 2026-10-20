//! Command-line adapter that calls the running Forge daemon.

use std::{env, time::Duration};

use forge_rpc::{SystemApiClient, SystemStatusResponse};
use jsonrpsee::http_client::HttpClientBuilder;

/// RPC endpoint used when no endpoint configuration is supplied.
pub const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:45003/rpc";
/// The maximum time a status request may wait for the daemon.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Resolves the daemon endpoint, preferring an explicit command-line value.
#[must_use]
pub fn server_url(flag_server_url: Option<&str>, env_server_url: Option<&str>) -> String {
    flag_server_url
        .or(env_server_url)
        .unwrap_or(DEFAULT_SERVER_URL)
        .to_owned()
}

/// Resolves the daemon endpoint from command-line and process configuration.
#[must_use]
pub fn server_url_from_process(flag_server_url: Option<&str>) -> String {
    server_url(
        flag_server_url,
        env::var("FORGE_SERVER_URL").ok().as_deref(),
    )
}

/// Requests the shared status result through the daemon's JSON-RPC endpoint.
///
/// # Errors
///
/// Returns an error when the endpoint is invalid, unavailable, or does not provide
/// the `system.status` RPC method within [`REQUEST_TIMEOUT`].
pub async fn request_status(server_url: &str) -> Result<SystemStatusResponse, String> {
    let client = HttpClientBuilder::default()
        .request_timeout(REQUEST_TIMEOUT)
        .build(server_url)
        .map_err(|error| {
            format!("could not configure Forge daemon endpoint `{server_url}`: {error}")
        })?;

    client.status().await.map_err(|error| {
        format!("could not reach Forge daemon at `{server_url}` within 10 seconds: {error}")
    })
}

/// Renders a concise human-readable daemon status.
#[must_use]
pub fn render_human_status(status: &SystemStatusResponse) -> String {
    let readiness = if status.ready { "ready" } else { "not ready" };
    format!("{} {} ({readiness})", status.name, status.version)
}

/// Renders the stable JSON representation of a daemon status.
///
/// # Panics
///
/// Panics only if serializing the fixed response type unexpectedly fails.
#[must_use]
pub fn render_json_status(status: &SystemStatusResponse) -> String {
    serde_json::to_string(status).expect("SystemStatusResponse must serialize to JSON")
}

#[cfg(test)]
mod tests {
    use forge_app::Application;
    use forge_rpc::SystemStatusResponse;
    use forge_server::{Daemon, router};

    use super::{
        DEFAULT_SERVER_URL, REQUEST_TIMEOUT, render_human_status, render_json_status,
        request_status, server_url,
    };

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
    fn status_output_has_human_and_stable_json_modes() {
        let status = SystemStatusResponse {
            name: "forge".to_owned(),
            version: "0.1.0".to_owned(),
            ready: true,
        };

        assert_eq!(render_human_status(&status), "forge 0.1.0 (ready)");
        assert_eq!(
            render_json_status(&status),
            r#"{"name":"forge","version":"0.1.0","ready":true}"#
        );
    }

    #[tokio::test]
    async fn requests_the_shared_status_from_a_running_daemon() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router(Daemon::new(Application::new())))
                .await
                .unwrap();
        });

        let status = request_status(&format!("http://{address}/rpc"))
            .await
            .unwrap();
        assert_eq!(status.name, "forge");
        assert_eq!(status.version, "0.1.0");
        assert!(status.ready);

        server.abort();
    }

    #[tokio::test]
    async fn unavailable_daemon_returns_a_clear_error_without_starting_one() {
        let error = request_status("http://127.0.0.1:0/rpc").await.unwrap_err();

        assert_eq!(REQUEST_TIMEOUT.as_secs(), 10);
        assert!(error.starts_with("could not reach Forge daemon"));
    }
}
