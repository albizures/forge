//! Real runner bootstrap module -> delivered CLI -> HTTP daemon -> store.
//! This is NOT durable dispatch: the Check is claimed directly, the old binding
//! prerequisite remains, and the JS authorization grant is a test-only fixture.
use forge_app::{
    AgentDispatchRepository, DispatchPreflightSelection, DispatchWorkspace,
    ManagedWorktreeRepository, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordType, WorkspaceDispatchRepository, WorkspaceRepository,
    agent_profiles::ResourceCatalog,
    assignment::BoundIdentity,
    check_templates::{CheckCriterion, TemplateReplacement},
    outcomes::Evidence,
};
use forge_server::{DaemonConfig, serve};
use forge_store::SqliteProjectRepository;
use serde_json::{Value, json};
use std::{
    io::Write,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("forge-t07-cli-{}-{nonce}", std::process::id()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
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
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

async fn bootstrap(input: Value) {
    let output = tokio::task::spawn_blocking(move || {
        let runner = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../runner");
        let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|path| path.join("node"))
            .find(|path| path.is_file())
            .unwrap()
            .canonicalize()
            .unwrap();
        let mut child = Command::new(node)
            .arg(runner.join("test/check-bootstrap-tracer.mjs"))
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&input).unwrap())
            .unwrap();
        child.wait_with_output().unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for bytes in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains("T07-CAPABILITY-CANARY"));
        assert!(!text.contains("T07-BOUND-BEHAVIOR"));
    }
    let _: Value = serde_json::from_slice(&output.stdout).unwrap();
}

