use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, Application, DispatchWorkspace,
    Project, RegistrationError, RepositoryInspector,
};
use forge_server::{Daemon, router};
use forge_store::SqliteProjectRepository;
use std::{path::Path, process::Command};

struct WorkspaceProvider;
impl AgentDispatchAdapterFactory for WorkspaceProvider {
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        Ok(Box::new(Self))
    }
}
impl AgentDispatchAdapter for WorkspaceProvider {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        panic!("configuration must not probe")
    }
    fn verify_workspace(&self, id: &str) -> Result<DispatchWorkspace, AgentAdapterError> {
        match id {
            "existing" => Ok(DispatchWorkspace { id: id.into() }),
            "mismatch" => Ok(DispatchWorkspace {
                id: "someone-else".into(),
            }),
            _ => Err(AgentAdapterError::NotFound(
                "workspace does not exist".into(),
            )),
        }
    }
    fn provision(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        panic!("configuration must not provision")
    }
    fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        panic!("configuration must not observe")
    }
    fn recover(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        panic!("configuration must not recover")
    }
    fn retire(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        panic!("configuration must not retire Agents or the operator's Workspace")
    }
}
struct NoInspector;
impl RepositoryInspector for NoInspector {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        panic!("no Git inspection")
    }
    fn head_commit(&self, _: &Path) -> Result<String, RegistrationError> {
        panic!("no Git inspection")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grouped_provider_configuration_round_trips_through_cli_and_daemon() {
    let app = Application::with_agent_dispatch(
        Box::new(SqliteProjectRepository::in_memory().unwrap()),
        Box::new(SqliteProjectRepository::in_memory().unwrap()),
        Box::new(WorkspaceProvider),
        Box::new(NoInspector),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/rpc", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router(Daemon::new(app)))
            .await
            .unwrap();
    });
    let run = |args: &[&str], json: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
        command
            .args(["dispatcher", "config"])
            .args(args)
            .args(["--server-url", &url]);
        // Explicit daemon endpoint must win over the environment. Workspace identity must
        // come from the argument, never the environment used by the removed command.
        command.env("FORGE_SERVER_URL", "http://127.0.0.1:1/rpc");
        command.env("HERDR_WORKSPACE_ID", "wrong-ambient-workspace");
        if json {
            command.arg("--json");
        }
        command.output().unwrap()
    };
    let json = |args: &[&str]| {
        let output = run(args, true);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let defaults = json(&["get"]);
    assert_eq!(
        defaults,
        serde_json::json!({"entries":[
            {"key":"herdr-socket", "value":null, "provenance":"default"},
            {"key":"workspace", "value":null, "provenance":"default"}
        ]})
    );
    let socket = json(&["set", "herdr-socket", "/tmp/herdr.sock"]);
    assert_eq!(socket["entries"][0]["value"], "/tmp/herdr.sock");
    assert_eq!(socket["entries"][0]["provenance"], "override");
    let workspace = json(&["set", "workspace", "existing"]);
    assert_eq!(workspace["entries"][0]["value"], "existing");
    assert_eq!(workspace["entries"][0]["provenance"], "override");
    for args in [
        vec!["set", "workspace", "missing"],
        vec!["set", "workspace", "mismatch"],
        vec!["set", "workspace", " "],
        vec!["set", "herdr-socket", "relative.sock"],
        vec!["set", "herdr-socket", ""],
        vec!["set", "workspace"],
        vec!["set", "herdr-socket"],
        vec!["unset", "unknown"],
        vec!["get", "unknown"],
    ] {
        let output = run(&args, true);
        assert!(!output.status.success(), "unexpected success for {args:?}");
        assert!(!output.stderr.is_empty());
    }
    // Rejections do not mutate either override.
    assert_eq!(json(&["get", "workspace"]), workspace);
    assert_eq!(json(&["get", "herdr-socket"]), socket);
    let human = run(&["get", "workspace"], false);
    assert!(human.status.success());
    assert_eq!(
        String::from_utf8(human.stdout).unwrap(),
        "workspace\texisting\toverride\n"
    );
    json(&["unset", "workspace"]);
    json(&["unset", "herdr-socket"]);
    assert_eq!(json(&["get"]), defaults);
    server.abort();
}
