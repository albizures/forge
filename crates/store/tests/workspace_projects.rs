use forge_app::{
    AgentDispatchRepository, Application, ManagedWorktreeRepository, NewTypedRecord, Project,
    ProjectRepository, RecordRepository, RecordType, RegistrationError, RepositoryInspector,
    WorkspaceRepository,
};
use forge_store::SqliteProjectRepository;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "forge-workspace-projects-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct GitInspector;
impl RepositoryInspector for GitInspector {
    fn inspect(&self, folder: &Path) -> Result<Project, RegistrationError> {
        let folder = folder.canonicalize().unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&folder)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success());
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        if git(&["rev-parse", "--git-dir"]) != ".git" {
            return Err(RegistrationError::new(
                "linked-worktree",
                "only main checkouts may be Projects",
            ));
        }
        let origin_url = git(&["remote", "get-url", "origin"]);
        Ok(Project {
            reference: String::new(),
            workspace_id: String::new(),
            slug: String::new(),
            project_root: folder,
            origin_url,
        })
    }
}

fn application(database: &Path) -> Application {
    Application::new(
        Box::new(SqliteProjectRepository::open(database).unwrap()),
        Box::new(GitInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(database).unwrap()))
}

#[test]
#[allow(clippy::too_many_lines)]
fn same_local_slugs_do_not_share_dispatch_capacity_and_consumption_survives_reopen() {
    let fixture = Fixture::new();
    let one = fixture.path().join("one");
    let two = fixture.path().join("two");
    repository(&one);
    repository(&two);
    let database = fixture.path().join("store.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("one", "One", &one).unwrap();
    store.create_workspace("two", "Two", &two).unwrap();
    for workspace in ["one", "two"] {
        for kind in [RecordType::Task, RecordType::Grilling] {
            forge_app::WorkspaceDispatchRepository::select_workspace_profile(
                &store,
                workspace,
                kind,
                Some(1),
            )
            .unwrap();
        }
        forge_app::WorkspaceDispatchRepository::retain_verified_workspace_binding(
            &store,
            workspace,
            Some(&forge_app::DispatchWorkspace {
                id: format!("provider-{workspace}"),
            }),
            0,
        )
        .unwrap();
    }
    let app = application(&database);
    app.register_workspace_project("one", &one, "api").unwrap();
    app.register_workspace_project("two", &two, "api").unwrap();
    for reference in ["one/api", "two/api"] {
        for _ in 0..2 {
            store
                .open_typed_record(&NewTypedRecord {
                    record_type: RecordType::Task,
                    task_kind: None,
                    body: "work".into(),
                    project_slugs: vec![reference.into()],
                    generated_by_ids: vec![],
                })
                .unwrap();
        }
    }
    let first = store
        .claim_next_agent_assignment("one/api", "one-first")
        .unwrap()
        .unwrap();
    assert_eq!(first.project_slug, "one/api");
    assert!(
        store
            .claim_next_agent_assignment("one/api", "one-blocked")
            .unwrap()
            .is_none()
    );
    let second = store
        .claim_next_agent_assignment("two/api", "two-first")
        .unwrap()
        .unwrap();
    assert_eq!(second.project_slug, "two/api");
    assert_ne!(first.record_id, second.record_id);
    assert!(
        store
            .claim_next_agent_assignment("two/api", "two-blocked")
            .unwrap()
            .is_none()
    );
    store
        .set_project_task_concurrency_limit("two/api", Some(2))
        .unwrap();
    assert_eq!(
        store
            .claim_next_agent_assignment("two/api", "two-second")
            .unwrap()
            .unwrap()
            .project_slug,
        "two/api"
    );
    assert!(
        store
            .claim_next_agent_assignment("one/api", "still-blocked")
            .unwrap()
            .is_none()
    );
    drop(app);
    drop(store);
    let reopened = SqliteProjectRepository::open(&database).unwrap();
    assert_eq!(reopened.list_open_agent_assignments().unwrap().len(), 3);
    assert!(
        reopened
            .claim_next_agent_assignment("one/api", "restart-blocked")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        reopened
            .effective_project_task_concurrency_limit("one/api")
            .unwrap(),
        1
    );
    assert_eq!(
        reopened
            .effective_project_task_concurrency_limit("two/api")
            .unwrap(),
        2
    );
}

#[test]
fn registration_requires_ownership_and_rejects_local_slug_reuse_or_reparenting() {
    let fixture = Fixture::new();
    let one = fixture.path().join("one");
    let nested = one.join("nested");
    let two = fixture.path().join("two");
    repository(&one);
    repository(&nested);
    repository(&two);
    let database = fixture.path().join("store.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("one", "One", &one).unwrap();
    store.create_workspace("two", "Two", &two).unwrap();
    let app = application(&database);
    let original = app.register_workspace_project("one", &one, "api").unwrap();
    let mut attempted = original.clone();
    attempted.workspace_id.clear();
    assert_eq!(
        store.register(&attempted).unwrap_err().code,
        "invalid-project-reference"
    );
    attempted = original.clone();
    attempted.reference = "another-identity".into();
    attempted.project_root = nested.canonicalize().unwrap();
    assert_eq!(
        store.register(&attempted).unwrap_err().code,
        "slug-conflict"
    );
    assert!(store.project("another-identity").is_err());
    attempted = original.clone();
    attempted.workspace_id = "two".into();
    attempted.project_root = two.canonicalize().unwrap();
    assert_eq!(
        store.register(&attempted).unwrap_err().code,
        "invalid-project-reference"
    );
    attempted = original.clone();
    attempted.reference = "../escape".into();
    assert_eq!(
        store.register(&attempted).unwrap_err().code,
        "invalid-project-reference"
    );
    assert_eq!(store.project(&original.reference).unwrap(), original);
    assert!(store.project_in_workspace("two", "api").is_err());
}

#[test]
fn unowned_development_projects_are_rejected_instead_of_silently_adopted() {
    let fixture = Fixture::new();
    let database = fixture.path().join("unowned.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("CREATE TABLE projects (slug TEXT PRIMARY KEY NOT NULL, registered_folder TEXT UNIQUE NOT NULL, origin_url TEXT NOT NULL); INSERT INTO projects VALUES ('api', '/old/api', 'https://example.test/repo.git');").unwrap();
    drop(connection);
    let error = SqliteProjectRepository::open(&database)
        .err()
        .expect("an unowned Project must not be adopted");
    assert_eq!(error.code, "project-workspace-required");
    assert!(error.message.contains("new store"));
}

fn repository(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    for args in [
        &["init"][..],
        &["remote", "add", "origin", "https://example.com/repo.git"],
    ] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(path)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}

#[test]
fn workspace_project_references_keep_identical_local_slugs_and_configuration_independent() {
    let temp = Fixture::new();
    let one = temp.path().join("one");
    let two = temp.path().join("two");
    repository(&one);
    repository(&two);
    let db = temp.path().join("forge.db");
    let store = SqliteProjectRepository::open(&db).unwrap();
    store.create_workspace("one", "One", &one).unwrap();
    store.create_workspace("two", "Two", &two).unwrap();
    let app = Application::new(
        Box::new(SqliteProjectRepository::open(&db).unwrap()),
        Box::new(GitInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(&db).unwrap()));
    let first = app.register_workspace_project("one", &one, "api").unwrap();
    let second = app.register_workspace_project("two", &two, "api").unwrap();
    assert_eq!(first.reference, "one/api");
    assert_eq!(first.slug, "api");
    assert_eq!(first.workspace_id, "one");
    assert_eq!(second.reference, "two/api");
    assert_eq!(second.slug, "api");
    assert_eq!(
        store.project("one/api").unwrap().project_root,
        one.canonicalize().unwrap()
    );
    assert_eq!(
        store.project("two/api").unwrap().project_root,
        two.canonicalize().unwrap()
    );
    store
        .set_project_setup_hook("one/api", Some("echo one"))
        .unwrap();
    store
        .set_project_setup_hook("two/api", Some("echo two"))
        .unwrap();
    assert_eq!(
        store.project_setup_hook("one/api").unwrap().as_deref(),
        Some("echo one")
    );
    assert_eq!(
        store.project_setup_hook("two/api").unwrap().as_deref(),
        Some("echo two")
    );
    store
        .set_project_task_concurrency_limit("one/api", Some(2))
        .unwrap();
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
        1
    );
    assert_eq!(
        store.remove_workspace("one").unwrap_err().code,
        "workspace-not-empty"
    );
    drop(app);
    drop(store);
    let reopened = SqliteProjectRepository::open(&db).unwrap();
    assert_eq!(reopened.project("one/api").unwrap(), first);
    assert_eq!(reopened.project("two/api").unwrap(), second);
}

#[test]
fn project_selection_prefers_nested_roots_and_explicit_references_override_cwd() {
    let fixture = Fixture::new();
    let outer = fixture.path().join("outer");
    let inner = outer.join("inner");
    let other = fixture.path().join("other");
    repository(&outer);
    repository(&inner);
    repository(&other);
    let db = fixture.path().join("forge.db");
    let store = SqliteProjectRepository::open(&db).unwrap();
    store.create_workspace("one", "One", &outer).unwrap();
    store.create_workspace("two", "Two", &other).unwrap();
    let app = Application::new(
        Box::new(SqliteProjectRepository::open(&db).unwrap()),
        Box::new(GitInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(&db).unwrap()));
    let root = app
        .register_workspace_project("one", &outer, "root")
        .unwrap();
    let nested = app
        .register_workspace_project("one", &inner, "nested")
        .unwrap();
    let second = app
        .register_workspace_project("two", &other, "api")
        .unwrap();
    let descendant = inner.join("src");
    std::fs::create_dir_all(&descendant).unwrap();
    assert_eq!(app.select_project(None, None, &descendant).unwrap(), nested);
    assert_eq!(app.select_project(None, None, &outer).unwrap(), root);
    assert_eq!(
        app.select_project(None, Some("one/root"), &descendant)
            .unwrap(),
        root
    );
    assert_eq!(
        app.select_project(Some("one"), Some("root"), fixture.path())
            .unwrap(),
        root
    );
    assert_eq!(
        app.select_project(None, Some("two/api"), &descendant)
            .unwrap(),
        second
    );
    assert_eq!(
        app.select_project(Some("one"), Some("two/api"), &descendant)
            .unwrap_err()
            .code,
        "workspace-conflict"
    );
    assert_eq!(
        app.select_project(None, None, fixture.path())
            .unwrap_err()
            .code,
        "workspace-required"
    );
    assert_eq!(
        app.select_project(Some("two"), None, &inner)
            .unwrap_err()
            .code,
        "project-not-found"
    );
}

#[test]
fn registration_rejects_external_reserved_duplicate_and_noncanonical_roots_without_writes() {
    let fixture = Fixture::new();
    let root = fixture.path().join("workspace");
    let external = fixture.path().join("external");
    let reserved = root.join(".forge/worktrees/api/42");
    repository(&root);
    repository(&external);
    repository(&reserved);
    let db = fixture.path().join("forge.db");
    let store = SqliteProjectRepository::open(&db).unwrap();
    store.create_workspace("one", "One", &root).unwrap();
    let app = Application::new(
        Box::new(SqliteProjectRepository::open(&db).unwrap()),
        Box::new(GitInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(&db).unwrap()));
    let registered = app.register_workspace_project("one", &root, "api").unwrap();
    assert_eq!(
        app.register_workspace_project("one", &root, "api").unwrap(),
        registered
    );
    assert_eq!(
        app.register_workspace_project("one", &root, "duplicate")
            .unwrap_err()
            .code,
        "folder-conflict"
    );
    assert_eq!(
        app.register_workspace_project("one", &external, "api")
            .unwrap_err()
            .code,
        "project-outside-workspace"
    );
    assert_eq!(
        app.register_workspace_project("one", &reserved, "reserved")
            .unwrap_err()
            .code,
        "reserved-project-root"
    );
    assert_eq!(
        app.register_workspace_project("one", &root, "../escape")
            .unwrap_err()
            .code,
        "invalid-project-reference"
    );
    // Durable registration enforces ownership as well; bypassing the application
    // does not permit a qualified identity in a different Workspace's filesystem.
    let invalid = Project {
        reference: "one/external".into(),
        workspace_id: "one".into(),
        slug: "external".into(),
        project_root: external.canonicalize().unwrap(),
        origin_url: registered.origin_url.clone(),
    };
    assert_eq!(
        store.register(&invalid).unwrap_err().code,
        "project-outside-workspace"
    );
    assert_eq!(
        store.project("one/external").unwrap_err().code,
        "project-not-found"
    );
    assert_eq!(
        store.project("one/duplicate").unwrap_err().code,
        "project-not-found"
    );
    assert!(reserved.join(".git").exists());
}

#[cfg(unix)]
#[test]
fn aliases_do_not_escape_ownership_or_bypass_reserved_subtrees() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let root = fixture.path().join("workspace");
    let outside = fixture.path().join("outside");
    let reserved_target = root.join("checkouts");
    repository(&root);
    repository(&outside);
    repository(&reserved_target);
    std::fs::create_dir_all(root.join(".forge")).unwrap();
    symlink(&reserved_target, root.join(".forge/worktrees")).unwrap();
    symlink(&outside, root.join("escape")).unwrap();
    symlink(&root, fixture.path().join("alias")).unwrap();
    let db = fixture.path().join("forge.db");
    let store = SqliteProjectRepository::open(&db).unwrap();
    store.create_workspace("one", "One", &root).unwrap();
    let app = Application::new(
        Box::new(SqliteProjectRepository::open(&db).unwrap()),
        Box::new(GitInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(&db).unwrap()));
    let registered = app.register_workspace_project("one", &root, "api").unwrap();
    assert_eq!(
        app.register_workspace_project("one", &fixture.path().join("alias"), "api")
            .unwrap(),
        registered
    );
    assert_eq!(
        app.select_project(None, None, &fixture.path().join("alias"))
            .unwrap(),
        registered
    );
    assert_eq!(
        app.register_workspace_project("one", &root.join("escape"), "escape")
            .unwrap_err()
            .code,
        "project-outside-workspace"
    );
    assert_eq!(
        app.register_workspace_project("one", &reserved_target, "reserved")
            .unwrap_err()
            .code,
        "reserved-project-root"
    );
    assert_eq!(
        store
            .register(&Project {
                reference: "one/alias".into(),
                workspace_id: "one".into(),
                slug: "alias".into(),
                project_root: fixture.path().join("alias"),
                origin_url: registered.origin_url
            })
            .unwrap_err()
            .code,
        "invalid-project-root"
    );
}
