//! Production composition against the Herdr socket seam (no provider resources by label).
use super::*;
use forge_app::{
    AgentDispatchRepository, ManagedWorktreeRepository, NewTypedRecord, ProjectRepository,
    RecordRepository, RecordType, WorkspaceDispatchRepository, WorkspaceRepository,
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader},
    os::unix::net::UnixListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

// Independent provider fault switches, not product lifecycle state.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
struct ProviderState {
    calls: Vec<Value>,
    missing: bool,
    mismatch: bool,
    gets: usize,
    lose_on_get: Option<usize>,
    agents: std::collections::BTreeMap<String, Value>,
    tab_closed: bool,
    lose_launch_response: bool,
}
struct Provider {
    path: PathBuf,
    state: Arc<Mutex<ProviderState>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Provider {
    #[allow(clippy::too_many_lines)] // Keep the isolated protocol fixture in one place.
    fn new(path: PathBuf) -> Self {
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let state = Arc::new(Mutex::new(ProviderState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let state = state.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(stream) => stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(1));
                            continue;
                        }
                        Err(error) => panic!("{error}"),
                    };
                    let mut line = String::new();
                    BufReader::new(stream.try_clone().unwrap())
                        .read_line(&mut line)
                        .unwrap();
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let response = {
                        let mut state = state.lock().unwrap();
                        state.calls.push(request.clone());
                        let result = match request["method"].as_str().unwrap() {
                            "ping" => json!({"type":"pong", "protocol":22}),
                            "session.snapshot" => {
                                json!({"type":"session_snapshot", "snapshot":{"protocol":22,"schema_version":1,"workspaces":[]}})
                            }
                            "workspace.get" => {
                                state.gets += 1;
                                if state.lose_on_get == Some(state.gets) {
                                    state.missing = true;
                                }
                                if state.missing {
                                    json!({"absent":true})
                                } else {
                                    json!({"workspace":{"workspace_id": if state.mismatch { json!("wrong-identity") } else { request["params"]["workspace_id"].clone() }, "cwd":"/unrelated/focused-pane", "cwd_policy":"home"}})
                                }
                            }
                            "tab.create" => {
                                json!({"tab":{"tab_id":"tab"},"root_pane":{"pane_id":"pane"}})
                            }
                            "agent.start" => {
                                let name = request["params"]["name"].as_str().unwrap().to_owned();
                                let workspace = state
                                    .calls
                                    .iter()
                                    .rev()
                                    .find(|call| call["method"] == "tab.create")
                                    .unwrap()["params"]["workspace_id"]
                                    .clone();
                                state.agents.insert(
                                    name.clone(),
                                    json!({
                                        "name":name, "workspace_id":workspace,
                                        "tab_id":"tab", "pane_id":"pane", "terminal_id":format!("terminal-{name}"),
                                        "agent":"pi", "agent_status":"idle", "interactive_ready":true
                                    }),
                                );
                                json!({"type":"agent_started", "agent":state.agents[&name]})
                            }
                            "agent.get" => {
                                match state
                                    .agents
                                    .get(request["params"]["target"].as_str().unwrap())
                                {
                                    Some(agent) => json!({"type":"agent_info", "agent":agent}),
                                    None => json!({"absent":true}),
                                }
                            }
                            "agent.prompt" => {
                                let name = request["params"]["target"].as_str().unwrap();
                                state.agents.get_mut(name).unwrap()["agent_status"] =
                                    json!("working");
                                json!({"type":"agent_prompted", "agent":state.agents[name]})
                            }
                            "agent.list" => {
                                json!({"type":"agent_list", "agents":state.agents.values().collect::<Vec<_>>()})
                            }
                            "tab.get" | "pane.get" => {
                                if state.tab_closed {
                                    json!({"absent":true})
                                } else {
                                    let workspace = state
                                        .calls
                                        .iter()
                                        .rev()
                                        .find(|call| call["method"] == "tab.create")
                                        .unwrap()["params"]["workspace_id"]
                                        .clone();
                                    json!({"tab_id":"tab", "pane_id":"pane", "workspace_id":workspace})
                                }
                            }
                            "pane.release_agent" => {
                                state.agents.clear();
                                json!({})
                            }
                            "tab.close" => {
                                state.tab_closed = true;
                                json!({})
                            }
                            method => panic!("unexpected provider operation {method}"),
                        };
                        if result["absent"] == true {
                            json!({"error":{"code":"not_found", "message":"missing"}})
                        } else {
                            json!({"result":result})
                        }
                    };
                    if request["method"] == "agent.start"
                        && state.lock().unwrap().lose_launch_response
                    {
                        continue;
                    }
                    serde_json::to_writer(&mut stream, &response).unwrap();
                    stream.write_all(b"\n").unwrap();
                }
            })
        };
        Self {
            path,
            state,
            stop,
            worker: Some(worker),
        }
    }
    fn reset(&self) {
        *self.state.lock().unwrap() = ProviderState::default();
    }
    fn no_provisioning(&self) {
        assert!(self.state.lock().unwrap().calls.iter().all(|call| matches!(
            call["method"].as_str(),
            Some("ping" | "session.snapshot" | "workspace.get")
        )));
    }
}
impl Drop for Provider {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let result = self.worker.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}
struct Fixture {
    root: PathBuf,
    database: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forge604-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let fixture = Self {
            database: root.join("store.sqlite"),
            root,
        };
        let store = fixture.store();
        for workspace in ["one", "two"] {
            let path = fixture.root.join(workspace);
            std::fs::create_dir_all(&path).unwrap();
            store.create_workspace(workspace, workspace, &path).unwrap();
            store
                .register(&forge_app::Project {
                    reference: format!("{workspace}/api"),
                    workspace_id: workspace.into(),
                    slug: "api".into(),
                    project_root: path.canonicalize().unwrap(),
                    origin_url: "https://example.test/api.git".into(),
                })
                .unwrap();
        }
        fixture
    }
    fn store(&self) -> SqliteProjectRepository {
        SqliteProjectRepository::open(&self.database).unwrap()
    }
    fn app(&self) -> Application {
        application_from_config_with_cli(
            &DaemonConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                store_path: self.database.clone(),
                managed: None,
            },
            Some(std::env::current_exe().unwrap()),
        )
        .unwrap()
    }
    fn configure(&self, app: &Application, provider: &Provider, workspace: &str) {
        assert!(provider.path.starts_with(&self.root));
        app.workspace_dispatch_config_set(
            workspace,
            "herdr-socket",
            provider.path.to_str().unwrap(),
            None,
        )
        .unwrap();
        for key in ["task-profile", "grilling-profile"] {
            app.workspace_dispatch_config_set(workspace, key, "1", None)
                .unwrap();
        }
        app.workspace_dispatch_config_set(
            workspace,
            "workspace",
            &format!("herdr-{workspace}"),
            None,
        )
        .unwrap();
    }
    fn grilling(&self, workspace: &str) -> forge_app::Record {
        self.store()
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Grilling,
                body: "decision".into(),
                project_slugs: vec![format!("{workspace}/api")],
                generated_by_ids: vec![],
            })
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
mod recovery {
    use super::*;
    include!("workspace_recovery_tests.rs");
}

