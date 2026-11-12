//! Production dispatch through Herdr's socket seam and the delivered real Forge CLI.
use forge_app::{
    AgentDispatchRepository, DispatchWorkspace, NewTypedRecord, Project, ProjectRepository,
    RecordRepository, RecordType, WorkspaceDispatchRepository, WorkspaceRepository,
};
use forge_server::{DaemonConfig, serve};
use forge_store::SqliteProjectRepository;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "forge-registered-grilling-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn registered_grilling_dispatches_and_bootstraps_with_delivered_cli() {
    if std::env::var_os("FORGE_REGISTERED_LAUNCH_CHILD").is_none() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let child = fixture.0.join("forge-server");
        std::fs::copy(std::env::current_exe().unwrap(), &child).unwrap();
        symlink(env!("CARGO_BIN_EXE_forge"), fixture.0.join("forge")).unwrap();
        let output = tokio::task::spawn_blocking(move || {
            Command::new(child)
                .args([
                    "--exact",
                    "registered_grilling_dispatches_and_bootstraps_with_delivered_cli",
                    "--nocapture",
                ])
                .env("FORGE_REGISTERED_LAUNCH_CHILD", "1")
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    exercise_launch().await;
}

#[allow(clippy::too_many_lines)]
async fn exercise_launch() {
    let fixture = Fixture::new();
    let workspace = fixture.0.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@example.test"],
        vec!["config", "user.name", "Test"],
        vec!["commit", "--allow-empty", "-qm", "base"],
    ] {
        assert!(
            Command::new("git")
                .current_dir(&workspace)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let base = String::from_utf8(
        Command::new("git")
            .current_dir(&workspace)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let db = fixture.0.join("store.sqlite");
    let socket = fixture.0.join("provider.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let calls: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let worker = {
        let calls = calls.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(stream) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                let result = match request["method"].as_str().unwrap() {
                    "ping" => json!({"type":"pong","protocol":22}),
                    "session.snapshot" => {
                        json!({"type":"session_snapshot","snapshot":{"protocol":22,"schema_version":1,"workspaces":[]}})
                    }
                    "workspace.get" => json!({"workspace":{"workspace_id":"provider-workspace"}}),
                    "tab.create" => json!({"tab":{"tab_id":"tab"},"root_pane":{"pane_id":"pane"}}),
                    "agent.start" => {
                        // Authenticate while the launch response is still in flight.
                        let env = {
                            let calls = calls.lock().unwrap();
                            calls.iter().rfind(|c| c["method"] == "tab.create").unwrap()["params"]["env"].as_object().unwrap().clone()
                        };
                        let cli = env["FORGE_ASSIGNMENT_FORGE_CLI"].as_str().unwrap();
                        let mut command = Command::new(cli);
                        command.args(["assignment", "bootstrap", "--json"]);
                        for (key, value) in env {
                            command.env(key, value.as_str().unwrap());
                        }
                        let output = command.output().unwrap();
                        assert!(
                            output.status.success(),
                            "{}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                        let bootstrap: Value = serde_json::from_slice(&output.stdout).unwrap();
                        assert_eq!(bootstrap["context"]["record"]["status"], "in_progress");
                        assert!(bootstrap["behavior"].is_string());
                        json!({"type":"agent_started"})
                    }
                    method => panic!("unexpected provider call {method}"),
                };
                calls.lock().unwrap().push(request);
                serde_json::to_writer(&mut stream, &json!({"result":result})).unwrap();
                stream.write_all(b"\n").unwrap();
            }
        })
    };
    let store = SqliteProjectRepository::open(&db).unwrap();
    store.create_workspace("test", "test", &workspace).unwrap();
    store
        .register(&Project {
            reference: "test/api".into(),
            workspace_id: "test".into(),
            slug: "api".into(),
            project_root: workspace.canonicalize().unwrap(),
            origin_url: "https://example.test/api".into(),
        })
        .unwrap();
    let skill = workspace.join(".agents/skills/entry");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: entry\ndescription: entry\n---\nskill body",
    )
    .unwrap();
    std::fs::write(skill.join("forge-agent.md"), "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\ntools: []\n---\nFROZEN GRILLING BEHAVIOR").unwrap();
    let policy = forge_app::agent_profiles::ResourceCatalog::discover(&fixture.0, &workspace)
        .unwrap()
        .resolve(&workspace, ".agents/skills/entry/forge-agent.md")
        .unwrap();
    let profile = store
        .register_agent_profile("test", RecordType::Grilling, &policy)
        .unwrap();
    let task_profile = store
        .register_agent_profile("test", RecordType::Task, &policy)
        .unwrap();
    store
        .set_project_task_concurrency_limit("test/api", Some(3))
        .unwrap();
    store
        .select_workspace_profile("test", RecordType::Task, Some(task_profile.revision))
        .unwrap();
    store
        .select_workspace_profile("test", RecordType::Grilling, Some(profile.revision))
        .unwrap();
    store.set_herdr_socket_path(Some(&socket)).unwrap();
    let generation = store
        .workspace_dispatch_configuration("test")
        .unwrap()
        .provider_generation;
    store
        .retain_verified_workspace_binding(
            "test",
            Some(&DispatchWorkspace {
                id: "provider-workspace".into(),
            }),
            generation,
        )
        .unwrap();
    let record = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Grilling,
            body: "A decision".into(),
            project_slugs: vec!["test/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    drop(store);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let daemon = tokio::spawn(serve(DaemonConfig {
        bind_addr: address,
        store_path: db.clone(),
        managed: None,
    }));
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(address).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let url = format!("http://{address}/rpc");
    let id = record.id.to_string();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args([
                "dispatcher",
                "dispatch",
                &id,
                "--workspace",
                "test",
                "--server-url",
                &url,
                "--json",
            ])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["assignment"]["state"], "working");
    let assignment = result["assignment"]["id"].as_str().unwrap().to_owned();
    let (env, args, cwd) = {
        let calls = calls.lock().unwrap();
        let tab = calls.iter().find(|c| c["method"] == "tab.create").unwrap();
        let start = calls.iter().find(|c| c["method"] == "agent.start").unwrap();
        (
            tab["params"]["env"].clone(),
            start["params"]["args"].clone(),
            tab["params"]["cwd"].clone(),
        )
    };
    assert_eq!(cwd, workspace.to_str().unwrap());
    assert!(!args.to_string().contains("FROZEN GRILLING BEHAVIOR"));
    assert_eq!(env["FORGE_ASSIGNMENT_ID"], assignment);
    let cli = Path::new(env["FORGE_ASSIGNMENT_FORGE_CLI"].as_str().unwrap()).to_owned();
    let environment = env.as_object().unwrap().clone();
    let bootstrap = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(cli);
        cmd.args(["assignment", "bootstrap", "--json"]);
        for (key, value) in environment {
            cmd.env(key, value.as_str().unwrap());
        }
        cmd.output().unwrap()
    })
    .await
    .unwrap();
    assert!(
        bootstrap.status.success(),
        "{}",
        String::from_utf8_lossy(&bootstrap.stderr)
    );
    let context: Value = serde_json::from_slice(&bootstrap.stdout).unwrap();
    assert_eq!(context["behavior"], "FROZEN GRILLING BEHAVIOR");
    assert_eq!(context["context"]["record"]["id"], record.id);
    assert_eq!(context["context"]["record"]["status"], "in_progress");
    let store = SqliteProjectRepository::open(&db).unwrap();
    assert!(
        store
            .schedule_agent_assignment_retry(
                &assignment,
                forge_app::AgentAssignmentState::Working,
                u64::MAX / 2,
                0
            )
            .unwrap()
    );
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Implement task".into(),
            project_slugs: vec!["test/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    store.set_task_base_commit(task.id, base.trim()).unwrap();
    drop(store);
    let task_id = task.id.to_string();
    let task_url = format!("http://{address}/rpc");
    let task_output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args([
                "dispatcher",
                "dispatch",
                &task_id,
                "--workspace",
                "test",
                "--server-url",
                &task_url,
                "--json",
            ])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(!task_output.status.success());
    assert!(
        String::from_utf8_lossy(&task_output.stderr).contains("task-dispatch-unsupported-contract")
    );
    let store = SqliteProjectRepository::open(&db).unwrap();
    assert!(store.list_agent_assignments(task.id).unwrap().is_empty());
    assert!(
        forge_app::ManagedWorktreeRepository::worktree_claim(&store, task.id)
            .unwrap()
            .is_none()
    );
    store
        .select_workspace_profile("test", RecordType::Grilling, Some(1))
        .unwrap();
    let legacy = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Grilling,
            body: "Legacy decision".into(),
            project_slugs: vec!["test/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    drop(store);
    let legacy_url = format!("http://{address}/rpc");
    let legacy_id = legacy.id.to_string();
    let legacy_output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args([
                "dispatcher",
                "dispatch",
                &legacy_id,
                "--workspace",
                "test",
                "--server-url",
                &legacy_url,
                "--json",
            ])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        legacy_output.status.success(),
        "{}",
        String::from_utf8_lossy(&legacy_output.stderr)
    );
    let legacy_result: Value = serde_json::from_slice(&legacy_output.stdout).unwrap();
    assert_eq!(legacy_result["assignment"]["state"], "working");
    {
        let calls = calls.lock().unwrap();
        let start = calls
            .iter()
            .rfind(|c| c["method"] == "agent.start")
            .unwrap();
        assert!(
            start["params"]["args"]
                .to_string()
                .contains("assignment bootstrap")
        );
        assert!(
            !start["params"]["args"]
                .to_string()
                .contains("Legacy decision")
        );
    }
    for record_id in [record.id, legacy.id] {
        let url = format!("http://{address}/rpc");
        let output = tokio::task::spawn_blocking(move || {
            Command::new(env!("CARGO_BIN_EXE_forge"))
                .args([
                    "dispatcher",
                    "dispatch",
                    &record_id.to_string(),
                    "--workspace",
                    "test",
                    "--server-url",
                    &url,
                    "--json",
                ])
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let duplicate: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(duplicate["already_assigned"], true);
    }
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call["method"] == "agent.start")
            .count(),
        2
    );
    daemon.abort();
    stop.store(true, Ordering::SeqCst);
    worker.join().unwrap();
}
