//! Deterministic T07 app-dispatch -> protected delivery -> actual Linux host /
//! shared flock -> daemon grant driver -> Node bootstrap/Harness/Bash/CLI tracer.
//! The custom harness re-execs the same supervisor library as the shipped binary;
//! it needs no arbitrary PATH host, sibling artifact or nested Cargo build.
#[path = "support/supervised_chain.rs"]
mod supervised_chain;
#[path = "support/supervised_dispatch.rs"]
mod supervised_dispatch;
#[path = "support/supervised_purge.rs"]
mod supervised_purge;
#[path = "support/supervised_rollback.rs"]
mod supervised_rollback;
use forge_app::{
    AgentDispatchRepository, ExecutionMode, ManagedWorktreeRepository, NewTypedRecord, Project,
    ProjectRepository, RecordRepository, RecordStatus, RecordType, WorkspaceDispatchRepository,
    WorkspaceRepository,
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
    durable_execution::{Admission, Repository as _, completion::AnswerObservation},
};
use forge_server::{Daemon, durable_authorization::DaemonAuthority, router};
use forge_store::SqliteProjectRepository;
use jsonrpsee::{
    core::{client::ClientT, params::ArrayParams},
    http_client::HttpClientBuilder,
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use supervised_dispatch::SupervisedDispatch;

fn executable(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join(name))
        .find(|path| path.is_file())
        .unwrap()
        .canonicalize()
        .unwrap()
}
fn git(program: &Path, cwd: &Path, args: &[&str]) -> String {
    let output = Command::new(program)
        .env_clear()
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "controlled Git operation failed");
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn task(store: &SqliteProjectRepository, body: &str) -> u64 {
    store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: body.into(),
            project_slugs: vec!["test/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap()
        .id
}
fn component_hash(node: &Path, runner: &Path) -> String {
    let module = runner.join("src/contract.ts");
    let output = Command::new(node)
        .env_clear()
        .args(["--input-type=module", "-e"])
        .arg(format!(
            "import {{componentHash}} from {}; process.stdout.write(componentHash());",
            json!(module)
        ))
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

fn gates(adapter: &SupervisedDispatch, store: &SqliteProjectRepository, base: &str) {
    let app = adapter.app();
    let manual = task(store, "Manual");
    store
        .set_record_execution_mode(
            manual,
            ExecutionMode::Dispatched,
            ExecutionMode::Manual,
            "operator",
            "Manual",
        )
        .unwrap();
    let blocked = task(store, "Dependency blocked");
    let prerequisite = task(store, "Prerequisite");
    app.relate_records(
        blocked,
        prerequisite,
        forge_app::RecordRelationshipKind::WaitsFor,
    )
    .unwrap();
    let spec = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Spec,
            body: "Open plan".into(),
            project_slugs: vec!["test/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let child = store
        .open_task_under_spec("Unauthorized child", spec.id, vec![])
        .unwrap()
        .id;
    for (record, code) in [
        (manual, "manual-dispatch-forbidden"),
        (blocked, "record-not-ready"),
        (child, "spec-execution-blocked"),
    ] {
        store.set_task_base_commit(record, base).unwrap();
        assert_eq!(app.dispatch_record(record).unwrap_err().code, code);
        assert!(store.list_agent_assignments(record).unwrap().is_empty());
        assert!(store.worktree_claim(record).unwrap().is_none());
    }
    assert_eq!(adapter.provisions.load(Ordering::SeqCst), 0);
    assert!(!adapter.root.join("assignment-capabilities").exists());
}

fn outcome(adapter: &SupervisedDispatch, assignment: &str, secret: &Path, file: &Path) -> Output {
    Command::new(&adapter.cli)
        .env_clear()
        .env("FORGE_ASSIGNMENT_ID", assignment)
        .env("FORGE_ASSIGNMENT_DAEMON_URL", &adapter.endpoint)
        .env("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE", secret)
        .args(["assignment", "outcome", "--file"])
        .arg(file)
        .arg("--json")
        .output()
        .unwrap()
}
fn redacted_tree(path: &Path, capability: &str) {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            redacted_tree(&path, capability);
        } else if path.is_file() {
            let bytes = fs::read(&path).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                !text.contains(capability),
                "capability leaked into runtime storage"
            );
            assert!(
                !text.contains("T07-SUPERVISED-MODEL-KEY"),
                "model key leaked into runtime storage"
            );
        }
    }
}