fn has(ready: &forge_app::WorkspaceDispatchReadiness, code: &str) -> bool {
    ready.blockers.iter().any(|blocker| blocker.code == code)
}

#[test]
#[allow(clippy::too_many_lines)]
fn production_readiness_distinguishes_configuration_outage_and_exact_identity() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let initial = app.workspace_dispatch_readiness("one").unwrap();
    assert!(!initial.configured && !initial.dispatchable);
    for code in [
        "task-profile-missing",
        "grilling-profile-missing",
        "binding-missing",
        "provider-unreachable",
    ] {
        assert!(has(&initial, code), "{initial:?}");
    }
    let grilling = fixture.grilling("one");
    assert!(app.dispatch_record(grilling.id).is_err());
    assert!(
        fixture
            .store()
            .list_agent_assignments(grilling.id)
            .unwrap()
            .is_empty()
    );
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    fixture.configure(&app, &provider, "one");
    fixture.configure(&app, &provider, "two");
    assert!(
        app.workspace_dispatch_readiness("one")
            .unwrap()
            .dispatchable
    );
    assert!(
        app.workspace_dispatch_readiness("two")
            .unwrap()
            .dispatchable
    );
    assert_eq!(
        app.workspace_dispatch_config_set("two", "workspace", "herdr-one", None)
            .unwrap_err()
            .code,
        "dispatch-workspace-conflict"
    );
    let before = fixture
        .store()
        .workspace_dispatch_configuration("one")
        .unwrap();
    for (key, value) in [
        ("task-profile", "999"),
        ("grilling-profile", "0"),
        ("task-profile", "invalid"),
        ("unknown", "value"),
    ] {
        assert!(
            app.workspace_dispatch_config_set("one", key, value, None)
                .is_err()
        );
    }
    assert_eq!(
        fixture
            .store()
            .workspace_dispatch_configuration("one")
            .unwrap(),
        before
    );
    app.workspace_dispatch_config_unset("one", "grilling-profile", None)
        .unwrap();
    let incomplete = app.workspace_dispatch_readiness("one").unwrap();
    assert!(has(&incomplete, "grilling-profile-missing"));
    assert!(!incomplete.configured && !incomplete.dispatchable);
    assert!(app.dispatch_record(grilling.id).is_err());
    assert!(
        fixture
            .store()
            .list_agent_assignments(grilling.id)
            .unwrap()
            .is_empty()
    );
    app.workspace_dispatch_config_set("one", "grilling-profile", "1", None)
        .unwrap();
    provider.state.lock().unwrap().missing = true;
    let missing = app.workspace_dispatch_readiness("one").unwrap();
    assert!(missing.configured && !missing.dispatchable);
    assert!(has(&missing, "binding-missing-provider-identity"));
    assert!(app.dispatch_record(grilling.id).is_err());
    provider.reset();
    provider.state.lock().unwrap().mismatch = true;
    assert!(has(
        &app.workspace_dispatch_readiness("one").unwrap(),
        "binding-unverifiable"
    ));
    provider.reset();
    provider.no_provisioning();
    drop(provider);
    let unavailable = app.workspace_dispatch_readiness("one").unwrap();
    assert!(unavailable.configured && !unavailable.dispatchable);
    assert!(has(&unavailable, "provider-unreachable"));
    assert!(
        app.workspace_dispatch_config_get("one", None, Some("one/api"))
            .is_ok()
    );
    app.workspace_dispatch_config_set("one", "task-profile", "1", None)
        .unwrap();
    app.workspace_dispatch_config_set("one", "project-limit", "3", Some("one/api"))
        .unwrap();
    assert_eq!(
        fixture
            .store()
            .effective_project_task_concurrency_limit("two/api")
            .unwrap(),
        1
    );
    assert!(
        app.workspace_dispatch_config_set("one", "project-limit", "4", Some("two/api"))
            .is_err()
    );
    let _planned = fixture.grilling("two");
    let provider = Provider::new(fixture.root.join("replacement.sock"));
    app.workspace_dispatch_config_set("one", "herdr-socket", provider.path.to_str().unwrap(), None)
        .unwrap();
    for workspace in ["one", "two"] {
        assert!(has(
            &app.workspace_dispatch_readiness(workspace).unwrap(),
            "binding-unverified"
        ));
        app.workspace_dispatch_config_set(
            workspace,
            "workspace",
            &format!("herdr-{workspace}"),
            None,
        )
        .unwrap();
        assert!(
            app.workspace_dispatch_readiness(workspace)
                .unwrap()
                .dispatchable
        );
    }
    drop(app);
    assert!(
        fixture
            .app()
            .workspace_dispatch_readiness("one")
            .unwrap()
            .dispatchable
    );
    provider.no_provisioning();
}

