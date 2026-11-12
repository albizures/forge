#[path = "support/manual.rs"]
mod manual;
mod support;
use forge_app::{
    AgentDispatchRepository, ManagedWorktreeRepository, NewAgentAssignment, NewTypedRecord,
    RecordRelationshipKind, RecordRepository, RecordType, WorkspaceDispatchRepository,
};
use forge_server::{DaemonConfig, serve};
use forge_store::SqliteProjectRepository;
use std::{path::Path, process::Command};

fn run(root: &str, secret: &Path, assignment: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(args)
        .env("FORGE_ASSIGNMENT_ID", assignment)
        .env("FORGE_ASSIGNMENT_DAEMON_URL", root)
        .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", secret)
        .output()
        .unwrap()
}

async fn run_cli(
    root: &str,
    secret: &Path,
    assignment: &str,
    args: &[&str],
) -> std::process::Output {
    let root = root.to_owned();
    let secret = secret.to_owned();
    let assignment = assignment.to_owned();
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    tokio::task::spawn_blocking(move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        run(&root, &secret, &assignment, &args)
    })
    .await
    .unwrap()
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One daemon lifecycle exercises both Assignment types.
async fn production_daemon_bootstraps_grilling_through_delivered_cli() {
    let root = std::env::temp_dir().join(format!("forge-cli-production-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let db = root.join("db.sqlite");
    let store = SqliteProjectRepository::open(&db).unwrap();
    support::register_project(&store, "alpha");
    let catalog = root.join("profile-catalog");
    let skill = catalog.join(".agents/skills/example");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: example\ndescription: example\n---\nSkill body",
    )
    .unwrap();
    std::fs::write(skill.join("forge-agent.md"), "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\ntools: []\nskills: [{name: example}]\n---\nFROZEN-BEHAVIOR").unwrap();
    let policy = forge_app::agent_profiles::ResourceCatalog::discover(&catalog, &catalog)
        .unwrap()
        .resolve(&catalog, ".agents/skills/example/forge-agent.md")
        .unwrap();
    let profile = store
        .register_agent_profile("test", RecordType::Task, &policy)
        .unwrap();
    let record = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Grilling,
            body: "Grilling title\nDiscuss".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    store
        .create_agent_assignment(&NewAgentAssignment {
            id: "production-assignment".into(),
            record_id: record.id,
            project_slug: "alpha".into(),
            profile_revision: 1,
        })
        .unwrap();
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Task title\nImplement".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    store.reserve_worktree_claim(task.id).unwrap();
    store.activate_worktree_claim(task.id).unwrap();
    store
        .create_agent_assignment(&NewAgentAssignment {
            id: "production-task".into(),
            record_id: task.id,
            project_slug: "alpha".into(),
            profile_revision: profile.revision,
        })
        .unwrap();
    let missing = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Missing worktree".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    store
        .create_agent_assignment(&NewAgentAssignment {
            id: "missing-worktree".into(),
            record_id: missing.id,
            project_slug: "alpha".into(),
            profile_revision: 1,
        })
        .unwrap();
    let prerequisite = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Prerequisite".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let blocked = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Blocked task".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    store
        .relate_records(
            blocked.id,
            prerequisite.id,
            RecordRelationshipKind::DependsOn,
        )
        .unwrap();
    store.reserve_worktree_claim(blocked.id).unwrap();
    store.activate_worktree_claim(blocked.id).unwrap();
    store
        .create_agent_assignment(&NewAgentAssignment {
            id: "blocked-task".into(),
            record_id: blocked.id,
            project_slug: "alpha".into(),
            profile_revision: 1,
        })
        .unwrap();
    drop(store);
    std::fs::remove_dir_all(&catalog).unwrap();
    let secret_dir = root.join("assignment-capabilities");
    std::fs::create_dir_all(&secret_dir).unwrap();
    let secret = secret_dir.join("production-assignment");
    std::fs::write(&secret, "production-secret\n").unwrap();
    let task_secret = secret_dir.join("production-task");
    std::fs::write(&task_secret, "task-secret\n").unwrap();
    let missing_secret = secret_dir.join("missing-worktree");
    std::fs::write(&missing_secret, "missing-secret\n").unwrap();
    let blocked_secret = secret_dir.join("blocked-task");
    std::fs::write(&blocked_secret, "blocked-secret\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for file in [&secret, &task_secret, &missing_secret, &blocked_secret] {
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let daemon = tokio::spawn(serve(DaemonConfig {
        bind_addr: addr,
        store_path: db.clone(),
        managed: None,
    }));
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let endpoint = format!("http://{addr}");
    let secret_clone = secret.clone();
    let endpoint_clone = endpoint.clone();
    let output = tokio::task::spawn_blocking(move || {
        run(
            &endpoint_clone,
            &secret_clone,
            "production-assignment",
            &["assignment", "bootstrap", "--json"],
        )
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["context"]["record"]["id"], record.id);
    assert_eq!(json["context"]["record"]["record_type"], "grilling");
    assert_eq!(json["context"]["record"]["status"], "in_progress");
    assert!(json["behavior"].is_string());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("production-secret"));
    let blocked_output = run_cli(
        &endpoint,
        &blocked_secret,
        "blocked-task",
        &["assignment", "bootstrap", "--json"],
    )
    .await;
    assert!(!blocked_output.status.success());
    assert!(blocked_output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&blocked_output.stderr).contains("blocked-secret"));
    let repair_store = SqliteProjectRepository::open(&db).unwrap();
    repair_store
        .set_record_execution_mode(
            prerequisite.id,
            forge_app::ExecutionMode::Dispatched,
            forge_app::ExecutionMode::Manual,
            "fixture",
            "Manual prerequisite",
        )
        .unwrap();
    repair_store.start_record(prerequisite.id).unwrap();
    for check in repair_store.list_record_checks(prerequisite.id).unwrap() {
        manual::pass(&repair_store, prerequisite.id, check.id);
    }
    repair_store
        .done_record(prerequisite.id, Some("resolved"))
        .unwrap();
    drop(repair_store);
    let repaired = run_cli(
        &endpoint,
        &blocked_secret,
        "blocked-task",
        &["assignment", "bootstrap", "--json"],
    )
    .await;
    assert!(
        repaired.status.success(),
        "{}",
        String::from_utf8_lossy(&repaired.stderr)
    );
    let failed = run_cli(
        &endpoint,
        &missing_secret,
        "missing-worktree",
        &["assignment", "bootstrap", "--json"],
    )
    .await;
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&failed.stderr).contains("missing-secret"));
    let task_result = run_cli(
        &endpoint,
        &task_secret,
        "production-task",
        &["assignment", "bootstrap", "--json"],
    )
    .await;
    assert!(
        task_result.status.success(),
        "{}",
        String::from_utf8_lossy(&task_result.stderr)
    );
    let task_context: serde_json::Value = serde_json::from_slice(&task_result.stdout).unwrap();
    assert_eq!(task_context["behavior"], "FROZEN-BEHAVIOR");
    assert!(
        !task_context["context"]
            .to_string()
            .contains("FROZEN-BEHAVIOR")
    );
    assert!(!task_context.to_string().contains("task-secret"));
    assert_eq!(task_context["context"]["record"]["record_type"], "task");
    assert_eq!(task_context["context"]["record"]["id"], task.id);
    let retry = run_cli(
        &endpoint,
        &task_secret,
        "production-task",
        &["assignment", "bootstrap", "--json"],
    )
    .await;
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    let spoof = run_cli(
        &endpoint,
        &task_secret,
        "production-assignment",
        &["assignment", "bootstrap", "--json"],
    )
    .await;
    assert!(!spoof.status.success());
    assert!(spoof.stdout.is_empty());
    let diagnostics = String::from_utf8_lossy(&spoof.stderr);
    assert!(!diagnostics.contains("task-secret"));
    assert!(!diagnostics.contains(task_secret.to_str().unwrap()));
    let task_id = task.id.to_string();
    let task_comment = run_cli(
        &endpoint,
        &task_secret,
        "production-task",
        &[
            "assignment",
            "comment",
            "--message",
            "Task progress",
            "--json",
        ],
    )
    .await;
    assert!(
        task_comment.status.success(),
        "{}",
        String::from_utf8_lossy(&task_comment.stderr)
    );
    let task_page = run_cli(
        &endpoint,
        &task_secret,
        "production-task",
        &["assignment", "comments", &task_id, "--json"],
    )
    .await;
    assert!(
        task_page.status.success(),
        "{}",
        String::from_utf8_lossy(&task_page.stderr)
    );
    assert!(String::from_utf8_lossy(&task_page.stdout).contains("Task progress"));
    let id = record.id.to_string();
    for args in [
        vec!["assignment", "context", "--json"],
        vec!["assignment", "record", &id, "--json"],
        vec!["assignment", "comment", "--message", "Progress", "--json"],
        vec!["assignment", "comments", &id, "--limit", "1", "--json"],
        vec!["assignment", "history", &id, "--json"],
        vec!["assignment", "start", "--json"],
    ] {
        let result = run_cli(&endpoint, &secret, "production-assignment", &args).await;
        assert!(
            result.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(result["schemaVersion"], 1);
        assert!(!result.to_string().contains("production-secret"));
    }
    let result = run_cli(
        &endpoint,
        &secret,
        "production-assignment",
        &["assignment", "done", "--resolution", "agreed", "--json"],
    )
    .await;
    assert!(!result.status.success());
    let result = run_cli(
        &endpoint,
        &secret,
        "production-assignment",
        &[
            "assignment",
            "done",
            "--resolution",
            "agreed",
            "--confirmed",
            "--confirmation",
            "Human confirmed in conversation",
            "--agreed-outcome",
            "Proceed",
            "--json",
        ],
    )
    .await;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let done: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(done["record"]["status"], "done");
    daemon.abort();
    let _ = daemon.await;
    std::fs::remove_dir_all(root).unwrap();
}
