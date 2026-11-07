use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, Application, DispatchWorkspace,
    NewTypedRecord, Project, ProjectRepository, RecordRepository, RecordType, RegistrationError,
    RepositoryInspector,
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
    let db = std::env::temp_dir().join(format!(
        "forge-dispatch-config-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SqliteProjectRepository::open(&db).unwrap();
    store
        .register(&Project {
            slug: "alpha".into(),
            registered_folder: "/repos/alpha".into(),
            origin_url: "https://example.com/alpha".into(),
        })
        .unwrap();
    let app = Application::with_agent_dispatch(
        Box::new(SqliteProjectRepository::in_memory().unwrap()),
        Box::new(SqliteProjectRepository::open(&db).unwrap()),
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
    verify_capacity(&json);
    verify_timeout_revisions(&store, &json);
    let defaults = verify_default_provenance(&db, &run, &json);
    verify_provider_configuration(&store, &run, &json, &defaults);
    server.abort();
    drop(store);
    let _ = std::fs::remove_file(db);
}

fn verify_capacity(json: &impl Fn(&[&str]) -> serde_json::Value) {
    let capacity = json(&["get", "project-limit", "--project", "alpha"]);
    assert_eq!(
        capacity["entries"][0],
        serde_json::json!({"key":"project-limit", "value":"1", "provenance":"default"})
    );
    let capacity = json(&["set", "project-limit", "3", "--project", "alpha"]);
    assert_eq!(capacity["entries"][0]["value"], "3");
    assert_eq!(capacity["entries"][0]["provenance"], "override");
    let aggregate = json(&["get", "--project", "alpha"]);
    assert_eq!(aggregate["entries"].as_array().unwrap().len(), 4);
    assert_eq!(aggregate["entries"][3], capacity["entries"][0]);
    json(&["unset", "project-limit", "--project", "alpha"]);
    let capacity = json(&["get", "project-limit", "--project", "alpha"]);
    assert_eq!(capacity["entries"][0]["value"], "1");
    assert_eq!(capacity["entries"][0]["provenance"], "default");
    // Keep room for Assignments used to observe revision binding below.
    json(&["set", "project-limit", "3", "--project", "alpha"]);
}

fn verify_timeout_revisions(
    store: &SqliteProjectRepository,
    json: &impl Fn(&[&str]) -> serde_json::Value,
) {
    let open_task = || {
        store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "configured task".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap()
    };
    open_task();
    let original_assignment = store
        .claim_next_agent_assignment("alpha", "original")
        .unwrap()
        .unwrap();
    let timeout = json(&["set", "acknowledgement-timeout-ms", "42000"]);
    assert_eq!(timeout["entries"][0]["value"], "42000");
    assert_eq!(timeout["entries"][0]["provenance"], "override");
    assert!(
        timeout["applicability"]
            .as_str()
            .unwrap()
            .contains("future Assignments only")
    );
    open_task();
    let updated_assignment = store
        .claim_next_agent_assignment("alpha", "updated")
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_assignment.profile.acknowledgement_timeout_millis,
        42_000
    );
    assert!(updated_assignment.profile.revision > original_assignment.profile.revision);
    assert_eq!(
        store.agent_assignment("original").unwrap().profile,
        original_assignment.profile
    );
    let reset = json(&["unset", "acknowledgement-timeout-ms"]);
    assert_eq!(reset["entries"][0]["value"], "300000");
    assert_eq!(reset["entries"][0]["provenance"], "default");
    assert!(
        reset["applicability"]
            .as_str()
            .unwrap()
            .contains("future Assignments only")
    );
    open_task();
    let default_assignment = store
        .claim_next_agent_assignment("alpha", "restored")
        .unwrap()
        .unwrap();
    assert_eq!(
        default_assignment.profile.acknowledgement_timeout_millis,
        300_000
    );
    assert!(default_assignment.profile.revision > updated_assignment.profile.revision);
    assert_eq!(
        store.agent_assignment("original").unwrap().profile,
        original_assignment.profile
    );
    assert_eq!(
        store.agent_assignment("updated").unwrap().profile,
        updated_assignment.profile
    );
    // Configuration tests below do not retire resources; finish these synthetic attempts first.
    for id in ["original", "updated", "restored"] {
        store
            .close_agent_assignment(id, forge_app::AgentAssignmentClosureReason::Retired)
            .unwrap();
    }
}

fn verify_default_provenance(
    db: &Path,
    run: &impl Fn(&[&str], bool) -> std::process::Output,
    json: &impl Fn(&[&str]) -> serde_json::Value,
) -> serde_json::Value {
    let defaults = json(&["get"]);
    assert_eq!(
        defaults,
        serde_json::json!({"entries":[
            {"key":"herdr-socket", "value":null, "provenance":"default"},
            {"key":"workspace", "value":null, "provenance":"default"},
            {"key":"acknowledgement-timeout-ms", "value":"300000", "provenance":"default"}
        ]})
    );
    // An explicit value equal to the default is still an override, durably.
    let explicit_default = json(&["set", "acknowledgement-timeout-ms", "300000"]);
    assert_eq!(explicit_default["entries"][0]["provenance"], "override");
    assert!(
        SqliteProjectRepository::open(db)
            .unwrap()
            .active_task_agent_profile()
            .unwrap()
            .acknowledgement_timeout_overridden
    );
    let human_timeout = run(&["unset", "acknowledgement-timeout-ms"], false);
    assert!(human_timeout.status.success());
    let human_timeout = String::from_utf8(human_timeout.stdout).unwrap();
    assert!(human_timeout.contains("future Assignments only"));
    assert!(human_timeout.contains("acknowledgement-timeout-ms\t300000\tdefault"));
    assert!(
        !SqliteProjectRepository::open(db)
            .unwrap()
            .active_task_agent_profile()
            .unwrap()
            .acknowledgement_timeout_overridden
    );
    defaults
}

fn verify_provider_configuration(
    store: &SqliteProjectRepository,
    run: &impl Fn(&[&str], bool) -> std::process::Output,
    json: &impl Fn(&[&str]) -> serde_json::Value,
    defaults: &serde_json::Value,
) {
    let profile_before_invalid = store.active_task_agent_profile().unwrap();
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
        vec!["set", "unknown", "value"],
        vec!["get", "project-limit"],
        vec!["unset", "project-limit"],
        vec!["set", "project-limit", "3"],
        vec!["get", "project-limit", "--project", "missing"],
        vec!["set", "project-limit", "3", "--project", "missing"],
        vec!["unset", "project-limit", "--project", "missing"],
        vec!["get", "project-limit", "--project", ""],
        vec!["set", "project-limit", "0", "--project", "alpha"],
        vec!["set", "project-limit", "1025", "--project", "alpha"],
        vec!["set", "project-limit", "-1", "--project", "alpha"],
        vec!["set", "project-limit", "word", "--project", "alpha"],
        vec!["set", "project-limit", "--project", "alpha"],
        vec!["set", "acknowledgement-timeout-ms"],
        vec!["set", "acknowledgement-timeout-ms", "0"],
        vec!["set", "acknowledgement-timeout-ms", "word"],
        vec!["set", "acknowledgement-timeout-ms", "18446744073709551615"],
    ] {
        let output = run(&args, true);
        assert!(!output.status.success(), "unexpected success for {args:?}");
        assert!(!output.stderr.is_empty());
    }
    // Rejections do not mutate overrides or create profile revisions.
    assert_eq!(
        store.active_task_agent_profile().unwrap(),
        profile_before_invalid
    );
    assert_eq!(
        json(&["get", "project-limit", "--project", "alpha"])["entries"][0]["value"],
        "3"
    );
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
    assert_eq!(&json(&["get"]), defaults);
}
