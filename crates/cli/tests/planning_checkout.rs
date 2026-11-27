use forge_app::{
    Application, NewTypedRecord, Project, ProjectRepository, RecordRepository, RecordType,
    WorkspaceRepository,
};
use forge_server::{Daemon, GitInspector, router};
use forge_store::SqliteProjectRepository;
use std::{path::Path, process::Command};

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)] // One scenario shares the daemon and checkout state across observations.
async fn planning_records_capture_only_the_initiating_checkout() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let main = root.join("main");
    let linked = root.join("linked");
    let other = root.join("other");
    std::fs::create_dir(&main).unwrap();
    std::fs::create_dir(&other).unwrap();
    git(&main, &["init", "-b", "main"]);
    git(
        &main,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.test",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    git(
        &main,
        &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
    );
    git(&other, &["init"]);
    let db = root.join("state.sqlite");
    let store = SqliteProjectRepository::open(&db).unwrap();
    store.create_workspace("one", "One", &root).unwrap();
    store
        .register(&Project {
            reference: "one/api".into(),
            workspace_id: "one".into(),
            slug: "api".into(),
            project_root: main.clone(),
            origin_url: "https://example.test/api.git".into(),
        })
        .unwrap();
    store
        .register(&Project {
            reference: "one/other".into(),
            workspace_id: "one".into(),
            slug: "other".into(),
            project_root: other.clone(),
            origin_url: "https://example.test/other.git".into(),
        })
        .unwrap();
    let scoped = store.in_record_workspace("one").unwrap();
    let parent = scoped
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Wayfinder,
            body: "parent".into(),
            project_slugs: vec!["one/api".into(), "one/other".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let app = Application::new(Box::new(store), Box::new(GitInspector))
        .with_workspace_repository(Box::new(SqliteProjectRepository::open(&db).unwrap()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/rpc", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router(Daemon::new(app)))
            .await
            .unwrap();
    });
    let run = |cwd: &Path, args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(args)
            .args(["--server-url", &endpoint, "--workspace", "one", "--json"])
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let spec = |cwd: &Path| {
        run(
            cwd,
            &["spec", "open", "--project", "one/api", "--body", "test"],
        )
    };
    assert_eq!(
        spec(&main)["planning_context"][0]["context"]["branch"],
        "main"
    );
    let linked_spec = spec(&linked);
    assert_eq!(
        linked_spec["planning_context"][0]["context"]["branch"],
        "feature"
    );
    for cwd in [&root, &other] {
        let context = &spec(cwd)["planning_context"][0]["context"];
        assert_eq!(context["status"], "unavailable");
        assert!(!context["reason"].as_str().unwrap().is_empty());
    }
    let grilling = run(
        &linked,
        &[
            "grilling",
            "open",
            "--wayfinder",
            &parent.id.to_string(),
            "--body",
            "test",
        ],
    );
    assert_eq!(
        grilling["planning_context"][0]["context"]["branch"],
        "feature"
    );
    assert_eq!(grilling["planning_context"][1]["project"], "one/other");
    assert_eq!(
        grilling["planning_context"][1]["context"]["status"],
        "unavailable"
    );
    assert!(
        !grilling["planning_context"][1]["context"]["reason"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    git(&linked, &["checkout", "--detach"]);
    let detached = spec(&linked);
    assert_eq!(
        detached["planning_context"][0]["context"]["status"],
        "unavailable"
    );
    assert!(
        detached["planning_context"][0]["context"]["reason"]
            .as_str()
            .unwrap()
            .contains("branch unavailable")
    );
    let detached_id = detached["id"].as_u64().unwrap().to_string();
    let human = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args([
            "record",
            "show",
            &detached_id,
            "--server-url",
            &endpoint,
            "--workspace",
            "one",
        ])
        .current_dir(&other)
        .output()
        .unwrap();
    assert!(human.status.success());
    assert!(
        String::from_utf8_lossy(&human.stdout)
            .contains("Planning context one/api: unavailable (branch unavailable")
    );
    let id = linked_spec["id"].as_u64().unwrap().to_string();
    let shown = run(&other, &["record", "show", &id]);
    assert_eq!(shown["planning_context"], linked_spec["planning_context"]);
    server.abort();
}
