use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, Application, DispatchWorkspace,
    Project, ProjectRepository, RecordType, RegistrationError, RepositoryInspector,
    WorkspaceDispatchRepository, WorkspaceRepository,
};
use forge_rpc::AgentDispatchApiClient;
use forge_server::{Daemon, router};
use forge_store::SqliteProjectRepository;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Default)]
struct Provider {
    offline: Arc<AtomicBool>,
    invalid: Arc<AtomicBool>,
}
impl AgentDispatchAdapterFactory for Provider {
    fn validate_profile(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        if self.invalid.load(Ordering::SeqCst) && profile.record_type == RecordType::Grilling {
            Err(AgentAdapterError::InvalidRequest(
                "unsupported profile".into(),
            ))
        } else {
            Ok(())
        }
    }
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        Ok(Box::new(self.clone()))
    }
}
impl AgentDispatchAdapter for Provider {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        if self.offline.load(Ordering::SeqCst) {
            Err(AgentAdapterError::Endpoint("unreachable".into()))
        } else {
            Ok(())
        }
    }
    fn verify_workspace(&self, id: &str) -> Result<DispatchWorkspace, AgentAdapterError> {
        self.probe()?;
        Ok(DispatchWorkspace { id: id.into() })
    }
    fn provision(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        panic!("management/readiness must not provision")
    }
    fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        panic!("management/readiness must not observe assignments")
    }
    fn recover(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        panic!("management/readiness must not recover assignments")
    }
    fn retire(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        panic!("configuration must not retire provider resources")
    }
}
struct NoInspector;
impl RepositoryInspector for NoInspector {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        panic!("no Git required for planning")
    }
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "forge-workspace-dispatch-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> SqliteProjectRepository {
        SqliteProjectRepository::open(&self.0.join("store.sqlite")).unwrap()
    }
    fn app(&self, provider: Provider) -> Application {
        Application::with_agent_dispatch(
            Box::new(self.store()),
            Box::new(self.store()),
            Box::new(provider),
            Box::new(NoInspector),
        )
        .with_workspace_repository(Box::new(self.store()))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(&String::from_utf8(output.stdout).unwrap()).unwrap()
}
fn rejected(output: Output, expected: &str) {
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains(expected), "{error}");
}
fn blocked(status: &Value, code: &str) -> bool {
    status["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["code"] == code)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn configuration_readiness_and_planning_route_by_workspace_without_a_default() {
    let fixture = Fixture::new();
    let store = fixture.store();
    for workspace in ["one", "two"] {
        let root = fixture.0.join(workspace);
        std::fs::create_dir_all(&root).unwrap();
        store.create_workspace(workspace, workspace, &root).unwrap();
        store
            .register(&Project {
                reference: format!("{workspace}/api"),
                workspace_id: workspace.into(),
                slug: "api".into(),
                project_root: root.canonicalize().unwrap(),
                origin_url: "https://example.test/repo.git".into(),
            })
            .unwrap();
    }
    let provider = Provider::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/rpc", listener.local_addr().unwrap());
    let app = fixture.app(provider.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router(Daemon::new(app)))
            .await
            .unwrap();
    });
    let run = |cwd: &Path, workspace: Option<&str>, args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
        command
            .current_dir(cwd)
            .args(args)
            .args(["--server-url", &url, "--json"]);
        if let Some(workspace) = workspace {
            command.args(["--workspace", workspace]);
        }
        command.output().unwrap()
    };
    let one = fixture.0.join("one");
    let initial = success(run(&one, None, &["dispatcher", "status"]));
    assert_eq!(initial["workspace_id"], "one");
    assert_eq!(initial["configured"], false);
    for code in [
        "task-profile-missing",
        "grilling-profile-missing",
        "binding-missing",
    ] {
        assert!(blocked(&initial, code));
    }
    rejected(
        run(&fixture.0, None, &["dispatcher", "config", "get"]),
        "Workspace",
    );
    rejected(
        run(&fixture.0, None, &["dispatcher", "status"]),
        "Workspace",
    );
    for workspace in ["one", "two"] {
        for key in ["task-profile", "grilling-profile"] {
            success(run(
                &fixture.0,
                Some(workspace),
                &["dispatcher", "config", "set", key, "1"],
            ));
        }
        success(run(
            &fixture.0,
            Some(workspace),
            &[
                "dispatcher",
                "config",
                "set",
                "workspace",
                &format!("herdr-{workspace}"),
            ],
        ));
        assert_eq!(
            success(run(&fixture.0, Some(workspace), &["dispatcher", "status"]))["dispatchable"],
            true
        );
    }
    // Explicit routing wins over cwd, while conflicting explicit selectors reject before writes.
    let selected = success(run(
        &one,
        Some("two"),
        &[
            "dispatcher",
            "config",
            "set",
            "project-limit",
            "3",
            "--project",
            "api",
        ],
    ));
    assert_eq!(selected["entries"][0]["ownership"], "project:two/api");
    assert_eq!(
        store
            .effective_project_task_concurrency_limit("one/api")
            .unwrap(),
        1
    );
    rejected(
        run(
            &one,
            Some("one"),
            &[
                "dispatcher",
                "config",
                "set",
                "project-limit",
                "4",
                "--project",
                "two/api",
            ],
        ),
        "conflict",
    );
    assert_eq!(
        store
            .effective_project_task_concurrency_limit("two/api")
            .unwrap(),
        3
    );
    let qualified = success(run(
        &fixture.0,
        None,
        &["dispatcher", "status", "--project", "two/api"],
    ));
    assert_eq!(qualified["workspace_id"], "two");
    rejected(
        run(
            &fixture.0,
            Some("two"),
            &["dispatcher", "config", "set", "workspace", "herdr-one"],
        ),
        "already bound",
    );
    rejected(
        run(
            &fixture.0,
            Some("one"),
            &["dispatcher", "config", "set", "task-profile", "999"],
        ),
        "revision",
    );
    success(run(
        &fixture.0,
        Some("one"),
        &[
            "dispatcher",
            "config",
            "set",
            "acknowledgement-timeout-ms",
            "42000",
        ],
    ));
    let profile = success(run(&fixture.0, Some("one"), &["dispatcher", "profile"]));
    assert_eq!(profile["acknowledgement_timeout_millis"], 42000);
    assert_eq!(
        success(run(&one, Some("two"), &["dispatcher", "profile"]))["acknowledgement_timeout_millis"],
        300_000
    );
    assert_eq!(
        success(run(
            &one,
            None,
            &["dispatcher", "profile", "--kind", "grilling"]
        ))["record_type"],
        "grilling"
    );
    provider.invalid.store(true, Ordering::SeqCst);
    let invalid = success(run(&one, None, &["dispatcher", "status"]));
    assert!(!invalid["configured"].as_bool().unwrap());
    assert!(blocked(&invalid, "grilling-profile-invalid"));
    provider.invalid.store(false, Ordering::SeqCst);
    assert_eq!(
        success(run(&one, None, &["dispatcher", "status"]))["dispatchable"],
        true
    );
    provider.offline.store(true, Ordering::SeqCst);
    let unavailable = success(run(&one, None, &["dispatcher", "status"]));
    assert_eq!(unavailable["configured"], true);
    assert_eq!(unavailable["dispatchable"], false);
    assert!(blocked(&unavailable, "provider-unreachable"));
    success(run(&one, None, &["dispatcher", "config", "get"]));
    success(run(
        &one,
        None,
        &["dispatcher", "config", "set", "task-profile", "1"],
    ));
    let planned = success(run(
        &one,
        None,
        &["wayfinder", "open", "--body", "plan without provider"],
    ));
    assert_eq!(planned["workspace_id"], "one");
    rejected(
        run(
            &fixture.0,
            Some("two"),
            &["dispatcher", "dispatch", &planned["id"].to_string()],
        ),
        "selected Workspace",
    );
    let ready = success(run(&one, None, &["ready", "--planning"]));
    assert!(ready.to_string().contains("plan without provider"));
    let client = jsonrpsee::http_client::HttpClientBuilder::default()
        .build(&url)
        .unwrap();
    let before = store.workspace_dispatch_configuration("one").unwrap();
    let bad = client
        .config_set(forge_rpc::SelectedRecordRequest {
            selection: forge_rpc::RecordSelection {
                workspace: Some("one".into()),
                cwd: Some("relative".into()),
            },
            record: forge_rpc::DispatcherConfigSetRequest {
                key: "task-profile".into(),
                value: "1".into(),
                project: None,
            },
        })
        .await;
    let error = bad.unwrap_err().to_string();
    assert!(error.contains("absolute"), "{error}");
    assert_eq!(
        store.workspace_dispatch_configuration("one").unwrap(),
        before
    );
    let no_selection = client
        .config_get(forge_rpc::SelectedRecordRequest {
            selection: forge_rpc::RecordSelection::default(),
            record: forge_rpc::DispatcherConfigGetRequest {
                key: None,
                project: None,
            },
        })
        .await;
    assert!(no_selection.is_err(), "daemon cwd must not route a request");
    server.abort();
    let _ = server.await;
}
