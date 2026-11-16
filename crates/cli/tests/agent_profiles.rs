use forge_app::{
    AgentDispatchRepository, Application, Project, RecordType, RegistrationError,
    RepositoryInspector, WorkspaceDispatchRepository, WorkspaceRepository,
};
use forge_rpc::AgentProfileApiClient;
use forge_server::{Daemon, router};
use forge_store::SqliteProjectRepository;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct NoProvider;
impl forge_app::AgentDispatchAdapterFactory for NoProvider {
    fn validate_profile(
        &self,
        _: &forge_app::AgentProfile,
    ) -> Result<(), forge_app::AgentAdapterError> {
        Err(forge_app::AgentAdapterError::Endpoint(
            "runtime unavailable".into(),
        ))
    }
    fn adapter_for(
        &self,
        _: &forge_app::AgentProfile,
    ) -> Result<Box<dyn forge_app::AgentDispatchAdapter>, forge_app::AgentAdapterError> {
        panic!("profile commands must not allocate providers")
    }
}
struct NoInspector;
impl RepositoryInspector for NoInspector {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        panic!("profile commands do not inspect repositories")
    }
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forge-profile-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn store(&self) -> SqliteProjectRepository {
        SqliteProjectRepository::open(&self.0.join("store.sqlite")).unwrap()
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn preview_registration_and_explicit_selections_are_isolated_immutable_and_side_effect_free()
{
    let fixture = Fixture::new();
    let store = fixture.store();
    let root = fixture.0.join("workspace");
    let profiles = root.join(".agents/profiles");
    std::fs::create_dir_all(&profiles).unwrap();
    let source = profiles.join("example.md");
    let skill = root.join(".agents/skills/example");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: example\ndescription: fixture\n---\n",
    )
    .unwrap();
    let document = "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: [read]\nskills: [{name: example}]\nextensions: []\nmcpServers: []\n---\nBEHAVIOR_SENTINEL\n";
    std::fs::write(&source, document).unwrap();
    store.create_workspace("one", "One", &root).unwrap();
    let app = Application::with_agent_dispatch(
        Box::new(fixture.store()),
        Box::new(fixture.store()),
        Box::new(NoProvider),
        Box::new(NoInspector),
    )
    .with_workspace_repository(Box::new(fixture.store()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/rpc", listener.local_addr().unwrap());
    let operator = fixture.0.join("operator");
    std::fs::create_dir_all(operator.join(".pi/extensions")).unwrap();
    let marker = fixture.0.join("EXECUTED");
    std::fs::write(operator.join(".pi/extensions/sentinel.ts"), format!("import {{ writeFileSync }} from 'node:fs'; writeFileSync({marker:?}, 'executed'); export default () => {{}};")).unwrap();
    std::fs::create_dir_all(root.join(".pi")).unwrap();
    std::fs::write(root.join(".pi/mcp.json"), serde_json::json!({"mcpServers": {"sentinel": {"command": "sh", "args": ["-c", format!("touch {}", marker.display())], "env": {"TOKEN": "CREDENTIAL_SENTINEL"}}}}).to_string()).unwrap();
    let document = document
        .replace("extensions: []", "extensions: [{name: sentinel}]")
        .replace("mcpServers: []", "mcpServers: [{name: sentinel}]");
    std::fs::write(&source, &document).unwrap();
    let second_root = fixture.0.join("two");
    std::fs::create_dir_all(&second_root).unwrap();
    store.create_workspace("two", "Two", &second_root).unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router(Daemon::new(app).with_profile_operator_root(operator)),
        )
        .await
        .unwrap();
    });
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .current_dir(&fixture.0)
        .args([
            "agent-profile",
            "preview",
            ".agents/profiles/example.md",
            "--workspace",
            "one",
            "--server-url",
            &url,
            "--json",
        ])
        .output()
        .unwrap();
    let preview = success(output);
    assert_eq!(preview["workspace"], "one");
    assert_eq!(preview["policy"]["model"], "openai/gpt-5");
    assert_eq!(preview["policy"]["thinking_level"], "low");
    assert_eq!(
        preview["policy"]["skills"][0]["path"],
        skill
            .join("SKILL.md")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert!(!preview.to_string().contains("BEHAVIOR_SENTINEL"));
    assert!(preview["limitations"].as_array().unwrap().len() >= 3);
    let limitations = preview["limitations"].to_string();
    assert!(limitations.contains("do not attest runtime identity or closed enforcement"));
    assert!(limitations.contains("Selected MCP servers are unsupported at launch"));
    assert!(limitations.contains("native reload and ambient user settings are not guarded"));
    assert!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .task_profile
            .is_none()
    );
    assert!(store.agent_profile(RecordType::Task, 2).is_err());
    assert!(!preview.to_string().contains("CREDENTIAL_SENTINEL"));
    assert_eq!(preview["policy"]["extensions"].as_array().unwrap().len(), 1);
    assert_eq!(
        preview["policy"]["mcp_servers"].as_array().unwrap().len(),
        1
    );
    assert!(!marker.exists());

    let cli = |workspace: &str, arguments: &[&str], json: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
        command.current_dir(&fixture.0).args(arguments).args([
            "--workspace",
            workspace,
            "--server-url",
            &url,
        ]);
        if json {
            command.arg("--json");
        }
        command.output().unwrap()
    };
    for field in [
        "",
        "thinkingLevel: null\n",
        "thinkingLevel: !low null\n",
        "thinkingLevel: !low\n",
        "thinkingLevel: !low low\n",
        "thinkingLevel: !custom low\n",
        "thinkingLevel: 3\n",
        "thinkingLevel: []\n",
        "thinkingLevel: {}\n",
        "thinkingLevel: HIGH\n",
        "thinkingLevel: unsupported\n",
    ] {
        std::fs::write(&source, document.replace("thinkingLevel: low\n", field)).unwrap();
        for operation in ["preview", "register"] {
            let output = cli(
                "one",
                &[
                    "agent-profile",
                    operation,
                    ".agents/profiles/example.md",
                    "--record-type",
                    "task",
                ],
                true,
            );
            assert!(!output.status.success(), "{operation}: {field}");
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(error.contains("thinkingLevel"), "{error}");
            assert!(
                error.contains("off, minimal, low, medium, high, xhigh"),
                "{error}"
            );
            assert!(!error.contains("BEHAVIOR_SENTINEL"));
        }
    }
    std::fs::write(&source, &document).unwrap();
    // Both production operations enforce the new authoring boundary without
    // consuming revisions or exposing source contents in errors.
    let legacy = skill.join("forge-agent.md");
    std::fs::write(&legacy, &document).unwrap();
    std::fs::write(root.join("outside.md"), &document).unwrap();
    for path in [".agents/skills/example/forge-agent.md", "outside.md"] {
        for operation in ["preview", "register"] {
            let output = cli(
                "one",
                &["agent-profile", operation, path, "--record-type", "task"],
                true,
            );
            assert!(!output.status.success());
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(error.contains("within .agents/profiles"));
            assert!(!error.contains("BEHAVIOR_SENTINEL"));
            assert!(!error.contains("CREDENTIAL_SENTINEL"));
        }
    }
    assert!(store.agent_profile(RecordType::Task, 2).is_err());
    std::fs::remove_file(legacy).unwrap();
    let readable = cli(
        "one",
        &["agent-profile", "preview", ".agents/profiles/example.md"],
        false,
    );
    assert!(readable.status.success());
    let text = String::from_utf8(readable.stdout).unwrap();
    assert!(text.contains("Preview only; registration and selection unchanged."));
    assert!(text.contains("SHA-256"));
    assert!(text.contains("Thinking level: low"));
    assert!(text.contains("No OS/same-user isolation"));
    assert!(!text.contains("CREDENTIAL_SENTINEL"));
    assert!(!text.contains("BEHAVIOR_SENTINEL"));
    // The same relative path is NOT resolved from the client cwd or another Workspace.
    let missing = cli(
        "two",
        &["agent-profile", "preview", ".agents/profiles/example.md"],
        true,
    );
    assert!(!missing.status.success());
    let registered = success(cli(
        "one",
        &[
            "agent-profile",
            "register",
            ".agents/profiles/example.md",
            "--record-type",
            "task",
        ],
        true,
    ));
    let revision = u32::try_from(registered["revision"].as_u64().unwrap()).unwrap();
    assert_eq!(revision, 2, "preview must not consume a revision");
    assert_eq!(registered["policy"], preview["policy"]);
    assert!(!registered.to_string().contains("CREDENTIAL_SENTINEL"));
    assert!(!registered.to_string().contains("BEHAVIOR_SENTINEL"));
    let frozen = fixture
        .store()
        .agent_profile(RecordType::Task, revision)
        .unwrap();
    assert_eq!(
        frozen.launch_policy.as_ref().unwrap().behavior,
        "BEHAVIOR_SENTINEL\n"
    );
    assert_eq!(
        frozen.launch_policy.as_ref().unwrap().mcp_servers[0].configuration["env"]["TOKEN"],
        "CREDENTIAL_SENTINEL"
    );
    assert!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .task_profile
            .is_none()
    );
    assert!(
        store
            .workspace_dispatch_configuration("two")
            .unwrap()
            .task_profile
            .is_none()
    );
    assert!(
        !cli("one", &["dispatcher", "profile"], true)
            .status
            .success(),
        "missing selection must fail"
    );
    success(cli(
        "one",
        &[
            "dispatcher",
            "config",
            "set",
            "task-profile",
            &revision.to_string(),
        ],
        true,
    ));
    let selected = store.workspace_dispatch_configuration("one").unwrap();
    assert_eq!(selected.task_profile.unwrap().revision, revision);
    assert!(selected.grilling_profile.is_none());
    let grilling = success(cli(
        "one",
        &[
            "agent-profile",
            "register",
            ".agents/profiles/example.md",
            "--record-type",
            "grilling",
        ],
        true,
    ));
    let grilling_revision = grilling["revision"].as_u64().unwrap().to_string();
    success(cli(
        "one",
        &[
            "dispatcher",
            "config",
            "set",
            "grilling-profile",
            &grilling_revision,
        ],
        true,
    ));
    success(cli(
        "two",
        &[
            "dispatcher",
            "config",
            "set",
            "task-profile",
            &revision.to_string(),
        ],
        true,
    ));
    std::fs::write(
        &source,
        document
            .replace("openai/gpt-5", "anthropic/claude")
            .replace("thinkingLevel: low", "thinkingLevel: xhigh")
            .replace("BEHAVIOR_SENTINEL", "NEW_BEHAVIOR"),
    )
    .unwrap();
    let newer = success(cli(
        "one",
        &[
            "agent-profile",
            "register",
            ".agents/profiles/example.md",
            "--record-type",
            "task",
        ],
        true,
    ));
    assert_eq!(newer["revision"], 3);
    assert_eq!(newer["policy"]["thinking_level"], "xhigh");
    assert_eq!(
        frozen
            .launch_policy
            .as_ref()
            .unwrap()
            .thinking_level
            .unwrap()
            .as_str(),
        "low"
    );
    std::fs::remove_file(&source).unwrap();
    assert_eq!(
        fixture
            .store()
            .agent_profile(RecordType::Task, revision)
            .unwrap(),
        frozen
    );
    for workspace in ["one", "two"] {
        let configuration = fixture
            .store()
            .workspace_dispatch_configuration(workspace)
            .unwrap();
        assert_eq!(configuration.task_profile.unwrap().revision, revision);
        let live = success(cli(workspace, &["dispatcher", "profile"], true));
        assert_eq!(live["model"], "openai/gpt-5");
    }
    assert_eq!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .grilling_profile
            .unwrap()
            .revision
            .to_string(),
        grilling_revision
    );
    assert!(
        store
            .workspace_dispatch_configuration("two")
            .unwrap()
            .grilling_profile
            .is_none()
    );
    assert!(
        !marker.exists(),
        "neither extensions nor MCP commands may execute"
    );
    // The production RPC, not only clap, rejects invalid kinds and malformed data.
    let client = jsonrpsee::http_client::HttpClientBuilder::default()
        .build(&url)
        .unwrap();
    let request = forge_rpc::AgentProfileDocumentRequest {
        workspace: "one".into(),
        document: ".agents/profiles/example.md".into(),
        record_type: "spec".into(),
    };
    let error = client
        .register(request.clone())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("select task or grilling"));
    std::fs::write(
        &source,
        "---\nversion: 1\nthinkingLevel: low\nsecret: CREDENTIAL_SENTINEL\n---\nBEHAVIOR_SENTINEL",
    )
    .unwrap();
    let request = forge_rpc::AgentProfileDocumentRequest {
        record_type: "task".into(),
        ..request
    };
    for error in [
        client.preview(request.clone()).await.unwrap_err(),
        client.register(request).await.unwrap_err(),
    ] {
        let error = error.to_string();
        assert!(error.contains("invalid Agent Profile frontmatter"));
        assert!(!error.contains("CREDENTIAL_SENTINEL"));
        assert!(!error.contains("BEHAVIOR_SENTINEL"));
    }
    assert!(
        store.agent_profile(RecordType::Task, 4).is_err(),
        "failed registration must not create revisions"
    );
    for kind in ["task", "grilling"] {
        for level in ["off", "minimal", "low", "medium", "high", "xhigh"] {
            std::fs::write(
                &source,
                document.replace("thinkingLevel: low", &format!("thinkingLevel: {level}")),
            )
            .unwrap();
            for operation in ["preview", "register"] {
                let response = success(cli(
                    "one",
                    &[
                        "agent-profile",
                        operation,
                        ".agents/profiles/example.md",
                        "--record-type",
                        kind,
                    ],
                    true,
                ));
                assert_eq!(response["policy"]["thinking_level"], level);
            }
        }
    }
    assert!(!marker.exists());
    server.abort();
}
