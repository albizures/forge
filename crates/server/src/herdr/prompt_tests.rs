//! Post-bootstrap prompts: re-verified identity, status and turn gate, and unambiguous outcomes.
use super::HerdrAdapter;
use forge_app::{
    AgentAdapterError, AgentAssignmentResource, AgentDispatchAdapter, AgentDispatchContext,
    AgentDispatchRepository, AgentPrompt, AgentPromptOutcome, DispatchWorkspace,
    NewAgentAssignment, NewTypedRecord, RecordRepository, RecordType,
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

fn agent(status: &str, seq: u64) -> Value {
    json!({
        "name":"forge-dispatch-assignment", "agent":"pi", "workspace_id":"workspace",
        "tab_id":"tab", "pane_id":"pane", "terminal_id":"terminal",
        "agent_status":status, "completion_seq":seq
    })
}

fn context(terminal_pinned: bool) -> AgentDispatchContext {
    let repository = forge_store::SqliteProjectRepository::in_memory().unwrap();
    crate::test_projects::register(&repository, "project", None);
    let record = repository
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "task".into(),
            project_slugs: vec!["project".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let assignment = repository
        .create_agent_assignment(&NewAgentAssignment {
            id: "assignment".into(),
            record_id: record.id,
            project_slug: "project".into(),
            profile_revision: 1,
        })
        .unwrap();
    let resource = |kind: &str, id: &str| AgentAssignmentResource {
        assignment_id: "assignment".into(),
        resource_kind: kind.into(),
        resource_id: id.into(),
        created_at_millis: 0,
        retired_at_millis: None,
    };
    let mut resources = vec![
        resource("tab", "tab"),
        resource("pane", "pane"),
        resource("agent-name", "forge-dispatch-assignment"),
    ];
    if terminal_pinned {
        resources.push(resource("terminal", "terminal"));
    }
    AgentDispatchContext {
        assignment,
        project_root: "/project".into(),
        agent_workspace: Some("/worktree".into()),
        dispatch_workspace: Some(DispatchWorkspace {
            id: "workspace".into(),
        }),
        bootstrap_prompt: None,
        capability_delivery: None,
        resources,
    }
}

/// Runs `agent.prompt` once against a scripted Herdr. `prompted` is the `agent.prompt` response;
/// `None` drops the connection after reading the request, as a lost response does.
fn send(
    info: &Value,
    prompted: Option<Value>,
    terminal_pinned: bool,
) -> (Result<AgentPromptOutcome, AgentAdapterError>, Vec<String>) {
    with_herdr(info, prompted, |adapter| {
        adapter.prompt(
            &context(terminal_pinned),
            &AgentPrompt {
                text: "keep going".into(),
                completion_seq: 4,
            },
        )
    })
}

fn with_herdr<T>(
    info: &Value,
    prompted: Option<Value>,
    call: impl FnOnce(&HerdrAdapter) -> T,
) -> (T, Vec<String>) {
    let path = std::env::temp_dir().join(format!(
        "forge-prompt-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let methods = Arc::new(Mutex::new(Vec::<String>::new()));
    let server = {
        let (stop, methods, info) = (stop.clone(), methods.clone(), info.clone());
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                let method = request["method"].as_str().unwrap().to_owned();
                methods.lock().unwrap().push(method.clone());
                let result = match method.as_str() {
                    "workspace.get" => json!({"workspace":{"workspace_id":"workspace"}}),
                    "agent.get" => json!({"type":"agent_info", "agent":info}),
                    "agent.prompt" => match &prompted {
                        Some(response) => response.clone(),
                        None => continue,
                    },
                    other => panic!("unexpected {other}"),
                };
                serde_json::to_writer(&mut stream, &json!({"result":result})).unwrap();
                stream.write_all(b"\n").unwrap();
            }
        })
    };
    let adapter = HerdrAdapter {
        socket_path: path.clone(),
    };
    let result = call(&adapter);
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
    std::fs::remove_file(path).unwrap();
    let methods = methods.lock().unwrap().clone();
    (result, methods)
}

fn prompted() -> Value {
    json!({"type":"agent_prompted", "agent":agent("working", 4)})
}

#[test]
fn an_idle_or_done_agent_at_the_expected_turn_receives_exactly_one_prompt() {
    for status in ["idle", "done"] {
        let (result, methods) = send(&agent(status, 4), Some(prompted()), true);
        assert_eq!(result.unwrap(), AgentPromptOutcome::Delivered);
        assert_eq!(
            methods.iter().filter(|m| *m == "agent.prompt").count(),
            1,
            "{methods:?}"
        );
    }
    // Legacy Assignments launched before terminals were pinned still verify the rest.
    let (result, _) = send(&agent("idle", 4), Some(prompted()), false);
    assert_eq!(result.unwrap(), AgentPromptOutcome::Delivered);
}

#[test]
fn an_agent_that_is_not_idle_at_the_expected_turn_is_not_prompted() {
    for (status, seq) in [("working", 4), ("blocked", 4), ("unknown", 4), ("idle", 5)] {
        let (result, methods) = send(&agent(status, seq), Some(prompted()), true);
        assert_eq!(result.unwrap(), AgentPromptOutcome::NotReady, "{status}");
        assert!(!methods.iter().any(|m| m == "agent.prompt"), "{methods:?}");
    }
}

#[test]
fn a_changed_terminal_pane_or_tab_is_never_prompted() {
    for (field, value) in [
        ("terminal_id", "replacement"),
        ("pane_id", "other"),
        ("tab_id", "other"),
        ("workspace_id", "other"),
    ] {
        let mut info = agent("idle", 4);
        info[field] = value.into();
        let (result, methods) = send(&info, Some(prompted()), true);
        assert!(
            matches!(result, Err(AgentAdapterError::Conflict(_))),
            "{field}: {result:?}"
        );
        assert!(!methods.iter().any(|m| m == "agent.prompt"), "{field}");
    }
}

#[test]
fn a_lost_or_foreign_prompt_response_is_an_error_after_one_send() {
    let (lost, methods) = send(&agent("idle", 4), None, true);
    assert!(lost.is_err());
    assert_eq!(methods.iter().filter(|m| *m == "agent.prompt").count(), 1);

    let foreign = Some(json!({"type":"agent_prompted", "agent":{
        "name":"forge-dispatch-assignment", "agent":"pi", "workspace_id":"workspace",
        "tab_id":"tab", "pane_id":"pane", "terminal_id":"replacement"}}));
    let (result, _) = send(&agent("idle", 4), foreign, true);
    assert!(matches!(result, Err(AgentAdapterError::Conflict(_))));
}

#[test]
fn observing_a_turn_reports_the_herdr_completion_sequence() {
    let (turn, _) = with_herdr(&agent("done", 7), None, |adapter| {
        adapter.observe_turn(&context(true)).unwrap()
    });
    assert_eq!(turn.observation, forge_app::AgentObservation::Done);
    assert_eq!(turn.completion_seq, Some(7));
    let mut info = agent("idle", 0);
    info.as_object_mut().unwrap().remove("completion_seq");
    let (turn, _) = with_herdr(&info, None, |adapter| {
        adapter.observe_turn(&context(true)).unwrap()
    });
    assert_eq!(turn.completion_seq, None);
}