#[test]
fn registered_profile_blocker_is_durable_redacted_and_clears_on_input_repair() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    fixture.configure(&app, &provider, "one");
    let root = fixture.root.join("one");
    let skill = root.join(".agents/skills/entry");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: entry\ndescription: entry\n---\nbody",
    )
    .unwrap();
    std::fs::write(skill.join("forge-agent.md"), "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\ntools: []\nskills: [{name: entry}]\n---\nPRIVATE_BEHAVIOR\n").unwrap();
    let profile = app
        .register_agent_profile(
            "one",
            &fixture.root.join("two"),
            std::path::Path::new(".agents/skills/entry/forge-agent.md"),
            RecordType::Task,
        )
        .unwrap();
    std::fs::write(skill.join("SKILL.md"), "drifted direct input").unwrap();
    app.workspace_dispatch_config_set("one", "task-profile", &profile.revision.to_string(), None)
        .unwrap();
    let ready = app.workspace_dispatch_readiness("one").unwrap();
    assert!(has(&ready, "task-profile-invalid"));
    assert!(
        ready
            .blockers
            .iter()
            .any(|b| b.message.contains(&profile.revision.to_string()))
    );
    assert!(!format!("{ready:?}").contains("PRIVATE_BEHAVIOR"));
    let task = fixture
        .store()
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "task".into(),
            project_slugs: vec!["one/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let store = fixture.store();
    let sentinel = fixture.root.join("setup-hook-ran");
    store
        .set_project_setup_hook("one/api", Some(&format!("touch {}", sentinel.display())))
        .unwrap();
    store
        .set_task_base_commit(task.id, "0123456789012345678901234567890123456789")
        .unwrap();
    assert!(app.dispatch_record(task.id).is_err());
    assert!(store.list_agent_assignments(task.id).unwrap().is_empty());
    assert!(
        forge_app::ManagedWorktreeRepository::worktree_claim(&store, task.id)
            .unwrap()
            .is_none()
    );
    assert!(!sentinel.exists());
    assert!(!fixture.root.join("assignment-capabilities").exists());
    provider.no_provisioning();
    drop(app);
    let persisted = fixture
        .store()
        .workspace_dispatch_configuration("one")
        .unwrap();
    assert!(
        persisted
            .profile_preflight_blockers
            .iter()
            .any(|b| b.record_type == RecordType::Task && b.profile_revision == profile.revision)
    );
    let store = fixture.store();
    store
        .hold_dispatch(task.id, "operator review required")
        .unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: entry\ndescription: entry\n---\nbody",
    )
    .unwrap();
    let app = fixture.app();
    assert!(
        app.workspace_dispatch_readiness("one")
            .unwrap()
            .dispatchable
    );
    assert!(store.dispatch_hold(task.id).unwrap().is_some());
    assert!(
        fixture
            .store()
            .workspace_dispatch_configuration("one")
            .unwrap()
            .profile_preflight_blockers
            .is_empty()
    );
}

