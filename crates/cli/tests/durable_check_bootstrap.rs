//! Real runner bootstrap module -> delivered CLI -> HTTP daemon -> store.
//! Application Check dispatch and Git worktree preparation feed the captured
//! transport into a real Harness/Bash/CLI Outcome path. The JS authorization
//! grant remains a test-only fixture, NOT supervised execution. No Herdr binding.
#[path = "support/durable_dispatch.rs"]
mod durable_dispatch;
use forge_app::{
    AgentDispatchRepository, Application, ManagedWorktreeRepository, NewTypedRecord, Project,
    ProjectRepository, RecordRepository, RecordType, WorkspaceDispatchRepository,
    WorkspaceRepository,
    agent_profiles::ResourceCatalog,
    assignment::BoundIdentity,
    check_templates::{CheckCriterion, TemplateReplacement},
    durable_execution::{
        Admission, Launch, Repository as _,
        completion::{AnswerObservation, Repository as _},
        effects::{InterruptedEffect, Repository as _},
    },
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
    sync::{Arc, Mutex},
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

fn node() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("node"))
        .find(|path| path.is_file())
        .unwrap()
        .canonicalize()
        .unwrap()
}

async fn component_hash() -> String {
    tokio::task::spawn_blocking(|| {
        let contract = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../runner/src/contract.ts")
            .canonicalize()
            .unwrap();
        let output = Command::new(node())
            .env_clear()
            .args(["--input-type=module", "-e"])
            .arg(format!(
                "import {{componentHash}} from {}; process.stdout.write(componentHash());",
                json!(contract)
            ))
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    })
    .await
    .unwrap()
}

