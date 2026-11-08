use forge_app::{Project, ProjectRepository, WorkspaceRepository};
use forge_store::SqliteProjectRepository;

pub fn register_project(store: &SqliteProjectRepository, slug: &str) {
    if store.workspace("test").is_err() {
        store
            .create_workspace("test", "Test", std::path::Path::new("/"))
            .unwrap();
    }
    let root = std::env::temp_dir()
        .join(format!("forge-cli-projects-{}", std::process::id()))
        .join(slug);
    std::fs::create_dir_all(&root).unwrap();
    store
        .register(&Project {
            reference: slug.into(),
            workspace_id: "test".into(),
            slug: slug.into(),
            project_root: root.canonicalize().unwrap(),
            origin_url: "https://example.test/repo.git".into(),
        })
        .unwrap();
}
