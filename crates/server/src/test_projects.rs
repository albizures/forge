//! Explicitly owned Project fixtures for provider/recovery tests.
use forge_app::{Project, ProjectRepository, WorkspaceRepository};
use forge_store::SqliteProjectRepository;
use std::path::Path;

pub fn register(store: &SqliteProjectRepository, slug: &str, root: Option<&Path>) {
    if store.workspace("test").is_err() {
        store
            .create_workspace("test", "Test", Path::new("/"))
            .unwrap();
    }
    let fixture = std::env::temp_dir()
        .join(format!("forge-server-projects-{}", std::process::id()))
        .join(slug);
    let root = root.unwrap_or(&fixture);
    std::fs::create_dir_all(root).unwrap();
    store
        .register(&Project {
            reference: slug.into(),
            workspace_id: "test".into(),
            slug: slug.into(),
            project_root: root.canonicalize().unwrap(),
            origin_url: format!("https://example.test/{slug}.git"),
        })
        .unwrap();
}
