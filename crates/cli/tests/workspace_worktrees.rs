use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchRepository,
    AgentProfile, Application, ManagedWorktreeRepository, RecordRepository, WorkspaceRepository,
};
use forge_server::{Daemon, GitInspector, GitWorktreeAdapter, router};
use forge_store::SqliteProjectRepository;
use jsonrpsee::core::client::ClientT;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

struct NoProvider;
impl AgentDispatchAdapterFactory for NoProvider {
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        panic!("worktree management must not contact a provider")
    }
}
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forge-workspace-worktrees-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn repository(root: &Path) {
    std::fs::create_dir_all(root).unwrap();
    git(root, &["init", "--initial-branch=main"]);
    git(
        root,
        &["remote", "add", "origin", "https://example.test/repo.git"],
    );
    git(
        root,
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
    );
}
fn success(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(&String::from_utf8(output.stdout).unwrap()).unwrap()
}
fn rejected(output: Output, diagnostic: &str) {
    assert!(
        !output.status.success(),
        "unexpected success: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains(diagnostic), "{error}");
}
fn outside_path(root: &Path) -> String {
    root.join("external-checkouts")
        .to_string_lossy()
        .into_owned()
}
fn application(database: &Path) -> Application {
    Application::with_managed_worktrees_and_agent_dispatch(
        Box::new(SqliteProjectRepository::open(database).unwrap()),
        Box::new(SqliteProjectRepository::open(database).unwrap()),
        Box::new(GitWorktreeAdapter),
        Box::new(SqliteProjectRepository::open(database).unwrap()),
        Box::new(NoProvider),
        Box::new(GitInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(database).unwrap()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn fixed_layout_selection_and_lifecycle_survive_reopen_without_dispatch_configuration() {
    let fixture = Fixture::new();
    let one = fixture.0.join("one");
    let nested = one.join("tools");
    let two = fixture.0.join("two");
    repository(&one);
    repository(&nested);
    repository(&two);
    let database = fixture.0.join("store.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("one", "One", &one).unwrap();
    store.create_workspace("two", "Two", &two).unwrap();
    let app = application(&database);
    app.register_workspace_project("one", &one, "api").unwrap();
    app.register_workspace_project("one", &nested, "tools")
        .unwrap();
    app.register_workspace_project("two", &two, "api").unwrap();
    let task = app
        .open_task("root Task", Some("one/api".into()), None)
        .unwrap();
    let nested_task = app
        .open_task("nested Task", Some("one/tools".into()), None)
        .unwrap();
    let other = app
        .open_task("same slug", Some("two/api".into()), None)
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/rpc", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router(Daemon::new(app)))
            .await
            .unwrap();
    });
    let run = |args: &[&str], cwd: &Path| {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(args)
            .args(["--server-url", &endpoint, "--json"])
            .current_dir(cwd)
            .output()
            .unwrap()
    };
    let create = |id: u64, branch: &str, workspace: &str| {
        success(run(
            &[
                "worktree",
                "create",
                &id.to_string(),
                "--branch",
                branch,
                "--base",
                "HEAD",
                "--workspace",
                workspace,
            ],
            &fixture.0,
        ))
    };
    rejected(
        run(
            &[
                "worktree",
                "create",
                &task.id.to_string(),
                "--branch",
                "root",
                "--base",
                "HEAD",
            ],
            &fixture.0,
        ),
        "Workspace",
    );
    assert!(store.worktree_claim(task.id).unwrap().is_none());
    rejected(
        run(
            &[
                "worktree",
                "create",
                &task.id.to_string(),
                "--branch",
                "root",
                "--base",
                "HEAD",
                "--workspace",
                "two",
            ],
            &one,
        ),
        "Workspace",
    );
    assert!(store.worktree_claim(task.id).unwrap().is_none());
    success(run(
        &[
            "project",
            "setup-hook",
            "set",
            "--project",
            "api",
            "--workspace",
            "one",
            "test -f .git && printf hook > hook-ran",
        ],
        &fixture.0,
    ));
    // RPC clients cannot bypass selection or sneak placement overrides past the CLI.
    let client = jsonrpsee::http_client::HttpClientBuilder::default()
        .build(&endpoint)
        .unwrap();
    for extra in [
        serde_json::json!({"workspace": "one", "path": outside_path(&fixture.0)}),
        serde_json::json!({"workspace": "one", "root": outside_path(&fixture.0)}),
        serde_json::json!({"cwd": "."}),
        serde_json::json!({}),
    ] {
        let mut request =
            serde_json::json!({"record_id": task.id, "branch": "rejected", "base": "HEAD"});
        request
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(
            client
                .request::<serde_json::Value, _>("worktree.create", jsonrpsee::rpc_params![request])
                .await
                .is_err()
        );
        assert!(store.worktree_claim(task.id).unwrap().is_none());
        assert!(git(&one, &["branch", "--list", "rejected"]).is_empty());
    }
    assert!(
        client
            .request::<serde_json::Value, _>(
                "worktree.rootSet",
                jsonrpsee::rpc_params![serde_json::json!({"root": outside_path(&fixture.0)})]
            )
            .await
            .is_err()
    );
    let root_tree = create(task.id, "root", "one");
    let nested_tree = create(nested_task.id, "nested", "one");
    let other_tree = create(other.id, "other", "two");
    let root_path = one.join(format!(".forge/worktrees/api/{}", task.id));
    let nested_path = one.join(format!(".forge/worktrees/tools/{}", nested_task.id));
    let other_path = two.join(format!(".forge/worktrees/api/{}", other.id));
    assert_eq!(root_tree["path"], root_path.to_str().unwrap());
    assert_eq!(nested_tree["path"], nested_path.to_str().unwrap());
    assert_eq!(other_tree["path"], other_path.to_str().unwrap());
    assert!(!nested.join(".forge").exists());
    assert_eq!(
        std::fs::read_to_string(root_path.join("hook-ran")).unwrap(),
        "hook"
    );
    assert!(!other_path.join("hook-ran").exists());
    std::fs::remove_file(root_path.join("hook-ran")).unwrap();
    assert!(store.project_setup_hook("two/api").unwrap().is_none());
    assert_eq!(
        create(task.id, "unused-branch", "one")["path"],
        root_tree["path"]
    );
    let listing = success(run(
        &["worktree", "list", "--all", "--workspace", "one"],
        &two,
    ));
    assert_eq!(listing["worktrees"].as_array().unwrap().len(), 2);
    assert!(
        listing["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tree| tree["path"] != other_tree["path"])
    );
    rejected(
        run(
            &[
                "worktree",
                "info",
                &task.id.to_string(),
                "--workspace",
                "two",
            ],
            &one,
        ),
        "Workspace",
    );
    for option in ["--path", "--root", "--worktree-root"] {
        rejected(
            run(
                &[
                    "worktree",
                    "create",
                    &task.id.to_string(),
                    "--branch",
                    "override",
                    "--base",
                    "HEAD",
                    option,
                    "/tmp/escape",
                ],
                &one,
            ),
            "unexpected argument",
        );
    }
    rejected(
        run(&["config", "set", "worktreeRoot", "/tmp/escape"], &one),
        "unrecognized subcommand",
    );
    // A clean checkout with an upstream remains ordinarily removable. Dirty checkouts do not.
    git(&nested_path, &["branch", "--set-upstream-to=main"]);
    std::fs::write(nested_path.join("untracked"), "dirty").unwrap();
    rejected(
        run(&["worktree", "remove", &nested_task.id.to_string()], &one),
        "ordinarily removed",
    );
    assert!(nested_path.exists());
    std::fs::remove_file(nested_path.join("untracked")).unwrap();
    success(run(
        &["worktree", "remove", &nested_task.id.to_string()],
        &one,
    ));
    assert!(!nested_path.exists());
    // Completion retains the Claim, and a new application can inspect/remove it.
    store.start_record(task.id).unwrap();
    for check in store.list_task_checks(task.id).unwrap() {
        store
            .assess_task_check(
                task.id,
                check.id,
                &forge_app::checks::AssessmentInput {
                    expected_version: check.version,
                    verdict: forge_app::checks::Verdict::Pass,
                    actor: "test operator".into(),
                    message: "Fixture deliverables verified".into(),
                    revision: None,
                    dirty: None,
                },
            )
            .unwrap();
    }
    store.done_record(task.id, None).unwrap();
    let reopened = application(&database);
    assert_eq!(
        reopened.managed_worktree_info(task.id).unwrap().claim.path,
        root_path
    );
    reopened.remove_managed_worktree(task.id, true).unwrap();
    assert!(!root_path.exists());
    assert!(other_path.exists());
    let invalid_task = reopened
        .open_task("invalid metadata", Some("one/tools".into()), None)
        .unwrap();
    let invalid = reopened.prepare_dispatch_worktree(invalid_task.id).unwrap();
    std::fs::remove_file(invalid.path.join(".git")).unwrap();
    let info = reopened.managed_worktree_info(invalid_task.id).unwrap();
    assert!(
        info.safety
            .facts
            .iter()
            .any(|fact| fact == "git-metadata-invalid")
    );
    assert!(!info.safety.removable);
    assert_eq!(
        reopened
            .prepare_dispatch_worktree(invalid_task.id)
            .unwrap_err()
            .code,
        "managed-worktree-invalid"
    );
    reopened
        .remove_managed_worktree(invalid_task.id, true)
        .unwrap();
    // Adoption uses the same Workspace-rooted path and deterministic branch/base evidence.
    let recovery = reopened
        .open_task("recover", Some("one/tools".into()), None)
        .unwrap();
    let recovery_path = one.join(format!(".forge/worktrees/tools/{}", recovery.id));
    git(
        &nested,
        &[
            "worktree",
            "add",
            "-b",
            &format!("feat/forge-{}", recovery.id),
            recovery_path.to_str().unwrap(),
            "HEAD",
        ],
    );
    success(run(
        &[
            "worktree",
            "recover",
            &recovery.id.to_string(),
            "--confirm-no-active-agent",
        ],
        &one,
    ));
    assert_eq!(
        store.worktree_claim(recovery.id).unwrap().unwrap().path,
        recovery_path
    );
    assert!(store.dispatch_hold(recovery.id).unwrap().is_some());
    reopened.remove_managed_worktree(recovery.id, true).unwrap();
    // Failed hooks retain cleanup evidence. Doctor repairs only the selected owner.
    let failed_one = reopened
        .open_task("failed hook", Some("one/api".into()), None)
        .unwrap();
    let failed_two = reopened
        .open_task("failed hook", Some("two/api".into()), None)
        .unwrap();
    store
        .set_project_setup_hook("one/api", Some("exit 9"))
        .unwrap();
    store
        .set_project_setup_hook("two/api", Some("exit 9"))
        .unwrap();
    assert!(reopened.prepare_dispatch_worktree(failed_one.id).is_err());
    assert!(reopened.prepare_dispatch_worktree(failed_two.id).is_err());
    assert_eq!(
        store.worktree_claim(failed_one.id).unwrap().unwrap().state,
        forge_app::WorktreeClaimState::CleanupRequired
    );
    let doctor = success(run(&["worktree", "doctor", "--workspace", "one"], &two));
    assert_eq!(doctor["worktrees"].as_array().unwrap().len(), 1);
    success(run(
        &["worktree", "doctor", "--repair", "--workspace", "one"],
        &two,
    ));
    assert!(store.worktree_claim(failed_one.id).unwrap().is_none());
    assert_eq!(
        store.worktree_claim(failed_two.id).unwrap().unwrap().state,
        forge_app::WorktreeClaimState::CleanupRequired
    );
    // Scoped repository handles cannot read or mutate foreign Claims either.
    let scoped = store.in_record_workspace("one").unwrap();
    assert!(scoped.worktree_claim(failed_two.id).is_err());
    assert!(scoped.delete_worktree_claim(failed_two.id).is_err());
    assert!(scoped.worktree_claims().unwrap().is_empty());
    reopened.repair_managed_worktree(failed_two.id).unwrap();
    server.abort();
}

#[test]
fn assignment_and_retained_resource_guards_precede_even_forced_git_cleanup() {
    use forge_app::{AgentAssignmentClosureReason, NewAgentAssignment, NewAgentAssignmentResource};
    let fixture = Fixture::new();
    let root = fixture.0.join("workspace");
    repository(&root);
    let database = fixture.0.join("store.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("one", "One", &root).unwrap();
    let app = application(&database);
    app.register_workspace_project("one", &root, "api").unwrap();
    let task = app
        .open_task("retained by agent", Some("one/api".into()), None)
        .unwrap();
    let claim = app.prepare_dispatch_worktree(task.id).unwrap();
    store
        .create_agent_assignment(&NewAgentAssignment {
            id: "live".into(),
            record_id: task.id,
            project_slug: "one/api".into(),
            profile_revision: 1,
        })
        .unwrap();
    let info = app.managed_worktree_info(task.id).unwrap();
    assert!(
        info.safety
            .blockers
            .iter()
            .any(|blocker| blocker == "worktree-in-use")
    );
    assert!(!info.safety.removable);
    for force in [false, true] {
        assert_eq!(
            app.remove_managed_worktree(task.id, force)
                .unwrap_err()
                .code,
            "worktree-in-use"
        );
        assert!(claim.path.join(".git").exists());
    }
    assert_eq!(
        app.repair_managed_worktree(task.id).unwrap_err().code,
        "worktree-in-use"
    );
    store
        .add_agent_assignment_resource(&NewAgentAssignmentResource {
            assignment_id: "live".into(),
            resource_kind: "agent-name".into(),
            resource_id: "original-agent".into(),
        })
        .unwrap();
    store
        .close_agent_assignment("live", AgentAssignmentClosureReason::Retired)
        .unwrap();
    let reopened = application(&database);
    assert_eq!(
        reopened
            .remove_managed_worktree(task.id, true)
            .unwrap_err()
            .code,
        "worktree-in-use"
    );
    assert!(claim.path.join(".git").exists());
    store
        .retire_agent_assignment_resource("live", "agent-name", "original-agent")
        .unwrap();
    reopened.remove_managed_worktree(task.id, true).unwrap();
    assert!(!claim.path.exists());
    assert!(store.worktree_claim(task.id).unwrap().is_none());
}

#[cfg(unix)]
#[test]
fn symlink_redirection_is_rejected_before_git_effects_and_during_recovery_or_removal() {
    let fixture = Fixture::new();
    let root = fixture.0.join("workspace");
    let outside = fixture.0.join("outside");
    repository(&root);
    std::fs::create_dir_all(&outside).unwrap();
    let database = fixture.0.join("store.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("one", "One", &root).unwrap();
    let app = application(&database);
    app.register_workspace_project("one", &root, "api").unwrap();
    // Test each existing ancestor, including dangling links and a redirected final path.
    for (index, component) in [
        ".forge",
        ".forge/worktrees",
        ".forge/worktrees/api",
        ".forge/worktrees/api/RECORD",
    ]
    .iter()
    .enumerate()
    {
        let task = app
            .open_task("unsafe placement", Some("one/api".into()), None)
            .unwrap();
        let relative = component.replace("RECORD", &task.id.to_string());
        let link = root.join(relative);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        let target = if index == 1 {
            outside.join("missing")
        } else {
            outside.clone()
        };
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let branch = format!("unsafe-{index}");
        assert_eq!(
            app.create_managed_worktree(task.id, &branch, "HEAD")
                .unwrap_err()
                .code,
            "managed-worktree-path-invalid"
        );
        assert!(store.worktree_claim(task.id).unwrap().is_none());
        assert!(!git(&root, &["branch", "--list", &branch]).contains(&branch));
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        assert_eq!(
            app.recover_managed_worktree(task.id, true)
                .unwrap_err()
                .code,
            "managed-worktree-path-invalid"
        );
        assert!(store.worktree_claim(task.id).unwrap().is_none());
        std::fs::remove_file(link).unwrap();
    }
    let task = app
        .open_task("retained", Some("one/api".into()), None)
        .unwrap();
    let claim = app.prepare_dispatch_worktree(task.id).unwrap();
    let moved = outside.join("retained");
    std::fs::rename(&claim.path, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &claim.path).unwrap();
    assert_eq!(
        app.remove_managed_worktree(task.id, true).unwrap_err().code,
        "managed-worktree-path-invalid"
    );
    assert_eq!(
        app.repair_managed_worktree(task.id).unwrap_err().code,
        "managed-worktree-path-invalid"
    );
    assert!(moved.join(".git").exists());
    assert!(store.worktree_claim(task.id).unwrap().is_some());
}
