use forge_app::{
    Application, Project, ProjectRepository, RegistrationError, RepositoryInspector,
    WorkspaceRepository,
};
use forge_store::SqliteProjectRepository;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct NoInspector;
impl RepositoryInspector for NoInspector {
    fn inspect(&self, _: &std::path::Path) -> Result<Project, RegistrationError> {
        panic!("Workspace management does not inspect Git")
    }
}

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "forge-workspaces-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn dir(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn registration_and_rename_survive_reopen_without_changing_identity_or_root() {
    let fixture = Fixture::new();
    let root = fixture.dir("workspace");
    let database = fixture.0.join("store.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    let registered = store
        .create_workspace("personal", "Personal", &root)
        .unwrap();
    assert_eq!(registered.id, "personal");
    assert_eq!(registered.root, root.canonicalize().unwrap());
    store.rename_workspace("personal", "My work").unwrap();
    drop(store);
    let store = SqliteProjectRepository::open(&database).unwrap();
    let workspace = store.workspace("personal").unwrap();
    assert_eq!(workspace.name, "My work");
    assert_eq!(workspace.root, registered.root);
    assert_eq!(store.workspaces().unwrap(), vec![workspace]);
}

#[test]
fn invalid_registration_and_rename_leave_the_store_unchanged() {
    let fixture = Fixture::new();
    let root = fixture.dir("workspace");
    let store = SqliteProjectRepository::in_memory().unwrap();
    assert_eq!(
        store
            .create_workspace("bad/id", "Name", &root)
            .unwrap_err()
            .code,
        "invalid-workspace-id"
    );
    assert_eq!(
        store
            .create_workspace("valid", " ", &root)
            .unwrap_err()
            .code,
        "invalid-workspace-name"
    );
    assert_eq!(
        store
            .create_workspace("valid", "Name", &root.join("missing"))
            .unwrap_err()
            .code,
        "invalid-workspace-root"
    );
    let file = root.join("file");
    std::fs::write(&file, "keep").unwrap();
    assert_eq!(
        store
            .create_workspace("valid", "Name", &file)
            .unwrap_err()
            .code,
        "invalid-workspace-root"
    );
    assert!(store.workspaces().unwrap().is_empty());
    let workspace = store.create_workspace("valid", "Name", &root).unwrap();
    assert_eq!(
        store
            .create_workspace("valid", "Other", &fixture.0)
            .unwrap_err()
            .code,
        "workspace-id-taken"
    );
    assert_eq!(
        store.rename_workspace("valid", "\n ").unwrap_err().code,
        "invalid-workspace-name"
    );
    assert_eq!(store.workspace("valid").unwrap(), workspace);
}

#[test]
fn canonical_roots_reject_equal_nested_ancestor_and_symlink_aliases() {
    let fixture = Fixture::new();
    let root = fixture.dir("work");
    let child = fixture.dir("work/nested");
    let sibling = fixture.dir("work-other");
    let store = SqliteProjectRepository::in_memory().unwrap();
    store.create_workspace("one", "One", &root).unwrap();
    for (id, path) in [
        ("equal", &root),
        ("child", &child),
        ("ancestor", &fixture.0),
    ] {
        assert_eq!(
            store
                .create_workspace(id, "Rejected", path)
                .unwrap_err()
                .code,
            "workspace-root-overlap"
        );
    }
    #[cfg(unix)]
    {
        let alias = fixture.0.join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        assert_eq!(
            store
                .create_workspace("alias", "Alias", &alias)
                .unwrap_err()
                .code,
            "workspace-root-overlap"
        );
    }
    store.create_workspace("two", "Two", &sibling).unwrap();
    assert_eq!(store.workspaces().unwrap().len(), 2);
}

#[test]
fn selection_is_explicit_first_without_a_saved_default_and_rejects_conflicts() {
    let fixture = Fixture::new();
    let root = fixture.dir("one");
    let nested = fixture.dir("one/nested");
    let other = fixture.dir("two");
    let store = SqliteProjectRepository::in_memory().unwrap();
    store.create_workspace("one", "One", &root).unwrap();
    store.create_workspace("two", "Two", &other).unwrap();
    let app = Application::new(
        Box::new(SqliteProjectRepository::in_memory().unwrap()),
        Box::new(NoInspector),
    )
    .with_workspace_repository(Box::new(store));
    assert_eq!(app.select_workspace(None, None, &nested).unwrap().id, "one");
    assert_eq!(
        app.select_workspace(Some("two"), None, &nested).unwrap().id,
        "two"
    );
    assert_eq!(
        app.select_workspace(None, Some("two"), &fixture.0)
            .unwrap()
            .id,
        "two"
    );
    assert_eq!(
        app.select_workspace(None, None, &fixture.0)
            .unwrap_err()
            .code,
        "workspace-required"
    );
    assert_eq!(
        app.select_workspace(Some("one"), Some("two"), &nested)
            .unwrap_err()
            .code,
        "workspace-conflict"
    );
    assert_eq!(
        app.select_workspace(Some("missing"), None, &nested)
            .unwrap_err()
            .code,
        "workspace-not-found"
    );
    #[cfg(unix)]
    {
        let alias = fixture.0.join("alias");
        std::os::unix::fs::symlink(&nested, &alias).unwrap();
        assert_eq!(app.select_workspace(None, None, &alias).unwrap().id, "one");
    }
}

#[test]
fn removal_is_non_destructive_and_rejects_projects_inside_the_boundary() {
    let fixture = Fixture::new();
    let root = fixture.dir("workspace");
    let file = root.join("keep.txt");
    std::fs::write(&file, "keep").unwrap();
    let store = SqliteProjectRepository::in_memory().unwrap();
    store.create_workspace("empty", "Empty", &root).unwrap();
    store.remove_workspace("empty").unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "keep");
    assert!(store.workspaces().unwrap().is_empty());
    store.create_workspace("owned", "Owned", &root).unwrap();
    std::fs::create_dir_all(root.join("api")).unwrap();
    store
        .register(&Project {
            reference: "owned/api".into(),
            workspace_id: "owned".into(),
            slug: "api".into(),
            project_root: root.join("api").canonicalize().unwrap(),
            origin_url: "https://example.com/api".into(),
        })
        .unwrap();
    assert_eq!(
        store.remove_workspace("owned").unwrap_err().code,
        "workspace-not-empty"
    );
    assert!(store.workspace("owned").is_ok());
    let independent = fixture.dir("independent");
    store
        .create_workspace("independent", "Independent", &independent)
        .unwrap();
    store.remove_workspace("independent").unwrap();
    assert!(independent.is_dir());
    assert_eq!(std::fs::read_to_string(file).unwrap(), "keep");
}
