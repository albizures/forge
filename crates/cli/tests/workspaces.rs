use forge_app::{Application, Project, RegistrationError, RepositoryInspector};
use forge_server::{Daemon, router};
use forge_store::SqliteProjectRepository;
use std::{path::Path, process::Command};

struct NoInspector;
impl RepositoryInspector for NoInspector {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        panic!("no Git inspection")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_management_and_selection_round_trip_through_cli_and_daemon() {
    let root = std::env::temp_dir().join(format!("forge-workspace-cli-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let app = Application::new(
        Box::new(SqliteProjectRepository::in_memory().unwrap()),
        Box::new(NoInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::in_memory().unwrap()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/rpc", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router(Daemon::new(app)))
            .await
            .unwrap();
    });
    let run = |args: &[&str], cwd: &Path| {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .arg("workspace")
            .args(args)
            .args(["--server-url", &url, "--json"])
            .current_dir(cwd)
            .output()
            .unwrap()
    };
    let output = run(
        &[
            "add",
            "personal",
            "--root",
            root.to_str().unwrap(),
            "--name",
            "Personal",
        ],
        Path::new("/"),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["id"], "personal");
    let output = run(&["here"], &root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = run(&["show", "--workspace", "personal"], Path::new("/"));
    assert!(output.status.success());
    let output = run(&["show"], Path::new("/"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--workspace"));
    let output = run(&["rename", "personal", "New name"], Path::new("/"));
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["name"], "New name");
    let output = run(&["list"], Path::new("/"));
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value.as_array().unwrap().len(), 1);
    let output = run(
        &["rename", "personal", "Rejected", "--workspace", "other"],
        Path::new("/"),
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflict"));
    let output = run(&["show", "--workspace", "personal"], Path::new("/"));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["name"], "New name");
    assert_eq!(
        value["root"],
        root.canonicalize().unwrap().to_str().unwrap()
    );
    let output = run(&["remove", "personal"], Path::new("/"));
    assert!(output.status.success());
    assert!(root.is_dir());
    server.abort();
    std::fs::remove_dir_all(root).unwrap();
}