// Commit the actual authenticated bootstrap, then withhold its reply. The worker
// sees transport failure, never trusted behavior, despite durable server effects.
async fn withhold_bootstrap(
    axum::extract::State((endpoint, calls)): axum::extract::State<(String, Arc<AtomicUsize>)>,
    body: axum::body::Bytes,
) -> axum::http::StatusCode {
    let request: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(request["method"], "assignment_bootstrap");
    let mut params = ArrayParams::new();
    for value in request["params"].as_array().unwrap() {
        params.insert(value).unwrap();
    }
    let client = HttpClientBuilder::default()
        .build(format!("{endpoint}/rpc"))
        .unwrap();
    let _: Value = client
        .request("assignment_bootstrap", params)
        .await
        .unwrap();
    calls.fetch_add(1, Ordering::SeqCst);
    axum::http::StatusCode::SERVICE_UNAVAILABLE
}

#[allow(clippy::too_many_lines)] // Whole isolated end-to-end scenario and retained-state assertions.
async fn scenario(mode: &str) {
    let fixture = tempfile::Builder::new()
        .prefix("forge-t07-supervised-")
        .tempdir()
        .unwrap();
    let root = fixture.path().canonicalize().unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    // Deliberate protected host setup: a group-writable ambient umask must not
    // turn runtime mapping into an implicit chmod/adoption operation.
    fs::create_dir(root.join(".forge")).unwrap();
    fs::set_permissions(root.join(".forge"), fs::Permissions::from_mode(0o700)).unwrap();
    let node = executable("node");
    let git_program = executable("git");
    let runner = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../runner")
        .canonicalize()
        .unwrap();
    let repository = root.join("repository");
    fs::create_dir(&repository).unwrap();
    git(&git_program, &repository, &["init", "-q"]);
    git(
        &git_program,
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
    let base = git(&git_program, &repository, &["rev-parse", "HEAD"]);
    let database = root.join("forge.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("test", "Test", &root).unwrap();
    store
        .register(&Project {
            reference: "test/api".into(),
            workspace_id: "test".into(),
            slug: "api".into(),
            project_root: repository,
            origin_url: "https://example.test/api.git".into(),
        })
        .unwrap();
    fs::create_dir_all(root.join(".agents/profiles")).unwrap();
    fs::write(root.join(".agents/profiles/worker.md"), "---\nversion: 1\nruntime: pi-durable\nmodel: openai/gpt-4.1\nthinkingLevel: off\ntools: [bash]\n---\nT07-SUPERVISED-BEHAVIOR").unwrap();
    let policy = ResourceCatalog::discover(&root.join("operator"), &root)
        .unwrap()
        .resolve(&root, ".agents/profiles/worker.md")
        .unwrap();
    let profile = store
        .register_agent_profile("test", RecordType::Task, &policy)
        .unwrap();
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
                criteria: if mode == "continuation-chain" {
                    vec![
                        CheckCriterion {
                            name: "Implementation".into(),
                            description: "Implement".into(),
                            profile_revision: Some(profile.revision),
                        },
                        CheckCriterion {
                            name: "Review".into(),
                            description: "Independently review the exact output".into(),
                            profile_revision: Some(profile.revision),
                        },
                    ]
                } else {
                    vec![CheckCriterion {
                        name: "Implementation".into(),
                        description: "Implement".into(),
                        profile_revision: Some(profile.revision),
                    }]
                },
            },
        )
        .unwrap();
    let model_key = root.join("model-key");
    fs::write(&model_key, "T07-SUPERVISED-MODEL-KEY").unwrap();
    fs::set_permissions(&model_key, fs::Permissions::from_mode(0o600)).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let proxy_calls = Arc::new(AtomicUsize::new(0));
    let proxy = if mode == "bootstrap-uncertain" {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = axum::Router::new()
            .route("/rpc", axum::routing::post(withhold_bootstrap))
            .with_state((format!("http://{addr}"), proxy_calls.clone()));
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Some((format!("http://{address}"), handle))
    } else {
        None
    };
    let adapter = SupervisedDispatch {
        root: root.clone(),
        database: database.clone(),
        component_hash: component_hash(&node, &runner),
        node: node.clone(),
        git: git_program.clone(),
        runner: runner.join("test/supervised-check-worker.mjs"),
        cli: Path::new(env!("CARGO_BIN_EXE_forge"))
            .canonicalize()
            .unwrap(),
        endpoint: format!("http://{addr}"),
        bootstrap_endpoint: proxy.as_ref().map(|(endpoint, _)| endpoint.clone()),
        credential: model_key.clone(),
        authority: Arc::new(
            DaemonAuthority::new()
                .unwrap()
                .with_inspection(
                    forge_server::durable_authorization::operator::InspectionComponents {
                        node: node.clone(),
                        inspector: runner.join("src/inspect-effects.ts"),
                    },
                )
                .unwrap(),
        ),
        mode: mode.into(),
        provisions: Arc::new(AtomicUsize::new(0)),
        reports: Arc::new(Mutex::new(vec![])),
        retirement: Arc::new(
            forge_server::durable_authorization::retirement::Controller::default()
                .with_inspection(
                    forge_server::durable_authorization::operator::InspectionComponents {
                        node: node.clone(),
                        inspector: runner.join("src/inspect-effects.ts"),
                    },
                )
                .unwrap(),
        ),
        launch: {
            let launch =
                forge_server::durable_authorization::release_gate::LaunchControl::default();
            assert!(launch.enable_controlled());
            launch
        },
    };
    // Embed the actual server with the explicitly injected controlled adapter.
    // Only this test drives scheduling: a second default Herdr reconciler must
    // not concurrently administer the controlled durable Assignment.
    let server = Daemon::new(adapter.app())
        .with_assignment_capability_root(root.join("assignment-capabilities"))
        .with_durable_operator_store(&database)
        .unwrap();
    let daemon = tokio::spawn(async move {
        axum::serve(
            listener,
            router(server).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    gates(&adapter, &store, &base);
    let record = task(&store, "Supervised Check");
    store.set_task_base_commit(record, &base).unwrap();
    fs::remove_file(&model_key).unwrap();
    assert!(adapter.app().dispatch_record(record).is_err());
    assert!(store.list_agent_assignments(record).unwrap().is_empty());
    assert!(store.worktree_claim(record).unwrap().is_none());
    assert!(!root.join("assignment-capabilities").exists());
    fs::write(&model_key, "T07-SUPERVISED-MODEL-KEY").unwrap();
    fs::set_permissions(&model_key, fs::Permissions::from_mode(0o600)).unwrap();
    let dispatch = adapter.clone();
    let (assignment, existing) =
        tokio::task::spawn_blocking(move || dispatch.app().dispatch_record(record).unwrap())
            .await
            .unwrap();
    assert!(!existing);
    assert_eq!(adapter.provisions.load(Ordering::SeqCst), 1);
    let mut report = adapter.reports.lock().unwrap().pop().unwrap();
    assert_eq!(report.launch.assignment_id, assignment.id);
    assert_eq!(assignment.profile.revision, profile.revision);
    let attempt = store
        .assignment_check_attempt(&assignment.id)
        .unwrap()
        .unwrap();
    assert_eq!(attempt.id, report.launch.attempt_id);
    assert!(
        matches!(&attempt.evidence, forge_app::outcomes::Evidence::Repository { commit, .. } if commit == &base)
    );
    let capability = fs::read_to_string(&report.capability_file).unwrap();
    assert_eq!(
        fs::metadata(&report.capability_file)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    redacted_tree(&report.directory, &capability);
    let check = &store.list_record_checks(record).unwrap()[0];
    let runtime = store.runtime("test", &assignment.id).unwrap().unwrap();
    let info = store.agent_assignment_info(&assignment.id).unwrap();
    assert!(info.capacity_reserved());
    assert_eq!(store.list_agent_assignments(record).unwrap().len(), 1);
    if mode == "continuation-chain" {
        supervised_chain::run(&adapter, &store, record, report).await;
        daemon.abort();
        let _ = daemon.await;
        println!("T10 supervised continuation/review chain: passed");
        return;
    }
    if matches!(mode, "accepted" | "retired" | "recovered" | "rollback") {
        assert_eq!(report.answer, Some(AnswerObservation::OutcomeAccepted));
        assert_eq!(
            store.show_record(record).unwrap().status,
            RecordStatus::InProgress
        );
        let assessment = check.latest_assessment.as_ref().unwrap();
        assert_eq!(
            assessment.assignment_id.as_deref(),
            Some(assignment.id.as_str())
        );
        assert_eq!(assessment.actor, assignment.id);
        assert_ne!(assessment.evidence, attempt.evidence);
        assert_eq!(assessment.generation, attempt.generation);
        let file = report.directory.join("controlled-outcome.json");
        let replay = outcome(&adapter, &assignment.id, &report.capability_file, &file);
        assert!(replay.status.success());
        let accepted: forge_app::outcomes::AcceptedOutcome =
            serde_json::from_slice(&replay.stdout).unwrap();
        assert_eq!(accepted.attempt, attempt);
        assert_eq!(accepted.evidence, assessment.evidence);
        assert!(!String::from_utf8_lossy(&replay.stdout).contains(&capability));
    } else {
        assert!(check.latest_assessment.is_none());
        assert!(check.current_verdict.is_none());
        if matches!(mode, "missing" | "rejected") {
            assert_eq!(report.answer, Some(AnswerObservation::MissingOutcome));
            assert!(info.recovery_blockers().contains(&"missing-outcome".into()));
            for json_output in [false, true] {
                let mut command = Command::new(&adapter.cli);
                command.env_clear().args([
                    "dispatcher",
                    "assignment-info",
                    &assignment.id,
                    "--server-url",
                    &format!("{}/rpc", adapter.endpoint),
                ]);
                if json_output {
                    command.arg("--json");
                }
                let output = command.output().unwrap();
                assert!(output.status.success());
                let text = String::from_utf8(output.stdout).unwrap();
                assert!(text.contains("missing-outcome"));
                assert!(!text.contains(&capability));
                assert!(!text.contains("T07-SUPERVISED-BEHAVIOR"));
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
                assert_eq!(store.agent_assignment_info(&assignment.id).unwrap(), info);
            }
            assert_eq!(
                store.show_record(record).unwrap().status,
                RecordStatus::InProgress
            );
        } else {
            assert!(report.answer.is_none());
            assert_eq!(runtime.admission, Admission::Unknown);
            assert!(!report.directory.join("runtime.sqlite").exists());
            assert_eq!(
                store.assignment_context_read(&assignment.id).unwrap(),
                mode == "bootstrap-uncertain"
            );
            assert_eq!(
                proxy_calls.load(Ordering::SeqCst),
                usize::from(mode == "bootstrap-uncertain")
            );
            assert!(
                !report
                    .events
                    .iter()
                    .any(|event| event["event"] == "bootstrapped")
            );
        }
    }
    if matches!(
        mode,
        "accepted" | "rejected" | "retired" | "recovered" | "rollback"
    ) {
        assert_eq!(
            fs::read_to_string(report.directory.join("controlled-effects")).unwrap(),
            "effect\n"
        );
    } else {
        assert!(!report.directory.join("controlled-effects").exists());
    }
    let replacement = task(&store, "Capacity must remain reserved");
    store.set_task_base_commit(replacement, &base).unwrap();
    assert!(adapter.app().dispatch_record(replacement).is_err());
    assert!(
        store
            .list_agent_assignments(replacement)
            .unwrap()
            .is_empty()
    );
    assert_eq!(adapter.provisions.load(Ordering::SeqCst), 1);
    if mode == "rollback" {
        supervised_rollback::disable(&adapter, record, &assignment.id, &capability, &profile);
    }
    if matches!(mode, "retired" | "rollback") {
        // Actual CLI/RPC -> application revocation -> registered original host.
        // Historical cleanup must survive missing source/authentication files.
        fs::remove_file(&model_key).unwrap();
        fs::remove_file(root.join(".agents/profiles/worker.md")).unwrap();
        let claim = store.worktree_claim(record).unwrap().unwrap();
        let head = git(&git_program, &claim.path, &["rev-parse", "HEAD"]);
        let status = git(
            &git_program,
            &claim.path,
            &["--no-optional-locks", "status", "--porcelain=v2"],
        );
        let checks = store.list_record_checks(record).unwrap();
        let retire = |success: bool| {
            let output = Command::new(&adapter.cli)
                .env_clear()
                .args([
                    "dispatcher",
                    "assignment-retire",
                    &assignment.id,
                    "--json",
                    "--server-url",
                    &format!("{}/rpc", adapter.endpoint),
                ])
                .output()
                .unwrap();
            assert_eq!(
                output.status.success(),
                success,
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!String::from_utf8_lossy(&output.stdout).contains(&capability));
            if !success {
                assert!(
                    String::from_utf8_lossy(&output.stderr)
                        .contains("assignment-retirement-unreconciled"),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        };
        // The pre-launch provision-intent remains a separate unknown-resource
        // confirmation, even after the owned process receipt is committed.
        retire(false);
        let pending = store.agent_assignment_info(&assignment.id).unwrap();
        assert!(pending.assignment.closed_at_millis.is_some());
        assert!(pending.capacity_reserved());
        assert!(
            pending
                .resources
                .iter()
                .any(|r| r.resource_kind == "provision-intent" && r.retired_at_millis.is_none())
        );
        assert!(adapter.app().dispatch_record(replacement).is_err());
        assert!(
            store
                .list_agent_assignments(replacement)
                .unwrap()
                .is_empty()
        );
        let resolve = Command::new(&adapter.cli)
            .env_clear()
            .args([
                "dispatcher",
                "assignment-resolve-unknown",
                &assignment.id,
                "--confirm-old-agent-cannot-act",
                "--confirm-unknown-resources-safe",
                "--json",
                "--server-url",
                &format!("{}/rpc", adapter.endpoint),
            ])
            .output()
            .unwrap();
        assert!(
            resolve.status.success(),
            "{}",
            String::from_utf8_lossy(&resolve.stderr)
        );
        assert!(
            store
                .agent_assignment_info(&assignment.id)
                .unwrap()
                .capacity_reserved()
        );
        retire(true);
        let retired = store.agent_assignment_info(&assignment.id).unwrap();
        assert!(retired.assignment.closed_at_millis.is_some());
        assert!(!retired.capacity_reserved());
        assert!(retired.dispatch_hold.is_some());
        retire(true);
        assert_eq!(
            store.agent_assignment_info(&assignment.id).unwrap(),
            retired
        );
        assert_eq!(store.list_record_checks(record).unwrap(), checks);
        assert_eq!(git(&git_program, &claim.path, &["rev-parse", "HEAD"]), head);
        assert_eq!(
            git(
                &git_program,
                &claim.path,
                &["--no-optional-locks", "status", "--porcelain=v2"]
            ),
            status
        );
        assert_eq!(
            store.runtime("test", &assignment.id).unwrap().unwrap(),
            runtime
        );
        assert_eq!(
            store.assignment_check_attempt(&assignment.id).unwrap(),
            Some(attempt)
        );
        assert_eq!(adapter.provisions.load(Ordering::SeqCst), 1);
        report.finish_retired(&capability);
        if mode == "retired" {
            supervised_purge::run(
                &adapter,
                &store,
                &assignment.id,
                record,
                &report.directory,
                &claim.path,
            );
        }
        if mode == "rollback" {
            supervised_rollback::refused_after_retirement(&adapter, &store, replacement);
        }
    } else {
        // Explicit fixture closure is not retirement proof. Reopening retains all
        // resources/capacity/diagnostics and never rewrites the accepted admission.
        if assignment.closed_at_millis.is_none() {
            store
                .close_agent_assignment(
                    &assignment.id,
                    forge_app::AgentAssignmentClosureReason::Retired,
                )
                .unwrap();
        }
        let reopened = SqliteProjectRepository::open(&database).unwrap();
        let retained = reopened.agent_assignment_info(&assignment.id).unwrap();
        assert!(retained.capacity_reserved());
        assert_eq!(retained.resources, info.resources);
        assert_eq!(
            reopened.runtime("test", &assignment.id).unwrap().unwrap(),
            runtime
        );
        assert_eq!(
            reopened.assignment_check_attempt(&assignment.id).unwrap(),
            Some(attempt)
        );
    }
    if let Some((_, proxy)) = proxy {
        proxy.abort();
        let _ = proxy.await;
    }
    daemon.abort();
    let _ = daemon.await;
    println!("T07 supervised {mode}: passed");
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--supervisor") {
        assert_eq!(args.len(), 5);
        if let Err(error) = forge_runner_host::supervisor::run(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
            Path::new(&args[4]),
        ) {
            eprintln!("controlled-host: {error}");
            std::process::exit(1);
        }
    } else if args.iter().any(|arg| arg == "--list") {
        println!("T07/T09/T10 supervised tracer (ten parallel controlled scenarios)");
    } else {
        // Parallel isolated scenarios are intentional, not a serial-test waiver.
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(8)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let modes = [
                    "accepted",
                    "retired",
                    "recovered",
                    "continuation-chain",
                    "missing",
                    "rejected",
                    "bootstrap-denied",
                    "bootstrap-uncertain",
                    "revoked",
                    "rollback",
                ];
                let handles = modes.map(|mode| tokio::spawn(scenario(mode)));
                let mut failures = vec![];
                for handle in handles {
                    if let Err(error) = handle.await {
                        failures.push(error);
                    }
                }
                assert!(
                    failures.is_empty(),
                    "supervised tracer scenarios failed: {failures:?}"
                );
            });
        println!("T07/T09/T10 supervised tracer: 10 scenarios passed");
    }
}