#[test]
fn provider_identity_loss_between_readiness_and_provisioning_has_no_launch_side_effects() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    fixture.configure(&app, &provider, "one");
    let record = fixture.grilling("one");
    assert!(
        app.workspace_dispatch_readiness("one")
            .unwrap()
            .dispatchable
    );
    provider.reset();
    // Three successful checks cover readiness and the bound-revision gate. Lose the
    // identity at the provider adapter boundary, before capability delivery or tab.create.
    provider.state.lock().unwrap().lose_on_get = Some(4);
    let (assignment, _) = app.dispatch_record(record.id).unwrap();
    assert_eq!(
        assignment.state,
        forge_app::AgentAssignmentState::Provisioning
    );
    assert_eq!(assignment.retry_count, 0);
    assert!(
        fixture
            .store()
            .list_agent_assignment_resources(&assignment.id)
            .unwrap()
            .is_empty()
    );
    provider.no_provisioning();
    drop(app);
    fixture
        .app()
        .reconcile_agent_dispatch("restarted", 1000)
        .unwrap();
    provider.no_provisioning();
    assert_eq!(
        fixture
            .store()
            .agent_assignment(&assignment.id)
            .unwrap()
            .retry_count,
        0
    );
}

#[test]
fn automatic_scheduling_has_no_global_readiness_or_latest_profile_bypass() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    fixture.configure(&app, &provider, "one");
    let one_profile = fixture
        .store()
        .configure_workspace_acknowledgement_timeout("one", Some(42_000))
        .unwrap();
    app.workspace_dispatch_config_set("two", "task-profile", "1", None)
        .unwrap();
    let two_profile = fixture
        .store()
        .configure_workspace_acknowledgement_timeout("two", Some(99_000))
        .unwrap();
    assert!(two_profile.revision > one_profile.revision);
    let store = fixture.store();
    let mut records = Vec::new();
    for workspace in ["one", "two"] {
        let root = fixture.root.join(workspace);
        git(&root, &["init"]).unwrap();
        git(
            &root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.test",
                "commit",
                "--allow-empty",
                "-m",
                "base",
            ],
        )
        .unwrap();
        let base = git(&root, &["rev-parse", "HEAD"]).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "implement".into(),
                project_slugs: vec![format!("{workspace}/api")],
                generated_by_ids: vec![],
            })
            .unwrap();
        store.set_task_base_commit(task.id, base.trim()).unwrap();
        records.push(task);
    }
    app.reconcile_agent_dispatch("scheduler", 1_000).unwrap();
    let assignments = store.list_agent_assignments(records[0].id).unwrap();
    assert!(assignments.is_empty());
    assert!(
        forge_app::ManagedWorktreeRepository::worktree_claim(&store, records[0].id)
            .unwrap()
            .is_none()
    );
    assert_eq!(store.show_record(records[0].id).unwrap(), records[0]);
    assert!(
        store
            .list_agent_assignments(records[1].id)
            .unwrap()
            .is_empty()
    );
    assert!(
        forge_app::ManagedWorktreeRepository::worktree_claim(&store, records[1].id)
            .unwrap()
            .is_none()
    );
    provider.no_provisioning();
}

