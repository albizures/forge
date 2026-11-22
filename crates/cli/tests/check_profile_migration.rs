//! Actual CLI -> daemon RPC -> store Check profile migration. No dispatch, runner or provider runs.
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchRepository,
    AgentProfile, Application, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordType, WorkspaceDispatchRepository, WorkspaceRepository,
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
};
use forge_server::{Daemon, GitInspector, router};
use forge_store::SqliteProjectRepository;
use serde_json::Value;
use std::process::Command;

struct Preflight;
impl AgentDispatchAdapterFactory for Preflight {
    fn validate_profile(&self, _: &AgentProfile) -> Result<(), AgentAdapterError> {
        Ok(())
    }
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        Err(AgentAdapterError::InvalidRequest(
            "migration never constructs an adapter".into(),
        ))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn cli_migrates_idempotently_and_preserves_history() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("forge.sqlite");
    std::fs::create_dir_all(root.path().join(".agents/profiles")).unwrap();
    for (name, runtime) in [("old", "herdr/pi"), ("new", "pi-durable")] {
        std::fs::write(
            root.path().join(format!(".agents/profiles/{name}.md")),
            format!("---\nversion: 1\nruntime: {runtime}\nmodel: openai/gpt-4.1\nthinkingLevel: off\ntools: [read]\n---\nWork"),
        )
        .unwrap();
    }
    let store = SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("test", "Test", root.path()).unwrap();
    store
        .register(&Project {
            reference: "test/api".into(),
            workspace_id: "test".into(),
            slug: "api".into(),
            project_root: root.path().canonicalize().unwrap(),
            origin_url: "https://example.test/api.git".into(),
        })
        .unwrap();
    let mut revisions = vec![];
    for name in ["old", "new"] {
        let policy = ResourceCatalog::discover(&root.path().join("operator"), root.path())
            .unwrap()
            .resolve(root.path(), format!(".agents/profiles/{name}.md"))
            .unwrap();
        revisions.push(
            store
                .register_agent_profile("test", RecordType::Task, &policy)
                .unwrap()
                .revision,
        );
    }
    store
        .replace_check_template(
            "test/api",
            &TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Pin".into(),
                criteria: vec![CheckCriterion {
                    name: "Implementation".into(),
                    description: "Deliver".into(),
                    profile_revision: Some(revisions[0]),
                }],
            },
        )
        .unwrap();
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver".into(),
            project_slugs: vec!["test/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let check = store.list_record_checks(task.id).unwrap()[0].id;
    let (id, check_id) = (task.id.to_string(), check.to_string());
    let app = Application::with_agent_dispatch(
        Box::new(SqliteProjectRepository::open(&database).unwrap()),
        Box::new(SqliteProjectRepository::open(&database).unwrap())
            as Box<dyn AgentDispatchRepository>,
        Box::new(Preflight),
        Box::new(GitInspector),
    )
    .with_workspace_repository(Box::new(store));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/rpc", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, router(Daemon::new(app)))
            .await
            .unwrap();
    });
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(args)
            .args(["--server-url", &endpoint])
            .current_dir(root.path())
            .output()
            .unwrap()
    };
    let target = format!("{check_id}={}", revisions[1]);
    let migrate = |op: &str, generation: &str, message: &str| {
        run(&[
            "record",
            "check-profile-migrate",
            &id,
            "--operation-id",
            op,
            "--expected-generation",
            generation,
            "--check",
            &target,
            "--actor",
            "operator",
            "--message",
            message,
            "--json",
        ])
    };
    let stale = migrate("m0", "9", "adopt");
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("stale-work-generation"));
    let first = migrate("m1", "1", "adopt");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["replayed"], false);
    assert_eq!(first["changes"][0]["prior_revision"], revisions[0]);
    assert_eq!(first["changes"][0]["new_revision"], revisions[1]);
    let again: Value = serde_json::from_slice(&migrate("m1", "1", "adopt").stdout).unwrap();
    assert_eq!(again["replayed"], true);
    assert_eq!(again["created_at_millis"], first["created_at_millis"]);
    let conflict = migrate("m1", "1", "different");
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("check-profile-migration-conflict"));
    let unchanged = migrate("m2", "1", "again");
    assert!(String::from_utf8_lossy(&unchanged.stderr).contains("check-profile-unchanged"));
    let history: Value =
        serde_json::from_slice(&run(&["record", "check-profile-migrations", &id, "--json"]).stdout)
            .unwrap();
    assert_eq!(history.as_array().unwrap().len(), 1);
    let human = run(&["record", "check-profile-migrations", &id]);
    assert!(String::from_utf8_lossy(&human.stdout).contains("Check profile migration m1"));
    let checks: Value =
        serde_json::from_slice(&run(&["record", "check", "list", &id, "--json"]).stdout)
            .unwrap_or(Value::Null);
    if let Some(first) = checks.as_array().and_then(|c| c.first()) {
        assert_eq!(first["profile_revision"], revisions[1]);
    }
}
