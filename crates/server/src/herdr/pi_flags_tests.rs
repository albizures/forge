//! Explicit opt-in real Pi CLI probe; never uses operator credentials or services.
use super::pi_launch_arguments;
use forge_app::{AgentDispatchRepository, AgentProfile, RecordType};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "forge-pi-flags-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        )))
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn profile(&self, selected: bool) -> AgentProfile {
        let tools = if selected { "[read]" } else { "[]" };
        let resources = if selected {
            "skills: [{name: chosen}]\nextensions: [{name: chosen}]\n"
        } else {
            ""
        };
        self.write(
            "catalog/.agents/skills/chosen/SKILL.md",
            "---\nname: chosen\ndescription: chosen-description\n---\nSKILL_BODY_NOT_PRELOADED\n",
        );
        self.write("catalog/.agents/profiles/chosen.md", &format!(
            "---\nversion: 1\nruntime: herdr/pi\nmodel: forge-test/fixture\nthinkingLevel: low\ntools: {tools}\n{resources}---\nPRIVATE_REGISTERED_BEHAVIOR\n"
        ));
        self.write("catalog/.pi/extensions/chosen.mjs", &format!(
            "import {{writeFileSync}} from 'node:fs'; export default function() {{writeFileSync({}, 'loaded');}}",
            serde_json::to_string(&self.0.join("selected-extension-loaded")).unwrap()
        ));
        let catalog = forge_app::agent_profiles::ResourceCatalog::discover(
            &self.0.join("home"),
            &self.0.join("catalog"),
        )
        .unwrap();
        let policy = catalog
            .resolve(&self.0.join("catalog"), ".agents/profiles/chosen.md")
            .unwrap();
        let store = forge_store::SqliteProjectRepository::in_memory().unwrap();
        let mut profile = store.agent_profile(RecordType::Task, 1).unwrap();
        profile.model.clone_from(&policy.model);
        profile.launch_policy = Some(policy);
        profile
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn thinking_flags_apply_to_both_record_types_and_preserve_historical_launches() {
    let fixture = Fixture::new();
    let historical = r#"{"version":1,"runtime":"herdr/pi","model":"test/model","tools":[],"behavior":"old","skills":[],"extensions":[],"mcpServers":[]}"#;
    for kind in [RecordType::Task, RecordType::Grilling] {
        let mut profile = fixture.profile(false);
        profile.record_type = kind;
        for level in ["off", "minimal", "low", "medium", "high", "xhigh"] {
            profile.launch_policy.as_mut().unwrap().thinking_level =
                Some(serde_json::from_value(serde_json::json!(level)).unwrap());
            let args = pi_launch_arguments(&profile, "protected bootstrap").unwrap();
            assert_eq!(args.iter().filter(|arg| *arg == "--thinking").count(), 1);
            assert!(args.windows(2).any(|pair| pair == ["--thinking", level]));
            assert_eq!(&args[args.len() - 2..], ["--", "protected bootstrap"]);
            assert!(
                !args
                    .iter()
                    .any(|arg| arg.contains("PRIVATE_REGISTERED_BEHAVIOR"))
            );
        }
        profile.launch_policy = Some(serde_json::from_str(historical).unwrap());
        assert!(
            !pi_launch_arguments(&profile, "bootstrap")
                .unwrap()
                .iter()
                .any(|arg| arg == "--thinking")
        );
        profile.launch_policy = None;
        assert_eq!(
            pi_launch_arguments(&profile, "bootstrap").unwrap(),
            ["--model", "forge-test/fixture", "bootstrap"]
        );
    }
}

#[test]
fn pi_flags_drift_and_unsupported_mcp_fail_before_provider_or_provision_intent() {
    use forge_app::{AgentDispatchAdapter, NewAgentAssignment, NewTypedRecord, RecordRepository};
    for mcp in [false, true] {
        let fixture = Fixture::new();
        let mut profile = fixture.profile(true);
        if mcp {
            fixture.write("catalog/.pi/mcp.json", r#"{"mcpServers":{"selected":{"command":"unused","env":{"KEY":"CREDENTIAL_SENTINEL"}}}}"#);
            fixture.write("catalog/.agents/profiles/chosen.md", "---\nversion: 1\nruntime: herdr/pi\nmodel: forge-test/fixture\nthinkingLevel: low\ntools: []\nmcpServers: [{name: selected}]\n---\nPRIVATE_REGISTERED_BEHAVIOR\n");
            profile.launch_policy = Some(
                forge_app::agent_profiles::ResourceCatalog::discover(
                    &fixture.0.join("home"),
                    &fixture.0.join("catalog"),
                )
                .unwrap()
                .resolve(&fixture.0.join("catalog"), ".agents/profiles/chosen.md")
                .unwrap(),
            );
        } else {
            fixture.write("catalog/.pi/extensions/chosen.mjs", "drifted entry");
        }
        let store = forge_store::SqliteProjectRepository::in_memory().unwrap();
        crate::test_projects::register(&store, "project", None);
        let record = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "task".into(),
                project_slugs: vec!["project".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let mut assignment = store
            .create_agent_assignment(&NewAgentAssignment {
                id: "flag-preflight".into(),
                record_id: record.id,
                project_slug: "project".into(),
                profile_revision: 1,
            })
            .unwrap();
        assignment.profile = profile;
        let context = forge_app::AgentDispatchContext {
            assignment,
            project_root: "/project".into(),
            agent_workspace: Some("/worktree".into()),
            dispatch_workspace: Some(forge_app::DispatchWorkspace {
                id: "workspace".into(),
            }),
            rendered_initial_prompt: "private instructions".into(),
            bootstrap_prompt: Some("bootstrap".into()),
            capability_delivery: Some(forge_app::AssignmentCapabilityDelivery {
                endpoint: "http://127.0.0.1:45003".into(),
                secret_file: "/secret-path".into(),
                cli_path: "/forge".into(),
            }),
            resources: vec![],
        };
        let adapter = crate::CapabilityDeliveryAdapter {
            inner: super::HerdrAdapter {
                socket_path: fixture.0.join("no-provider.sock"),
            },
            daemon_url: "http://127.0.0.1:45003".into(),
            capability_secret_root: fixture.0.join("capabilities"),
            cli_override: None,
        };
        let Err(forge_app::AgentAdapterError::InvalidRequest(message)) =
            adapter.provision(&context, &store)
        else {
            panic!("invalid policy must fail before connecting to the nonexistent provider");
        };
        assert!(message.contains(if mcp { "unsupported" } else { "drifted" }));
        for forbidden in [
            "CREDENTIAL_SENTINEL",
            "PRIVATE_REGISTERED_BEHAVIOR",
            "/secret-path",
        ] {
            assert!(!message.contains(forbidden));
        }
        assert!(
            !fixture.0.join("capabilities").exists(),
            "invalid policy must not issue a capability"
        );
        assert_eq!(store.agent_launch_intent("flag-preflight").unwrap(), None);
        assert!(
            store
                .list_agent_assignment_resources("flag-preflight")
                .unwrap()
                .is_empty()
        );
        assert!(
            store.begin_agent_provision("flag-preflight").unwrap(),
            "must not consume provision intent"
        );
    }
}

#[test]
fn registered_grilling_receives_protected_capability_from_production_adapter() {
    use forge_app::{NewAgentAssignment, NewTypedRecord, RecordRepository};
    use std::os::unix::fs::PermissionsExt;
    if std::env::var_os("FORGE_CONTEXT_CHILD").is_none() {
        let fixture = Fixture::new();
        fs::create_dir_all(&fixture.0).unwrap();
        let executable = fixture.0.join("forge-server");
        fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let cli = fixture.0.join("forge");
        for available in [false, true] {
            if available {
                fs::write(&cli, "#!/bin/sh\nexit 0\n").unwrap();
                fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
            }
            let output = Command::new(&executable)
                .args(["--exact", "herdr::pi_flags_tests::registered_grilling_receives_protected_capability_from_production_adapter", "--nocapture"])
                .env("FORGE_CONTEXT_CHILD", if available { "available" } else { "missing" }).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    }
    let fixture = Fixture::new();
    let mut profile = fixture.profile(false);
    profile.record_type = RecordType::Grilling;
    let store = forge_store::SqliteProjectRepository::in_memory().unwrap();
    crate::test_projects::register(&store, "project", None);
    let record = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Grilling,
            body: "Grilling".into(),
            project_slugs: vec!["project".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let mut assignment = store
        .create_agent_assignment(&NewAgentAssignment {
            id: "grilling-delivery".into(),
            record_id: record.id,
            project_slug: "project".into(),
            profile_revision: 1,
        })
        .unwrap();
    assignment.profile = profile;
    let context = forge_app::AgentDispatchContext {
        assignment,
        project_root: fixture.0.join("catalog"),
        agent_workspace: Some(fixture.0.join("catalog")),
        dispatch_workspace: None,
        rendered_initial_prompt: "private instructions".into(),
        bootstrap_prompt: None,
        capability_delivery: None,
        resources: vec![],
    };
    let adapter = crate::CapabilityDeliveryAdapter {
        inner: super::HerdrAdapter {
            socket_path: fixture.0.join("unused.sock"),
        },
        daemon_url: "http://127.0.0.1:45003".into(),
        capability_secret_root: fixture.0.join("secrets"),
        cli_override: None,
    };
    if std::env::var("FORGE_CONTEXT_CHILD").unwrap() == "missing" {
        assert!(adapter.context(&context).is_err());
        assert!(!fixture.0.join("secrets").exists());
        return;
    }
    let delivered = adapter.context(&context).unwrap();
    assert!(
        delivered
            .bootstrap_prompt
            .unwrap()
            .contains("assignment bootstrap")
    );
    let delivery = delivered.capability_delivery.unwrap();
    assert_eq!(delivery.cli_path.file_name().unwrap(), "forge");
    assert_eq!(
        fs::metadata(&delivery.secret_file)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::read_to_string(delivery.secret_file)
            .unwrap()
            .trim()
            .len(),
        64
    );
}

#[test]
fn direct_input_drift_after_tab_allocation_blocks_launch_and_retains_evidence() {
    use forge_app::{AgentDispatchAdapter, NewAgentAssignment, NewTypedRecord, RecordRepository};
    use std::os::unix::net::UnixListener;
    let fixture = Fixture::new();
    let profile = fixture.profile(true);
    let store = forge_store::SqliteProjectRepository::in_memory().unwrap();
    crate::test_projects::register(&store, "project", None);
    let record = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "task".into(),
            project_slugs: vec!["project".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let mut assignment = store
        .create_agent_assignment(&NewAgentAssignment {
            id: "drift-after-tab".into(),
            record_id: record.id,
            project_slug: "project".into(),
            profile_revision: 1,
        })
        .unwrap();
    assignment.profile = profile;
    let context = forge_app::AgentDispatchContext {
        assignment,
        project_root: "/project".into(),
        agent_workspace: Some("/worktree".into()),
        dispatch_workspace: Some(forge_app::DispatchWorkspace {
            id: "workspace".into(),
        }),
        rendered_initial_prompt: "private".into(),
        bootstrap_prompt: Some("bootstrap".into()),
        capability_delivery: Some(forge_app::AssignmentCapabilityDelivery {
            endpoint: "http://127.0.0.1:45003".into(),
            secret_file: "/secret".into(),
            cli_path: "/forge".into(),
        }),
        resources: vec![],
    };
    let socket = fixture.0.join("herdr.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let extension = fixture.0.join("catalog/.pi/extensions/chosen.mjs");
    let server = thread::spawn(move || {
        for (method, result) in [
            ("ping", serde_json::json!({"type":"pong","protocol":22})),
            (
                "session.snapshot",
                serde_json::json!({"type":"session_snapshot","snapshot":{"protocol":22,"schema_version":1,"workspaces":[]}}),
            ),
            (
                "workspace.get",
                serde_json::json!({"workspace":{"workspace_id":"workspace"}}),
            ),
            (
                "tab.create",
                serde_json::json!({"tab":{"tab_id":"tab"},"root_pane":{"pane_id":"pane"}}),
            ),
        ] {
            let mut stream = listener.accept().unwrap().0;
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], method);
            if method == "tab.create" {
                fs::write(&extension, "changed after allocation").unwrap();
            }
            serde_json::to_writer(&mut stream, &serde_json::json!({"result":result})).unwrap();
            stream.write_all(b"\n").unwrap();
        }
    });
    let adapter = super::HerdrAdapter {
        socket_path: socket,
    };
    assert!(matches!(
        adapter.provision(&context, &store),
        Err(forge_app::AgentAdapterError::InvalidRequest(_))
    ));
    server.join().unwrap();
    let retained = store
        .list_agent_assignment_resources("drift-after-tab")
        .unwrap();
    assert!(retained.iter().any(|r| r.resource_kind == "tab"));
    assert!(retained.iter().any(|r| r.resource_kind == "pane"));
    assert!(
        store
            .agent_launch_intent("drift-after-tab")
            .unwrap()
            .is_none()
    );
}

#[test]
fn pi_flags_reject_non_utf8_selected_paths_without_substitution() {
    use std::os::unix::ffi::OsStringExt;
    let fixture = Fixture::new();
    let mut profile = fixture.profile(true);
    let entry = &mut profile.launch_policy.as_mut().unwrap().extensions[0];
    let path = fixture
        .0
        .join(std::ffi::OsString::from_vec(b"entry-\xff.mjs".to_vec()));
    fs::rename(&entry.path, &path).unwrap();
    entry.path = path;
    assert!(matches!(
        pi_launch_arguments(&profile, "bootstrap"),
        Err(forge_app::AgentAdapterError::InvalidRequest(_))
    ));
}

#[test]
#[ignore = "requires FORGE_TEST_PI pointing to a real Pi executable; isolated loopback-only model"]
fn pi_flags_runtime_selected_and_empty_resources() {
    let pi = PathBuf::from(std::env::var_os("FORGE_TEST_PI").expect("set FORGE_TEST_PI"));
    assert!(
        pi.is_absolute(),
        "FORGE_TEST_PI must be an absolute executable path"
    );
    for selected in [true, false] {
        probe(&pi, selected);
    }
}

fn install_ambient(fixture: &Fixture) {
    fixture.write("home/.pi/agent/settings.json", r#"{"defaultTools":["bash","write"],"defaultProjectTrust":"always","retry":{"enabled":false}}"#);
    fixture.write(
        "execution/.pi/settings.json",
        r#"{"defaultTools":["write"],"defaultProjectTrust":"always"}"#,
    );
    fixture.write("AGENTS.md", "ANCESTOR_INSTRUCTIONS_SENTINEL");
    fixture.write("execution/AGENTS.md", "REPO_INSTRUCTIONS_SENTINEL");
    fixture.write("execution/read-target.txt", "READ_TOOL_RESULT_SENTINEL");
    for directory in ["home/.pi/agent", "execution/.pi"] {
        fixture.write(&format!("{directory}/SYSTEM.md"), "AMBIENT_SYSTEM_SENTINEL");
        fixture.write(
            &format!("{directory}/APPEND_SYSTEM.md"),
            "AMBIENT_APPEND_SENTINEL",
        );
        fixture.write(
            &format!("{directory}/prompts/probe.md"),
            "AMBIENT_TEMPLATE_SENTINEL",
        );
        fixture.write(
            &format!("{directory}/themes/broken.json"),
            "INVALID_AMBIENT_THEME",
        );
        fixture.write(
            &format!("{directory}/skills/ambient/SKILL.md"),
            "---\nname: ambient\ndescription: AMBIENT_SKILL_SENTINEL\n---\nbody\n",
        );
        fixture.write(&format!("{directory}/extensions/ambient.mjs"), &format!(
            "import {{writeFileSync}} from 'node:fs'; export default function() {{writeFileSync({}, 'unexpected');}}",
            serde_json::to_string(&fixture.0.join("ambient-extension-loaded")).unwrap()
        ));
        let mut servers = serde_json::Map::new();
        for exposure in ["direct", "deferred", "hidden", "codemode"] {
            servers.insert(exposure.into(), serde_json::json!({
                "command": "/bin/sh", "args": ["-c", "touch \"$1\"", "fixture", fixture.0.join("ambient-mcp-started")],
                "exposure": exposure
            }));
        }
        fixture.write(
            &format!("{directory}/mcp.json"),
            &serde_json::json!({"mcpServers": servers}).to_string(),
        );
    }
}

fn probe(pi: &Path, selected: bool) {
    let fixture = Fixture::new();
    install_ambient(&fixture);
    let profile = fixture.profile(selected);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    fixture.write(
        "home/.pi/agent/models.json",
        &serde_json::json!({
            "providers": {"forge-test": {
                "baseUrl": format!("http://{}/v1", listener.local_addr().unwrap()),
                "api": "openai-completions", "apiKey": "fixture-only-key", "authHeader": true,
                "models": [{"id":"fixture", "name":"Fixture", "reasoning":false,
                    "contextWindow":32768, "maxTokens":1024,
                    "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]
            }}
        })
        .to_string(),
    );
    let server = thread::spawn(move || model_server(&listener, selected));
    let stdout = fixture.0.join("stdout");
    let stderr = fixture.0.join("stderr");
    let args = pi_launch_arguments(&profile, "/probe").unwrap();
    let mut child = Command::new(pi)
        .env_clear()
        .env("HOME", fixture.0.join("home"))
        .env("PI_CODING_AGENT_DIR", fixture.0.join("home/.pi/agent"))
        .env(
            "PATH",
            std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()),
        )
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("PI_OFFLINE", "1")
        .current_dir(fixture.0.join("execution"))
        .args(["--offline", "--no-session", "--print", "--mode", "json"])
        .args(args)
        .stdin(Stdio::null())
        .stdout(fs::File::create(&stdout).unwrap())
        .stderr(fs::File::create(&stderr).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(25);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "Pi probe timed out: {}",
                fs::read_to_string(&stderr).unwrap()
            );
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "Pi failed: {}\n{}",
        fs::read_to_string(&stderr).unwrap(),
        fs::read_to_string(&stdout).unwrap()
    );
    let requests = server.join().unwrap();
    assert_requests(&requests, selected);
    assert_eq!(
        fixture.0.join("selected-extension-loaded").exists(),
        selected
    );
    assert!(!fixture.0.join("ambient-extension-loaded").exists());
    assert!(!fixture.0.join("ambient-mcp-started").exists());
}

fn assert_requests(requests: &[serde_json::Value], selected: bool) {
    let request = &requests[0];
    assert_eq!(request["model"], "fixture");
    let serialized = request.to_string();
    for sentinel in [
        "ANCESTOR_INSTRUCTIONS_SENTINEL",
        "REPO_INSTRUCTIONS_SENTINEL",
        "/probe",
    ] {
        assert!(serialized.contains(sentinel), "missing {sentinel}");
    }
    for sentinel in [
        "AMBIENT_SYSTEM_SENTINEL",
        "AMBIENT_APPEND_SENTINEL",
        "AMBIENT_TEMPLATE_SENTINEL",
        "AMBIENT_SKILL_SENTINEL",
        "PRIVATE_REGISTERED_BEHAVIOR",
        "SKILL_BODY_NOT_PRELOADED",
    ] {
        assert!(!serialized.contains(sentinel), "unexpected {sentinel}");
    }
    let tools: Vec<_> = request["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    if selected {
        assert_eq!(tools, ["read"]);
        assert!(serialized.contains("chosen-description"));
        assert!(
            requests[1]
                .to_string()
                .contains("READ_TOOL_RESULT_SENTINEL")
        );
    } else {
        assert!(tools.is_empty());
        assert!(!serialized.contains("chosen-description"));
    }
}

fn model_server(listener: &TcpListener, read_tool: bool) -> Vec<serde_json::Value> {
    listener.set_nonblocking(true).unwrap();
    let mut requests = Vec::new();
    for turn in 0..if read_tool { 2 } else { 1 } {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "Pi did not call fixture model");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("{error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut length = 0;
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            assert!(!line.is_empty(), "incomplete fixture request");
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
            headers.push_str(&line);
        }
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-only-key")
        );
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        requests.push(serde_json::from_slice(&body).unwrap());
        let tool_call = read_tool && turn == 0;
        let delta = if tool_call {
            serde_json::json!({"role":"assistant", "tool_calls":[{"index":0,"id":"read-1","type":"function","function":{"name":"read","arguments":"{\"path\":\"read-target.txt\"}"}}]})
        } else {
            serde_json::json!({"role":"assistant","content":"fixture ok"})
        };
        let chunk = serde_json::json!({"id":"fixture", "object":"chat.completion.chunk", "choices":[{"index":0,"delta":delta,"finish_reason":null}]});
        let end = serde_json::json!({"id":"fixture", "object":"chat.completion.chunk", "choices":[{"index":0,"delta":{},"finish_reason":if tool_call {"tool_calls"} else {"stop"}}]});
        let body = format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n");
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    }
    requests
}
