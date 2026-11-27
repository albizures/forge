//! The delivered CLI against the production daemon for an Implementation Assignment: the
//! commands an Agent keeps, the verdict commands Forge closes, and Review Request itself.
use forge_app::{
    AgentDispatchRepository, DispatchPreflightSelection, DispatchWorkspace,
    ManagedWorktreeRepository, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordType, WorkspaceDispatchRepository, WorkspaceRepository,
    check_templates::{CheckCriterion, TemplateReplacement},
    outcomes::Evidence,
};
use forge_server::{DaemonConfig, serve};
use forge_store::SqliteProjectRepository;
use std::{path::Path, process::Command};

fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
        ])
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

async fn cli(endpoint: &str, secret: &Path, args: &[&str]) -> std::process::Output {
    let (endpoint, secret) = (endpoint.to_owned(), secret.to_owned());
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(&args)
            .env("FORGE_ASSIGNMENT_ID", "implementation")
            .env("FORGE_ASSIGNMENT_DAEMON_URL", endpoint)
            .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", secret)
            .output()
            .unwrap()
    })
    .await
    .unwrap()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)] // One daemon lifecycle covers the whole Agent-facing surface.
async fn the_implementation_agent_keeps_context_comments_and_review_request_but_cannot_judge() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().canonicalize().unwrap();
    let db = base.join("db.sqlite");
    let store = SqliteProjectRepository::open(&db).unwrap();
    store.create_workspace("test", "Test", &base).unwrap();
    let repository = base.join("alpha");
    std::fs::create_dir_all(&repository).unwrap();
    git(&repository, &["init"]);
    git(&repository, &["commit", "--allow-empty", "-m", "Input"]);
    store
        .register(&Project {
            reference: "alpha".into(),
            workspace_id: "test".into(),
            slug: "alpha".into(),
            project_root: repository.clone(),
            origin_url: "https://example.test/alpha.git".into(),
        })
        .unwrap();
    let skill = base.join("catalog/.agents/skills/work");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: work\ndescription: work\n---\nWork",
    )
    .unwrap();
    std::fs::create_dir_all(base.join("catalog/.agents/profiles")).unwrap();
    std::fs::write(
        base.join("catalog/.agents/profiles/work.md"),
        "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: [read]\n---\nWork",
    )
    .unwrap();
    let catalog = base.join("catalog");
    let policy = forge_app::agent_profiles::ResourceCatalog::discover(&catalog, &catalog)
        .unwrap()
        .resolve(&catalog, ".agents/profiles/work.md")
        .unwrap();
    let profile = store
        .register_agent_profile("test", RecordType::Task, &policy)
        .unwrap();
    store
        .replace_check_template(
            "alpha",
            &TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Pin".into(),
                criteria: ["Implementation", "Review"]
                    .map(|name| CheckCriterion {
                        name: name.into(),
                        description: name.into(),
                        profile_revision: Some(profile.revision),
                    })
                    .into(),
            },
        )
        .unwrap();
    store
        .select_workspace_profile("test", RecordType::Task, Some(profile.revision))
        .unwrap();
    store
        .select_workspace_profile("test", RecordType::Grilling, Some(1))
        .unwrap();
    let generation = store
        .workspace_dispatch_configuration("test")
        .unwrap()
        .provider_generation;
    store
        .retain_verified_workspace_binding(
            "test",
            Some(&DispatchWorkspace {
                id: "provider".into(),
            }),
            generation,
        )
        .unwrap();
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver the report".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let checkout = store.reserve_worktree_claim(task.id).unwrap().path;
    git(
        &repository,
        &[
            "worktree",
            "add",
            "-b",
            &format!("feat/forge-{}", task.id),
            checkout.to_str().unwrap(),
        ],
    );
    store
        .set_task_base_commit(task.id, &git(&checkout, &["rev-parse", "HEAD"]))
        .unwrap();
    store.activate_worktree_claim(task.id).unwrap();
    let checks = store.list_record_checks(task.id).unwrap();
    store
        .claim_task_check_acceptance(
            task.id,
            "implementation",
            &DispatchPreflightSelection {
                task_revision: profile.revision,
                grilling_revision: 1,
                binding_id: "provider".into(),
                provider_generation: generation,
            },
            &Evidence::Repository {
                repository: repository.display().to_string(),
                commit: git(&checkout, &["rev-parse", "HEAD"]),
            },
            checks[0].id,
            profile.revision,
            None,
        )
        .unwrap();
    drop(store);
    let secrets = base.join("assignment-capabilities");
    std::fs::create_dir_all(&secrets).unwrap();
    let secret = secrets.join("implementation");
    std::fs::write(&secret, "implementation-secret\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
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
    let ok = |output: &std::process::Output| {
        assert!(output.status.success(), "{}", stderr(output));
    };

    // The Agent's bootstrap is minimal and Forge-owned.
    let boot = cli(&endpoint, &secret, &["assignment", "bootstrap", "--json"]).await;
    ok(&boot);
    let boot: serde_json::Value = serde_json::from_slice(&boot.stdout).unwrap();
    assert_eq!(boot["taskBody"], "Deliver the report", "{boot}");
    assert!(
        boot["commandContract"]
            .as_str()
            .unwrap()
            .contains("request-review"),
        "{boot}"
    );
    assert!(boot.get("context").is_none(), "{boot}");

    // What it keeps.
    let task_id = task.id.to_string();
    for args in [
        vec!["assignment", "context", "--json"],
        vec![
            "assignment",
            "comment",
            "--message",
            "Halfway there",
            "--json",
        ],
        vec!["assignment", "finding", "list", "--json"],
        vec!["assignment", "record", &task_id, "--json"],
    ] {
        let output = cli(&endpoint, &secret, &args).await;
        assert!(output.status.success(), "{args:?}: {}", stderr(&output));
    }

    // What Forge closes: no verdict, assessment or Finding surface.
    let outcome = base.join("outcome.json");
    std::fs::write(
        &outcome,
        serde_json::json!({
            "attempt_id": "implementation", "generation": 1,
            "evidence": {"kind": "repository", "repository": repository, "commit": git(&checkout, &["rev-parse", "HEAD"])},
            "verdict": "pass", "actor": "x", "message": "done",
        })
        .to_string(),
    )
    .unwrap();
    let check = checks[0].id.to_string();
    let outcome_path = outcome.to_str().unwrap();
    for args in [
        vec!["assignment", "outcome", "--file", outcome_path],
        vec![
            "assignment",
            "check",
            "assess",
            &check,
            "--expected-version",
            "0",
            "--verdict",
            "pass",
            "--message",
            "Fine",
        ],
        vec![
            "assignment",
            "check",
            "invalidate",
            &check,
            "--expected-version",
            "0",
            "--assessment",
            "1",
            "--message",
            "No",
        ],
        vec![
            "assignment",
            "finding",
            "raise",
            "--severity",
            "major",
            "--blocking",
            "true",
            "--message",
            "Self-raised",
        ],
        vec![
            "assignment",
            "finding",
            "resolve",
            "1",
            "--expected-version",
            "1",
            "--message",
            "Self-resolved",
        ],
        vec![
            "assignment",
            "finding",
            "dismiss",
            "1",
            "--expected-version",
            "1",
            "--message",
            "Self-dismissed",
        ],
    ] {
        let output = cli(&endpoint, &secret, &args).await;
        assert!(!output.status.success(), "{args:?} must fail");
        assert!(
            stderr(&output).contains("implementation-outcome-forbidden"),
            "{args:?}: {}",
            stderr(&output)
        );
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(!stderr(&output).contains("implementation-secret"));
    }

    // Review Request: refused for uncommitted work, accepted once committed, repeatable.
    std::fs::write(checkout.join("report.txt"), "Report\n").unwrap();
    let dirty = cli(&endpoint, &secret, &["assignment", "request-review"]).await;
    assert!(!dirty.status.success());
    assert!(
        stderr(&dirty).contains("review-request-worktree-invalid"),
        "{}",
        stderr(&dirty)
    );
    git(&checkout, &["add", "report.txt"]);
    git(&checkout, &["commit", "-m", "Report"]);
    let accepted = cli(&endpoint, &secret, &["assignment", "request-review"]).await;
    ok(&accepted);
    assert!(
        String::from_utf8_lossy(&accepted.stdout).contains("Review requested"),
        "{}",
        String::from_utf8_lossy(&accepted.stdout)
    );
    ok(&cli(&endpoint, &secret, &["assignment", "request-review"]).await);
    daemon.abort();
}