async fn bootstrap(input: Value) -> Value {
    let output = tokio::task::spawn_blocking(move || {
        let runner = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../runner");
        let mut child = Command::new(node())
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
        assert!(!text.contains("T07-MODEL-KEY-CANARY"));
    }
    serde_json::from_slice(&output.stdout).unwrap()
}

async fn outcome(assignment_id: &str, endpoint: &str, secret: &Path, file: &Path) -> Output {
    let (assignment_id, endpoint, secret, file) = (
        assignment_id.to_owned(),
        endpoint.to_owned(),
        secret.to_owned(),
        file.to_owned(),
    );
    tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .env_clear()
            .env("FORGE_ASSIGNMENT_ID", assignment_id)
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
async fn runner_gate_uses_real_authenticated_check_bootstrap_and_outcome() {
    check_execution("accepted").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn final_prose_without_outcome_retains_missing_outcome_from_actual_runtime_answer() {
    check_execution("missing").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_outcome_then_final_prose_remains_missing_outcome() {
    check_execution("rejected").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interrupted_external_mutation_retains_forge_blocker_across_daemon_restart() {
    check_execution("effect-before-return").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn returned_tool_without_committed_receipt_retains_forge_blocker_across_daemon_restart() {
    check_execution("effect-before-commit").await;
}

async fn interrupted_effect_process(
    runtime: &Path,
    launch: &Launch,
    checkout: &Path,
    key: &Path,
    before_commit: bool,
) -> Vec<InterruptedEffect> {
    // Dirty tracked/index/untracked state must survive death and inspection.
    std::fs::write(checkout.join("tracked.txt"), "staged bytes\n").unwrap();
    git(checkout, &["add", "tracked.txt"]);
    std::fs::write(checkout.join("tracked.txt"), "unstaged bytes\n").unwrap();
    std::fs::write(checkout.join("untracked.txt"), "untracked bytes\n").unwrap();
    let status = git(checkout, &["status", "--porcelain=v1"]);
    let index = git(checkout, &["show", ":tracked.txt"]);
    let input = json!({ "directory": runtime, "worktree": checkout, "key": key, "launch": launch, "beforeResultCommit": before_commit });
    let output = tokio::task::spawn_blocking(move || {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../runner/test/retained-effect-tracer.mjs");
        let mut child = Command::new(node())
            .env_clear()
            .arg(script)
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
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["mutations"], 1);
    assert_eq!(
        report["boundary"],
        if before_commit {
            "before-result-commit"
        } else {
            "after-external-effect"
        }
    );
    assert_eq!(git(checkout, &["status", "--porcelain=v1"]), status);
    assert_eq!(git(checkout, &["show", ":tracked.txt"]), index);
    assert_eq!(
        std::fs::read(checkout.join("tracked.txt")).unwrap(),
        b"unstaged bytes\n"
    );
    assert_eq!(
        std::fs::read(checkout.join("untracked.txt")).unwrap(),
        b"untracked bytes\n"
    );
    serde_json::from_value(report["effects"].clone()).unwrap()
}

async fn inspect_interrupted_effect(endpoint: &str, assignment: &str) {
    for json_output in [false, true] {
        let endpoint = format!("{endpoint}/rpc");
        let assignment = assignment.to_owned();
        let output = tokio::task::spawn_blocking(move || {
            let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
            command.env_clear().args([
                "dispatcher",
                "assignment-info",
                &assignment,
                "--workspace",
                "test",
                "--server-url",
                &endpoint,
            ]);
            if json_output {
                command.arg("--json");
            }
            command.output().unwrap()
        })
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("interrupted-effect"));
        assert!(!text.contains("T07-CAPABILITY-CANARY"));
        assert!(!text.contains("T07-MODEL-KEY-CANARY"));
        if json_output {
            let info: forge_rpc::AgentAssignmentInfoResponse = serde_json::from_str(&text).unwrap();
            assert!(info.capacity_reserved);
            assert!(
                info.recovery_blockers
                    .contains(&"interrupted-effect".into())
            );
        }
    }
}

#[allow(clippy::too_many_lines)] // One isolated real daemon/CLI/bootstrap composition.
async fn check_execution(mode: &str) {
    let fixture = Fixture::new();
    let root = &fixture.0;
    // The trusted runtime mapping rejects group-writable ancestors regardless
    // of the test host's umask; establish the owned boundary before Git setup.
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(root.join(".forge"))
        .unwrap();
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
    // Check pins, not current Workspace selections, determine Task execution.
    store
        .select_workspace_profile("test", RecordType::Task, None)
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
    let model_auth = root.join("model-auth");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&model_auth)
        .unwrap();
    let model_key = model_auth.join("key");
    std::fs::write(&model_key, "T07-MODEL-KEY-CANARY").unwrap();
    std::fs::set_permissions(&model_key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let launches = Arc::new(Mutex::new(vec![]));
    let app = Application::with_managed_worktrees_and_agent_dispatch(
        Box::new(SqliteProjectRepository::open(&db).unwrap()),
        Box::new(SqliteProjectRepository::open(&db).unwrap()),
        Box::new(forge_server::GitWorktreeAdapter),
        Box::new(SqliteProjectRepository::open(&db).unwrap()),
        Box::new(durable_dispatch::DurableDispatch {
            credential: model_key.clone(),
            launches: launches.clone(),
        }),
        Box::new(forge_server::GitInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(&db).unwrap()));
    assert!(store.worktree_claim(task.id).unwrap().is_none());
    let evidence = Evidence::Repository {
        repository: repository.display().to_string(),
        commit,
    };
    // Authentication preflight fails before worktree preparation, Assignment
    // creation, capability delivery or transport provisioning.
    std::fs::remove_file(&model_key).unwrap();
    assert!(app.dispatch_record(task.id).is_err());
    assert!(store.worktree_claim(task.id).unwrap().is_none());
    assert!(store.list_agent_assignments(task.id).unwrap().is_empty());
    assert!(launches.lock().unwrap().is_empty());
    assert!(!root.join("assignment-capabilities").exists());
    std::fs::write(&model_key, "T07-MODEL-KEY-CANARY").unwrap();
    std::fs::set_permissions(&model_key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let (assignment, existing) = app.dispatch_record(task.id).unwrap();
    assert!(!existing);
    let assignment_id = assignment.id.as_str();
    let checkout = store.worktree_claim(task.id).unwrap().unwrap();
    assert!(checkout.path.is_dir());
    assert_ne!(checkout.path, repository);
    assert_eq!(
        app.dispatch_record(task.id).unwrap(),
        (assignment.clone(), true)
    );
    let captured = {
        let mut launches = launches.lock().unwrap();
        assert_eq!(launches.len(), 1);
        launches.pop().unwrap()
    };
    assert_eq!(captured.context.assignment.id, assignment.id);
    assert_eq!(
        captured.context.agent_workspace,
        Some(checkout.path.clone())
    );
    assert_eq!(captured.profile.configuration.cwd, checkout.path);
    assert_eq!(
        store.assignment_dispatch_workspace(assignment_id).unwrap(),
        None
    );
    let attempt = store
        .assignment_check_attempt(assignment_id)
        .unwrap()
        .unwrap();
    assert_eq!(attempt.check_id, check.id);
    assert_eq!(attempt.evidence, evidence);
    assert_eq!(
        captured.context.assignment.profile.revision,
        profile.revision
    );
    let launch = Launch {
        assignment_id: assignment.id.clone(),
        operation_id: "t07-launch".into(),
        attempt_id: attempt.id.clone(),
        profile_revision: profile.revision,
        runtime_id: "t07-runtime".into(),
        incarnation_id: "t07-incarnation".into(),
        conversation_id: "t07-conversation".into(),
        store_id: "t07-store".into(),
        forge_revision: "controlled-component-test".into(),
        component_hash: component_hash().await,
        runtime_schema: forge_app::durable_execution::RUNTIME_SCHEMA_VERSION,
    };
    assert_eq!(
        store.reserve("test", &launch).unwrap().admission,
        Admission::Pending
    );
    assert!(store.begin_send("test", &launch).unwrap());
    let provisioned = &captured.context.assignment;
    let bound = BoundIdentity {
        assignment_id: provisioned.id.clone(),
        record_id: provisioned.record_id,
        profile_revision: provisioned.profile.revision,
        profile_record_type: "task".into(),
        provider: provisioned.profile.provider.clone(),
        agent_kind: provisioned.profile.agent_kind.clone(),
        model: provisioned.profile.model.clone(),
        attempt_number: provisioned.attempt_number,
        check_attempt: Some(attempt.clone()),
        execution_directory: captured.context.agent_workspace.clone().unwrap(),
        provider_workspace: None,
    };
    let secrets = root.join("assignment-capabilities");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&secrets)
        .unwrap();
    let secret = secrets.join(assignment_id);
    std::fs::write(&secret, "T07-CAPABILITY-CANARY").unwrap();
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let daemon = tokio::spawn(serve(DaemonConfig {
        bind_addr: addr,
        store_path: db.clone(),
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
        "FORGE_ASSIGNMENT_ID": assignment_id, "FORGE_ASSIGNMENT_DAEMON_URL": endpoint,
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
    assert!(!store.assignment_context_read(assignment_id).unwrap());
    bootstrap(input.clone()).await;
    assert!(store.assignment_context_read(assignment_id).unwrap());
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
    if mode.starts_with("effect-") {
        let authority = forge_server::durable_authorization::DaemonAuthority::new().unwrap();
        let runtime_directory = authority
            .prepare_directory(&store, "test", &launch)
            .unwrap();
        let effects = interrupted_effect_process(
            &runtime_directory,
            &launch,
            &checkout.path,
            &model_key,
            mode == "effect-before-commit",
        )
        .await;
        // The lost runtime admission reply deliberately remains Unknown. Runtime
        // observations must still be retained, never interpreted as safe absence.
        let inspector = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../runner/src/inspect-effects.ts")
            .canonicalize()
            .unwrap();
        let inspect = forge_server::durable_authorization::effects::inspect_and_retain;
        let database_before = std::fs::read(runtime_directory.join("runtime.sqlite")).unwrap();
        // Scope/binding rejection must precede spawning even an invalid binary.
        assert!(
            inspect(
                &store,
                "foreign",
                &launch,
                Path::new("/missing-node"),
                &inspector
            )
            .await
            .is_err()
        );
        let mut foreign_launch = launch.clone();
        foreign_launch.incarnation_id = "foreign-incarnation".into();
        assert!(
            inspect(
                &store,
                "test",
                &foreign_launch,
                Path::new("/missing-node"),
                &inspector
            )
            .await
            .is_err()
        );
        inspect(&store, "test", &launch, &node(), &inspector)
            .await
            .unwrap();
        let before = store.agent_assignment_info(assignment_id).unwrap();
        let inspect_exclusive =
            forge_server::durable_authorization::effects::inspect_exclusive_and_retain;
        // This component worker is not supervised. Missing lock evidence must
        // fail closed; install an explicit lock FIXTURE for adapter composition,
        // not a claim of verified process absence or production retirement.
        assert!(
            inspect_exclusive(&store, "test", &launch, &node(), &inspector)
                .await
                .is_err()
        );
        assert!(!runtime_directory.join("owner.lock").exists());
        std::fs::write(runtime_directory.join("owner.lock"), b"lock-fixture").unwrap();
        std::fs::set_permissions(
            runtime_directory.join("owner.lock"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let ownership = inspect_exclusive(&store, "test", &launch, &node(), &inspector)
            .await
            .unwrap();
        assert!(
            forge_runner_host::inspection::ExclusiveInspection::acquire(&runtime_directory)
                .is_err()
        );
        assert!(
            inspect_exclusive(&store, "test", &launch, &node(), &inspector)
                .await
                .is_err()
        );
        assert_eq!(store.agent_assignment_info(assignment_id).unwrap(), before);
        drop(ownership);
        assert_eq!(
            std::fs::read(runtime_directory.join("runtime.sqlite")).unwrap(),
            database_before
        );
        assert_eq!(
            before
                .resources
                .iter()
                .filter(|resource| resource.resource_kind == "interrupted-effect")
                .count(),
            effects.len()
        );
        assert!(before.capacity_reserved());
        assert!(
            before
                .recovery_blockers()
                .contains(&"interrupted-effect".into())
        );
        assert_eq!(
            store.assignment_check_attempt(assignment_id).unwrap(),
            Some(attempt.clone())
        );
        assert!(
            store.list_record_checks(task.id).unwrap()[0]
                .latest_assessment
                .is_none()
        );
        daemon.abort();
        let _ = daemon.await;
        let reopened = SqliteProjectRepository::open(&db).unwrap();
        let ownership = inspect_exclusive(&reopened, "test", &launch, &node(), &inspector)
            .await
            .unwrap();
        ownership.verify().unwrap();
        drop(ownership);
        assert!(
            inspect(
                &reopened,
                "test",
                &launch,
                Path::new("/missing-node"),
                &inspector
            )
            .await
            .is_err()
        );
        // An inspector failure must release the guard, but never the retained
        // effects, hold or capacity. The ordinary observation path above remains
        // available for non-exclusive operator investigation.
        assert!(
            inspect_exclusive(
                &reopened,
                "test",
                &launch,
                Path::new("/missing-node"),
                &inspector
            )
            .await
            .is_err()
        );
        drop(
            forge_runner_host::inspection::ExclusiveInspection::acquire(&runtime_directory)
                .unwrap(),
        );
        assert!(
            reopened
                .retain_interrupted_effects("foreign", &launch, &effects)
                .is_err()
        );
        let mut conflicting = effects.clone();
        conflicting[0].call_id = "conflicting-call".into();
        assert!(
            reopened
                .retain_interrupted_effects("test", &launch, &conflicting)
                .is_err()
        );
        assert_eq!(
            reopened.agent_assignment_info(assignment_id).unwrap(),
            before
        );
        let restarted = tokio::spawn(serve(DaemonConfig {
            bind_addr: addr,
            store_path: db.clone(),
            durable_model_key_file: None,
            managed: None,
        }));
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
        inspect_interrupted_effect(&endpoint, assignment_id).await;
        assert_eq!(
            reopened.agent_assignment_info(assignment_id).unwrap(),
            before
        );
        restarted.abort();
        let _ = restarted.await;
        return;
    }
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
    let valid = json!({"attempt_id": attempt.id, "generation": attempt.generation, "evidence": output_evidence, "verdict": "pass", "actor": "spoofed", "message": "Delivered", "disposition": "complete", "findings": [], "verifications": []});
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
        let rejected = outcome(assignment_id, &endpoint, &secret, &file).await;
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
    let unauthorized = outcome(assignment_id, &endpoint, &foreign, &file).await;
    assert!(!unauthorized.status.success());
    assert!(String::from_utf8_lossy(&unauthorized.stderr).contains("assignment-capability-denied"));
    // The first accepted submission is driven by a deterministic fake model
    // through the actual guarded Bash tool and CLI, not by this Rust process.
    // Application dispatch is real; transport capture and lease remain fixtures.
    let runtime = root.join("component-runtime");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&runtime)
        .unwrap();
    let mut execution = input.clone();
    execution["execution"] = json!({
        "key": model_key, "storage": runtime.join("runtime.sqlite"), "outcome": file,
        "launch": launch, "mode": mode, "configuration": captured.profile.configuration
    });
    if mode == "rejected" {
        let mut stale_outcome = valid.clone();
        stale_outcome["generation"] = json!(attempt.generation + 1);
        std::fs::write(&file, serde_json::to_vec(&stale_outcome).unwrap()).unwrap();
    }
    let report = bootstrap(execution).await;
    for entry in std::fs::read_dir(&runtime).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("T07-CAPABILITY-CANARY"));
        assert!(!text.contains("T07-MODEL-KEY-CANARY"));
    }
    let admission: Admission =
        serde_json::from_value(report["execution"]["admission"].clone()).unwrap();
    assert!(matches!(admission, Admission::Accepted { .. }));
    store.record_result("test", &launch, &admission).unwrap();
    assert_eq!(report["execution"]["answer"], "answered");
    let observation = store.observe_answer("test", &launch).unwrap();
    if mode != "accepted" {
        assert_eq!(observation, AnswerObservation::MissingOutcome);
        assert_eq!(store.observe_answer("test", &launch).unwrap(), observation);
        let info = store.agent_assignment_info(assignment_id).unwrap();
        assert!(
            info.recovery_blockers()
                .iter()
                .any(|blocker| blocker == "missing-outcome")
        );
        assert_eq!(
            info.resources
                .iter()
                .filter(|resource| resource.resource_kind == "missing-outcome")
                .count(),
            1
        );
        let current = &store.list_record_checks(task.id).unwrap()[0];
        assert!(current.latest_assessment.is_none());
        assert!(current.current_verdict.is_none());
        assert_eq!(
            store.show_record(task.id).unwrap().status,
            forge_app::RecordStatus::InProgress
        );
        assert_eq!(store.list_agent_assignments(task.id).unwrap().len(), 1);
        assert_eq!(
            store.assignment_check_attempt(assignment_id).unwrap(),
            Some(attempt)
        );
        assert!(store.runtime("test", assignment_id).unwrap().is_some());
        assert!(info.dispatch_hold.is_some());
        assert!(info.capacity_reserved());
        // Inspect the diagnostic produced by actual runtime prose, not a seeded
        // presentation fixture. Inspection cannot consume or clear retained state.
        for json_output in [false, true] {
            let endpoint = format!("{endpoint}/rpc");
            let assignment_id = assignment_id.to_owned();
            let output = tokio::task::spawn_blocking(move || {
                let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
                command.env_clear().args([
                    "dispatcher",
                    "assignment-info",
                    &assignment_id,
                    "--server-url",
                    &endpoint,
                ]);
                if json_output {
                    command.arg("--json");
                }
                command.output().unwrap()
            })
            .await
            .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(text.contains("missing-outcome"));
            assert!(!text.contains("T07-CAPABILITY-CANARY"));
            assert!(!text.contains("T07-BOUND-BEHAVIOR"));
            if json_output {
                let response: forge_rpc::AgentAssignmentInfoResponse =
                    serde_json::from_str(&text).unwrap();
                assert!(response.capacity_reserved);
                assert!(
                    response
                        .recovery_blockers
                        .contains(&"missing-outcome".into())
                );
            }
        }
        assert_eq!(store.agent_assignment_info(assignment_id).unwrap(), info);
        // Closure and a new repository connection do not erase uncertainty or
        // release capacity. This is test-controlled closure, not safe retirement.
        store
            .close_agent_assignment(
                assignment_id,
                forge_app::AgentAssignmentClosureReason::Retired,
            )
            .unwrap();
        let reopened = SqliteProjectRepository::open(&db).unwrap();
        assert_eq!(
            reopened.observe_answer("test", &launch).unwrap(),
            observation
        );
        let retained = reopened.agent_assignment_info(assignment_id).unwrap();
        assert!(retained.capacity_reserved());
        assert!(
            retained
                .recovery_blockers()
                .contains(&"missing-outcome".into())
        );
        assert_eq!(retained.resources, info.resources);
        daemon.abort();
        let _ = daemon.await;
        return;
    }
    assert_eq!(observation, AnswerObservation::OutcomeAccepted);
    assert_eq!(
        store.list_record_checks(task.id).unwrap()[0].current_verdict,
        Some(forge_app::checks::Verdict::Pass)
    );
    // Exact replay reads back the model tool's accepted result for assertions.
    let accepted = outcome(assignment_id, &endpoint, &secret, &file).await;
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
    assert_eq!(assessment.actor, assignment_id);
    assert_eq!(assessment.assignment_id.as_deref(), Some(assignment_id));
    assert_eq!(assessment.evidence, output_evidence);
    assert_eq!(assessment.generation, attempt.generation);
    assert!(!String::from_utf8_lossy(&accepted.stdout).contains("T07-CAPABILITY-CANARY"));
    let replay = outcome(assignment_id, &endpoint, &secret, &file).await;
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
        store.assignment_check_attempt(assignment_id).unwrap(),
        Some(attempt)
    );
    store
        .close_agent_assignment(
            assignment_id,
            forge_app::AgentAssignmentClosureReason::Retired,
        )
        .unwrap();
    let mut revoked = input;
    revoked["expected"] = "denied".into();
    bootstrap(revoked).await;
    // Exact accepted replay survives closure; closure cannot rewrite that result.
    let closed_replay = outcome(assignment_id, &endpoint, &secret, &file).await;
    assert!(closed_replay.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&accepted.stdout).unwrap(),
        serde_json::from_slice::<Value>(&closed_replay.stdout).unwrap()
    );
    let mut conflicting = valid;
    conflicting["message"] = "Different submission after revocation".into();
    std::fs::write(&file, serde_json::to_vec(&conflicting).unwrap()).unwrap();
    let rejected = outcome(assignment_id, &endpoint, &secret, &file).await;
    assert!(!rejected.status.success());
    daemon.abort();
    let _ = daemon.await;
}