#[test]
fn production_grilling_launch_uses_its_selected_revision_and_retained_container() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    fixture.configure(&app, &provider, "one");
    fixture.configure(&app, &provider, "two");
    let record = fixture.grilling("two");
    let (assignment, duplicate) = app.dispatch_record(record.id).unwrap();
    assert!(!duplicate);
    assert_eq!(
        assignment.state,
        forge_app::AgentAssignmentState::Provisioning
    );
    assert_eq!(
        fixture
            .store()
            .assignment_dispatch_workspace(&assignment.id)
            .unwrap()
            .unwrap()
            .id,
        "herdr-two"
    );
    let calls = provider.state.lock().unwrap();
    let tab = calls
        .calls
        .iter()
        .find(|call| call["method"] == "tab.create")
        .unwrap();
    assert_eq!(tab["params"]["workspace_id"], "herdr-two");
    assert_eq!(
        tab["params"]["cwd"],
        fixture.root.join("two").to_str().unwrap()
    );
    let launch = calls
        .calls
        .iter()
        .find(|call| call["method"] == "agent.start")
        .unwrap();
    assert!(!launch.to_string().contains("assignment bootstrap"));
    assert!(!launch.to_string().contains("decision"));
    let prompt = calls
        .calls
        .iter()
        .find(|call| call["method"] == "agent.prompt")
        .unwrap();
    assert!(
        prompt["params"]["text"]
            .as_str()
            .unwrap()
            .contains("assignment bootstrap")
    );
    assert_eq!(
        calls
            .calls
            .iter()
            .filter(|call| call["method"] == "agent.start")
            .count(),
        1
    );
    assert!(calls.calls.iter().all(|call| !matches!(
        call["method"].as_str(),
        Some("workspace.create" | "workspace.delete")
    )));
    drop(calls);
    app.workspace_dispatch_config_set("two", "task-profile", "1", None)
        .unwrap();
    assert!(
        app.workspace_dispatch_config_unset("two", "workspace", None)
            .is_err()
    );
    assert!(
        app.workspace_dispatch_config_unset("one", "herdr-socket", None)
            .is_err()
    );
    app.workspace_dispatch_config_unset("one", "workspace", None)
        .unwrap();
    assert!(app.dispatch_record(record.id).unwrap().1);
}

