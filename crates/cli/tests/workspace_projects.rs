use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchRepository,
    AgentProfile, Application, ManagedWorktreeRepository, ProjectRepository, WorkspaceRepository,
};

struct NoProvider;
impl AgentDispatchAdapterFactory for NoProvider {
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        panic!("Project registration and configuration must not contact a provider")
    }
}
use forge_rpc::ProjectApiClient;
use forge_server::{Daemon, GitInspector, GitWorktreeAdapter, router};
use forge_store::SqliteProjectRepository;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forge-cli-workspace-projects-{}-{}",
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
fn git(root: &Path, args: &[&str]) {
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
}
fn repository(root: &Path) {
    std::fs::create_dir_all(root).unwrap();
    git(root, &["init"]);
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
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn rejected(output: Output, diagnostic: &str) {
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !output.status.success(),
        "unexpected success: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(stderr.contains(diagnostic), "{stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn workspace_project_registration_selection_and_configuration_use_the_real_git_inspector() {
    let fixture = Fixture::new();
    let one = fixture.0.join("one");
    let nested = one.join("tools");
    let two = fixture.0.join("two");
    let api = two.join("api");
    repository(&one);
    repository(&nested);
    repository(&api);
    let database = fixture.0.join("forge.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("one", "One", &one).unwrap();
    store.create_workspace("two", "Two", &two).unwrap();
    let app = Application::with_managed_worktrees_and_agent_dispatch(
        Box::new(SqliteProjectRepository::open(&database).unwrap()),
        Box::new(SqliteProjectRepository::open(&database).unwrap()),
        Box::new(GitWorktreeAdapter),
        Box::new(SqliteProjectRepository::open(&database).unwrap()),
        Box::new(NoProvider),
        Box::new(GitInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(&database).unwrap()));
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
            .args(["--server-url", &endpoint])
            .current_dir(cwd)
            .output()
            .unwrap()
    };
    success(run(&["add", "project", ".", "api"], &one));
    success(run(&["add", "project", ".", "tools"], &nested));
    success(run(
        &[
            "add",
            "project",
            api.to_str().unwrap(),
            "api",
            "--workspace",
            "two",
        ],
        &fixture.0,
    ));
    let here = |args: &[&str], cwd: &Path| -> serde_json::Value {
        serde_json::from_str(&success(run(args, cwd))).unwrap()
    };
    let selected = here(&["here", "--json"], &nested);
    assert_eq!(selected["reference"], "one/tools");
    assert_eq!(selected["workspace_id"], "one");
    assert_eq!(selected["slug"], "tools");
    assert_eq!(selected["project_root"], nested.to_str().unwrap());
    assert!(selected.get("registered_folder").is_none());
    assert_eq!(
        here(&["here", "--json", "--project", "one/api"], &nested)["reference"],
        "one/api"
    );
    assert_eq!(
        here(&["here", "--json", "--project", "two/api"], &one)["reference"],
        "two/api"
    );
    assert_eq!(
        here(
            &["here", "--json", "--workspace", "two", "--project", "api"],
            &fixture.0
        )["reference"],
        "two/api"
    );
    rejected(
        run(
            &["here", "--workspace", "one", "--project", "two/api"],
            &nested,
        ),
        "conflict",
    );
    rejected(run(&["here"], &fixture.0), "--workspace");
    rejected(
        run(&["here", "--project", "api"], &fixture.0),
        "--workspace",
    );
    rejected(run(&["here"], &two), "Project");
    rejected(
        run(
            &["add", "project", api.to_str().unwrap(), "unselected"],
            &fixture.0,
        ),
        "--workspace",
    );
    rejected(
        run(
            &[
                "add",
                "project",
                api.to_str().unwrap(),
                "two/conflict",
                "--workspace",
                "one",
            ],
            &fixture.0,
        ),
        "conflict",
    );
    rejected(
        run(
            &[
                "add",
                "project",
                api.to_str().unwrap(),
                "external",
                "--workspace",
                "one",
            ],
            &fixture.0,
        ),
        "within",
    );
    rejected(
        run(&["add", "project", ".", "duplicate"], &one),
        "already registered",
    );
    let no_remote = one.join("no-remote");
    repository(&no_remote);
    git(&no_remote, &["remote", "remove", "origin"]);
    rejected(
        run(&["add", "project", ".", "no-remote"], &no_remote),
        "origin",
    );
    let descendant = one.join("src");
    std::fs::create_dir_all(&descendant).unwrap();
    rejected(
        run(&["add", "project", ".", "subfolder"], &descendant),
        "main worktree root",
    );
    let reserved = one.join(".forge/worktrees/other/99");
    repository(&reserved);
    rejected(
        run(&["add", "project", ".", "reserved"], &reserved),
        ".forge/worktrees",
    );
    let metadata = one.join(".forge/tools");
    repository(&metadata);
    success(run(&["add", "project", ".", "metadata"], &metadata));
    rejected(run(&["workspace", "remove", "one"], &fixture.0), "projects");
    assert!(one.join(".git").exists());
    let linked = one.join("manual-linked");
    git(
        &one,
        &["worktree", "add", "-b", "manual", linked.to_str().unwrap()],
    );
    rejected(
        run(&["add", "project", ".", "linked"], &linked),
        "main worktree",
    );

    // Configuration commands also route Project selectors, rather than accepting
    // a daemon-wide slug that could affect the other Workspace's api Project.
    success(run(
        &[
            "project",
            "setup-hook",
            "set",
            "--project",
            "one/api",
            "echo one",
        ],
        &fixture.0,
    ));
    success(run(
        &[
            "project",
            "setup-hook",
            "set",
            "--project",
            "api",
            "echo two",
            "--workspace",
            "two",
        ],
        &fixture.0,
    ));
    assert_eq!(
        store.project_setup_hook("one/api").unwrap().as_deref(),
        Some("echo one")
    );
    assert_eq!(
        store.project_setup_hook("two/api").unwrap().as_deref(),
        Some("echo two")
    );
    rejected(
        run(
            &[
                "project",
                "setup-hook",
                "set",
                "--project",
                "two/api",
                "wrong",
                "--workspace",
                "one",
            ],
            &fixture.0,
        ),
        "conflict",
    );
    assert_eq!(
        store.project_setup_hook("two/api").unwrap().as_deref(),
        Some("echo two")
    );
    success(run(
        &[
            "dispatcher",
            "config",
            "set",
            "project-limit",
            "2",
            "--project",
            "one/api",
        ],
        &fixture.0,
    ));
    success(run(
        &[
            "dispatcher",
            "config",
            "set",
            "project-limit",
            "3",
            "--project",
            "api",
            "--workspace",
            "two",
        ],
        &fixture.0,
    ));
    assert_eq!(
        store
            .effective_project_task_concurrency_limit("one/api")
            .unwrap(),
        2
    );
    assert_eq!(
        store
            .effective_project_task_concurrency_limit("two/api")
            .unwrap(),
        3
    );
    rejected(
        run(
            &[
                "dispatcher",
                "config",
                "set",
                "project-limit",
                "99",
                "--project",
                "two/api",
                "--workspace",
                "one",
            ],
            &fixture.0,
        ),
        "conflict",
    );
    assert_eq!(
        store
            .effective_project_task_concurrency_limit("two/api")
            .unwrap(),
        3
    );
    store.set_project_setup_hook("one/api", None).unwrap();
    let spec: serde_json::Value = serde_json::from_str(&success(run(
        &[
            "spec",
            "open",
            "--body",
            "Workspace-owned Spec",
            "--project",
            "one/api",
            "--json",
        ],
        &fixture.0,
    )))
    .unwrap();
    assert_eq!(spec["project_scope"]["project_slugs"][0], "one/api");
    let spec_id = spec["id"].as_u64().unwrap().to_string();
    let task: serde_json::Value = serde_json::from_str(&success(run(
        &[
            "task",
            "under-spec",
            "--spec",
            &spec_id,
            "--body",
            "Linked checkout",
            "--workspace",
            "one",
            "--json",
        ],
        &fixture.0,
    )))
    .unwrap();
    let task_id = task["id"].as_u64().unwrap().to_string();
    store
        .set_worktree_root(Some(&one.join("execution")))
        .unwrap();
    let worktree: serde_json::Value = serde_json::from_str(&success(run(
        &[
            "worktree", "create", &task_id, "--branch", "managed", "--base", "HEAD", "--json",
        ],
        &fixture.0,
    )))
    .unwrap();
    let managed = Path::new(worktree["path"].as_str().unwrap());
    rejected(
        run(&["add", "project", ".", "managed-linked"], managed),
        "main worktree",
    );
    assert!(store.project("one/managed-linked").is_err());
    #[cfg(unix)]
    {
        let alias = fixture.0.join("alias");
        std::os::unix::fs::symlink(&nested, &alias).unwrap();
        assert_eq!(here(&["here", "--json"], &alias)["reference"], "one/tools");
        rejected(
            run(&["add", "project", ".", "alias"], &alias),
            "already registered",
        );
        std::os::unix::fs::symlink(&api, one.join("escape")).unwrap();
        rejected(run(&["add", "project", "escape", "escape"], &one), "within");
    }
    // The daemon enforces routing independently of the CLI and never resolves
    // client-relative paths against its own process cwd.
    let client = jsonrpsee::http_client::HttpClientBuilder::default()
        .build(&endpoint)
        .unwrap();
    let error = client
        .register(forge_rpc::RegisterProjectRequest {
            folder: api.to_string_lossy().into_owned(),
            slug: "two/conflicting-api".into(),
            workspace: Some("one".into()),
            cwd: fixture.0.to_string_lossy().into_owned(),
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("conflict"));
    let error = client
        .register(forge_rpc::RegisterProjectRequest {
            folder: ".".into(),
            slug: "relative".into(),
            workspace: Some("one".into()),
            cwd: fixture.0.to_string_lossy().into_owned(),
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("absolute"));
    assert!(store.project("one/relative").is_err());
    assert!(store.project("two/conflicting-api").is_err());
    server.abort();
}
