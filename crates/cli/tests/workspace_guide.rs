use forge_app::Application;
use forge_server::{Daemon, GitInspector, router};
use forge_store::SqliteProjectRepository;
use std::{path::PathBuf, process::Command};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn documented_local_planning_setup_works_without_dispatch_configuration() {
    let fixture =
        Fixture(std::env::temp_dir().join(format!("forge-workspace-guide-{}", std::process::id())));
    let workspace = fixture.0.join("personal");
    let project = workspace.join("api");
    std::fs::create_dir_all(&project).unwrap();
    for args in [
        vec!["init"],
        vec!["remote", "add", "origin", "https://example.test/api.git"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.test",
            "commit",
            "--allow-empty",
            "-m",
            "base",
        ],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(&project)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
    }
    let database = fixture.0.join("forge.sqlite");
    let app = Application::new(
        Box::new(SqliteProjectRepository::open(&database).unwrap()),
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

    // Execute the actual first setup block, substituting its root and requesting JSON.
    // Running outside the registered Workspace also checks explicit routing.
    let guide = include_str!("../../../docs/workspaces.md");
    let block = guide
        .split("```sh\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let mut results = Vec::new();
    for line in block.lines().filter(|line| !line.trim().is_empty()) {
        let mut args = shlex::split(line).unwrap();
        assert_eq!(args.remove(0), "forge");
        for arg in &mut args {
            *arg = arg.replace("/repos/personal", workspace.to_str().unwrap());
        }
        let is_registration = args.first().is_some_and(|arg| arg == "add");
        if !is_registration && !args.iter().any(|arg| arg == "--json") {
            args.push("--json".into());
        }
        let output = Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(&args)
            .args(["--server-url", &endpoint])
            .current_dir(&fixture.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{line}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        results.push(if is_registration {
            serde_json::Value::Null
        } else {
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
        });
    }
    assert_eq!(results.len(), 7);
    assert_eq!(results[0]["id"], "personal");
    assert_eq!(results[1][0]["id"], "personal");
    assert_eq!(
        results[2]["root"],
        workspace.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(results[4]["reference"], "personal/api");
    assert_eq!(results[5]["workspace_id"], "personal");
    assert_eq!(results[5]["record_type"], "spec");
    let ready = results[6]["records"].as_array().unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0]["record"]["id"], results[5]["id"]);
    assert_eq!(ready[0]["record"]["workspace_id"], "personal");
    assert!(!workspace.join(".forge/worktrees").exists());
    server.abort();
}