#[test]
#[allow(clippy::too_many_lines)]
fn workspace_launch_context_and_recovery_stay_with_the_original_owner_after_restart() {
    let fixture = Fixture::new();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    let app = fixture.app();
    for workspace in ["one", "two"] {
        fixture.configure(&app, &provider, workspace);
    }
    let nested = fixture.root.join("two/nested");
    std::fs::create_dir_all(&nested).unwrap();
    let store = fixture.store();
    store
        .register(&forge_app::Project {
            reference: "two/web".into(),
            workspace_id: "two".into(),
            slug: "web".into(),
            project_root: nested.clone(),
            origin_url: "https://example.test/web.git".into(),
        })
        .unwrap();
    for reference in ["one/api", "two/api", "two/web"] {
        store
            .set_project_task_concurrency_limit(reference, Some(1))
            .unwrap();
    }
    let one = fixture.grilling("one");
    let two = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Grilling,
            body: "multi-project decision".into(),
            project_slugs: vec!["two/api".into(), "two/web".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let first = app.dispatch_record(one.id).unwrap().0;
    let web = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Grilling,
            body: "web decision".into(),
            project_slugs: vec!["two/web".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let occupied = store
        .claim_record_agent_assignment(web.id, "web-occupied")
        .unwrap()
        .0;
    assert_eq!(
        app.dispatch_record(two.id).unwrap_err().code,
        "project-capacity"
    );
    assert!(store.list_agent_assignments(two.id).unwrap().is_empty());
    assert_eq!(
        provider
            .state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|call| call["method"] == "agent.start")
            .count(),
        1
    );
    store
        .close_agent_assignment(
            &occupied.id,
            forge_app::AgentAssignmentClosureReason::Retired,
        )
        .unwrap();
    let second = app.dispatch_record(two.id).unwrap().0;
    {
        let state = provider.state.lock().unwrap();
        let tabs: Vec<_> = state
            .calls
            .iter()
            .filter(|call| call["method"] == "tab.create")
            .collect();
        assert_eq!(tabs.len(), 2);
        for (tab, workspace) in tabs.iter().zip(["one", "two"]) {
            assert_eq!(
                tab["params"]["cwd"],
                fixture.root.join(workspace).to_str().unwrap()
            );
            assert_eq!(tab["params"]["workspace_id"], format!("herdr-{workspace}"));
            assert_eq!(tab["params"]["focus"], false);
        }
        let delivery = state
            .calls
            .iter()
            .filter(|call| call["method"] == "agent.prompt")
            .nth(1)
            .unwrap();
        let prompt = delivery["params"]["text"].as_str().unwrap();
        assert!(prompt.contains("assignment bootstrap"));
        assert!(!prompt.contains("multi-project decision"));
    }
    let secret_root = fixture.root.join("assignment-capabilities");
    for assignment in [&first, &second] {
        let secret = std::fs::read_to_string(secret_root.join(&assignment.id)).unwrap();
        let bootstrap = app
            .assignment(&secret_root, &assignment.id, secret.trim())
            .unwrap()
            .bootstrap()
            .unwrap();
        if assignment.id == second.id {
            assert_eq!(bootstrap.context.projects.len(), 2);
            assert!(
                bootstrap
                    .context
                    .projects
                    .iter()
                    .any(|p| p.project_root == nested)
            );
        }
    }
    assert!(
        store
            .in_record_workspace("one")
            .unwrap()
            .agent_workspace(two.id)
            .is_err()
    );
    let retained = store.list_agent_assignment_resources(&second.id).unwrap();
    assert_eq!(
        app.dispatch_record(web.id).unwrap_err().code,
        "project-capacity"
    );
    assert_eq!(store.list_agent_assignments(web.id).unwrap().len(), 1);
    drop(app);
    let restarted = fixture.app();
    restarted
        .reconcile_agent_dispatch("restart", 1_000)
        .unwrap();
    for (original, workspace) in [(&first, "one"), (&second, "two")] {
        let recovered = store.agent_assignment(&original.id).unwrap();
        assert_eq!(recovered.profile, original.profile);
        assert_eq!(recovered.project_slug, original.project_slug);
        assert_eq!(recovered.state, forge_app::AgentAssignmentState::Working);
        assert_eq!(
            store
                .assignment_dispatch_workspace(&original.id)
                .unwrap()
                .unwrap()
                .id,
            format!("herdr-{workspace}")
        );
        assert!(store.remove_workspace(workspace).is_err());
    }
    assert_eq!(
        store.list_agent_assignment_resources(&second.id).unwrap(),
        retained
    );
    provider.state.lock().unwrap().missing = true;
    restarted
        .reconcile_agent_dispatch("provider-outage", 2_000)
        .unwrap();
    for original in [&first, &second] {
        let after_outage = store.agent_assignment(&original.id).unwrap();
        assert!(after_outage.closed_at_millis.is_none());
        assert_eq!(after_outage.profile, original.profile);
    }
    assert_eq!(
        store.list_agent_assignment_resources(&second.id).unwrap(),
        retained
    );
    provider.state.lock().unwrap().missing = false;
    store
        .close_agent_assignment(&second.id, forge_app::AgentAssignmentClosureReason::Retired)
        .unwrap();
    assert_eq!(
        restarted.dispatch_record(web.id).unwrap_err().code,
        "project-capacity"
    );
    assert!(store.remove_workspace("two").is_err());
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "web implementation".into(),
            project_slugs: vec!["two/web".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    assert!(
        store
            .claim_next_agent_assignment("two/web", "blocked-scheduler")
            .unwrap()
            .is_none()
    );
    assert!(store.list_agent_assignments(task.id).unwrap().is_empty());
    let state = provider.state.lock().unwrap();
    assert_eq!(
        state
            .calls
            .iter()
            .filter(|call| call["method"] == "agent.start")
            .count(),
        2
    );
    assert_eq!(
        state
            .calls
            .iter()
            .filter(|call| call["method"] == "agent.get")
            .count(),
        4
    );
    assert!(state.calls.iter().all(|call| matches!(
        call["method"].as_str(),
        Some(
            "ping"
                | "session.snapshot"
                | "workspace.get"
                | "tab.create"
                | "agent.start"
                | "agent.get"
                | "agent.prompt"
        )
    )));
}