async fn outcome(endpoint: &str, secret: &Path, file: &Path) -> Output {
    let (endpoint, secret, file) = (endpoint.to_owned(), secret.to_owned(), file.to_owned());
    tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .env_clear()
            .env("FORGE_ASSIGNMENT_ID", "t07-check")
            .env("FORGE_ASSIGNMENT_DAEMON_URL", endpoint)
            .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", secret)
            .args(["assignment", "outcome", "--file"])
            .arg(file)
            .arg("--json")
            .output()
            .unwrap()
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)] // One isolated real daemon/CLI/bootstrap composition.
async fn runner_gate_uses_real_authenticated_check_bootstrap_and_outcome() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let repository = root.join("repository");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(
        &repository,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "Input",
        ],
    );
    let commit = git(&repository, &["rev-parse", "HEAD"]);
    let db = root.join("store.sqlite");
    let store = SqliteProjectRepository::open(&db).unwrap();
    store.create_workspace("test", "Test", root).unwrap();
    store
        .register(&Project {
            reference: "test/api".into(),
            workspace_id: "test".into(),
            slug: "api".into(),
            project_root: repository.clone(),
            origin_url: "https://example.test/api.git".into(),
        })
        .unwrap();
    std::fs::create_dir_all(root.join(".agents/profiles")).unwrap();
    std::fs::write(root.join(".agents/profiles/work.md"), "---\nversion: 1\nruntime: pi-durable\nmodel: openai/gpt-4.1\nthinkingLevel: off\ntools: [read, write, edit, bash]\n---\nT07-BOUND-BEHAVIOR").unwrap();
    let policy = ResourceCatalog::discover(&root.join("operator"), root)
        .unwrap()
        .resolve(root, ".agents/profiles/work.md")
        .unwrap();
    let profile = store
        .register_agent_profile("test", RecordType::Task, &policy)
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
    // Persisted prerequisite only: no Herdr process/socket/resource is used.
    store
        .retain_verified_workspace_binding(
            "test",
            Some(&DispatchWorkspace {
                id: "legacy-prerequisite".into(),
            }),
            generation,
        )
        .unwrap();
    store
        .replace_check_template(
            "test/api",
            &TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Pin".into(),
                criteria: vec![CheckCriterion {
                    name: "Implementation".into(),
                    description: "Deliver".into(),
                    profile_revision: Some(profile.revision),
                }],
            },
        )
        .unwrap();
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver work".into(),
            project_slugs: vec!["test/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let check = store.list_record_checks(task.id).unwrap().remove(0);
    store.set_task_base_commit(task.id, &commit).unwrap();
    let checkout = store.reserve_worktree_claim(task.id).unwrap();
    git(
        &repository,
        &[
            "worktree",
            "add",
            "-b",
            &forge_app::stacks::task_branch(task.id),
            checkout.path.to_str().unwrap(),
        ],
    );
    store.activate_worktree_claim(task.id).unwrap();
    let evidence = Evidence::Repository {
        repository: repository.display().to_string(),
        commit,
    };
    let (assignment, existing) = store
        .claim_task_check_acceptance(
            task.id,
            "t07-check",
            &DispatchPreflightSelection {
                task_revision: profile.revision,
                grilling_revision: 1,
                binding_id: "legacy-prerequisite".into(),
                provider_generation: generation,
            },
            &evidence,
            check.id,
            profile.revision,
            None,
        )
        .unwrap();
    assert!(!existing);
    let attempt = store
        .assignment_check_attempt("t07-check")
        .unwrap()
        .unwrap();
    let bound = BoundIdentity {
        assignment_id: assignment.id,
        record_id: task.id,
        profile_revision: profile.revision,
        profile_record_type: "task".into(),
        provider: profile.provider,
        agent_kind: profile.agent_kind,
        model: profile.model,
        attempt_number: assignment.attempt_number,
        check_attempt: Some(attempt.clone()),
        execution_directory: checkout.path.clone(),
        provider_workspace: Some("legacy-prerequisite".into()),
    };
    let secrets = root.join("assignment-capabilities");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&secrets)
        .unwrap();
    let secret = secrets.join("t07-check");
    std::fs::write(&secret, "T07-CAPABILITY-CANARY").unwrap();
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let daemon = tokio::spawn(serve(DaemonConfig {
        bind_addr: addr,
        store_path: db,
        durable_model_key_file: None,
        managed: None,
    }));
    let endpoint = format!("http://{addr}");
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let input = json!({"bound": bound, "expected": "accepted", "capability": {
        "FORGE_ASSIGNMENT_ID": "t07-check", "FORGE_ASSIGNMENT_DAEMON_URL": endpoint,
        "FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE": secret,
        "FORGE_ASSIGNMENT_FORGE_CLI": Path::new(env!("CARGO_BIN_EXE_forge")).canonicalize().unwrap()
    }});
    let mut wrong_secret = input.clone();
    wrong_secret["expected"] = "denied".into();
    let foreign = root.join("foreign-capability");
    std::fs::write(&foreign, "wrong-capability").unwrap();
    std::fs::set_permissions(&foreign, std::fs::Permissions::from_mode(0o600)).unwrap();
    wrong_secret["capability"]["FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE"] = json!(foreign);
    bootstrap(wrong_secret).await;
    assert!(!store.assignment_context_read("t07-check").unwrap());
    bootstrap(input.clone()).await;
    assert!(store.assignment_context_read("t07-check").unwrap());
    assert_eq!(
        store.show_record(task.id).unwrap().status,
        forge_app::RecordStatus::InProgress
    );
    assert!(
        store.list_record_checks(task.id).unwrap()[0]
            .latest_assessment
            .is_none()
    );
    let mut stale = input.clone();
    stale["expected"] = "denied".into();
    stale["bound"]["checkAttempt"]["evidence"]["commit"] = "f".repeat(40).into();
    bootstrap(stale).await;
    // Implementation may deliver a new clean commit; retain input A on the
    // attempt while the authenticated Assessment binds output B.
    std::fs::write(checkout.path.join("deliverable.txt"), "Implemented\n").unwrap();
    git(&checkout.path, &["add", "deliverable.txt"]);
    git(
        &checkout.path,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "Output",
        ],
    );
    let output_evidence = Evidence::Repository {
        repository: repository.display().to_string(),
        commit: git(&checkout.path, &["rev-parse", "HEAD"]),
    };
    assert_ne!(evidence, output_evidence);
    let file = root.join("outcome.json");
    let valid = json!({"attempt_id": "t07-check", "generation": attempt.generation, "evidence": output_evidence, "verdict": "pass", "actor": "spoofed", "message": "Delivered", "disposition": "complete", "findings": [], "verifications": []});
    for (invalid, code) in [
        (json!({"attempt_id": "foreign"}), "attempt-not-found"),
        (
            json!({"generation": attempt.generation + 1}),
            "stale-generation",
        ),
    ] {
        let mut request = valid.clone();
        for (key, value) in invalid.as_object().unwrap() {
            request[key] = value.clone();
        }
        std::fs::write(&file, serde_json::to_vec(&request).unwrap()).unwrap();
        let rejected = outcome(&endpoint, &secret, &file).await;
        assert!(!rejected.status.success());
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains(code),
            "expected {code}: {}",
            String::from_utf8_lossy(&rejected.stderr)
        );
        assert!(!String::from_utf8_lossy(&rejected.stderr).contains("T07-CAPABILITY-CANARY"));
        assert!(
            store.list_record_checks(task.id).unwrap()[0]
                .latest_assessment
                .is_none()
        );
    }
    std::fs::write(&file, serde_json::to_vec(&valid).unwrap()).unwrap();
    let unauthorized = outcome(&endpoint, &foreign, &file).await;
    assert!(!unauthorized.status.success());
    assert!(String::from_utf8_lossy(&unauthorized.stderr).contains("assignment-capability-denied"));
    let accepted = outcome(&endpoint, &secret, &file).await;
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let result: forge_app::outcomes::AcceptedOutcome =
        serde_json::from_slice(&accepted.stdout).unwrap();
    assert_eq!(result.attempt, attempt);
    assert_eq!(result.evidence, output_evidence);
    let assessment = result.check.latest_assessment.as_ref().unwrap();
    assert_eq!(assessment.actor, "t07-check");
    assert_eq!(assessment.assignment_id.as_deref(), Some("t07-check"));
    assert_eq!(assessment.evidence, output_evidence);
    assert_eq!(assessment.generation, attempt.generation);
    assert!(!String::from_utf8_lossy(&accepted.stdout).contains("T07-CAPABILITY-CANARY"));
    let replay = outcome(&endpoint, &secret, &file).await;
    assert!(replay.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&accepted.stdout).unwrap(),
        serde_json::from_slice::<Value>(&replay.stdout).unwrap()
    );
    assert_eq!(
        store.list_record_checks(task.id).unwrap()[0].current_verdict,
        Some(forge_app::checks::Verdict::Pass)
    );
    assert_eq!(
        store.assignment_check_attempt("t07-check").unwrap(),
        Some(attempt)
    );
    store
        .close_agent_assignment(
            "t07-check",
            forge_app::AgentAssignmentClosureReason::Retired,
        )
        .unwrap();
    let mut revoked = input;
    revoked["expected"] = "denied".into();
    bootstrap(revoked).await;
    // Exact accepted replay survives closure; closure cannot rewrite that result.
    let closed_replay = outcome(&endpoint, &secret, &file).await;
    assert!(closed_replay.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&accepted.stdout).unwrap(),
        serde_json::from_slice::<Value>(&closed_replay.stdout).unwrap()
    );
    let mut conflicting = valid;
    conflicting["message"] = "Different submission after revocation".into();
    std::fs::write(&file, serde_json::to_vec(&conflicting).unwrap()).unwrap();
    let rejected = outcome(&endpoint, &secret, &file).await;
    assert!(!rejected.status.success());
    daemon.abort();
    let _ = daemon.await;
}
