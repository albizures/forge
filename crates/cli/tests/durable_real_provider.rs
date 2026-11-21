//! T19 / Spec AC22 real-provider release gate (test class c). OPT-IN ONLY.
//!
//! Without `FORGE_REAL_PROVIDER_KEY_FILE` this binary prints a skip line and
//! succeeds, so ordinary `cargo test` never contacts a provider. With it, one
//! isolated Implementation -> independent Review cycle runs against the real
//! `openai/gpt-4.1` provider and makes billed model calls.
//!
//! Composition: the production durable composition
//! (`forge_server::durable_authorization::launch::compose`) behind the production
//! release gate, the real `forge-runner-host` binary that this Cargo invocation
//! built, the SHIPPED runner entry (`runner/src/check-entry.ts`, no faux model),
//! the real Forge CLI and daemon RPC, a scratch Workspace, store and Git
//! repository. The ONLY test-only substitution is controlled launch eligibility
//! (`LaunchControl::enable_controlled`): every shipped Revision withholds new
//! durable launches. No Herdr fallback exists (any call panics).
//!
//! The key file is passed to Forge by path only. This harness never reads it.
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchRepository,
    AgentObservation, AgentProfile, Application, NewTypedRecord, Project, ProjectRepository,
    RecordRepository, RecordType, WorkspaceDispatchRepository, WorkspaceRepository,
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
    checks::Verdict,
    durable_execution::{
        Repository as _, completion::AnswerObservation, completion::Repository as _,
    },
    outcomes::Evidence,
};
use forge_server::{
    Daemon,
    durable_authorization::{
        launch::{DriverState, Execution, current_forge_revision},
        recovery,
        release_gate::{LaunchControl, ReleaseGate},
        retirement,
    },
    router,
};
use forge_store::SqliteProjectRepository;
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const TASK: &str = "Add a file named `hello.txt` at the repository root containing exactly one line: `Hello from Forge` (followed by a newline). Commit it. Change nothing else.";

const BEHAVIOR: &str = r#"You are an unattended Forge Task worker. No human will answer questions. Do the work with your tools, then submit exactly one Outcome.

Your trusted Assignment context (JSON, below) contains `bound.checkAttempt` with `id`, `generation`, `check_id` and `evidence`, plus the Task `record` body.

1. Run `forge assignment check list --json` and find the Check whose id equals `bound.checkAttempt.check_id`. Its name is either `Implementation` or `Review`.
2. Work only in the current directory (a Git worktree of the bound repository).
   - Implementation: make exactly the change the Task body asks for, `git add` it and `git commit -m "<summary>"`. Your evidence commit is the new `git rev-parse HEAD`. Verdict `pass` once the change is committed.
   - Review: you are an independent reviewer. Do NOT modify, add or commit anything. Inspect the commit in `bound.checkAttempt.evidence.commit` (for example `git show --stat <commit>`, `git show <commit>:hello.txt`, `git log`) and judge it strictly against the Task body. Verdict `pass` only if it fully meets the Task and changes nothing else, otherwise `fail`. Your evidence must be exactly `bound.checkAttempt.evidence`, unchanged.
3. Write the Outcome as JSON to a new file outside the worktree, created with `mktemp`. Its shape is exactly:
   {"attempt_id": "<bound.checkAttempt.id>", "generation": <bound.checkAttempt.generation as a number>, "evidence": <bound.checkAttempt.evidence with only "commit" replaced by the full 40-hex evidence commit>, "verdict": "pass" or "fail", "actor": "worker", "message": "<one sentence>", "disposition": "complete", "findings": [], "verifications": []}
4. Run `forge assignment outcome --file <that file> --json`. If it reports an error, read it, correct the file and retry. When it succeeds, reply with one short sentence and stop."#;

/// Original-backend stand-in: any non-durable or fallback call fails the run.
struct NoHerdr;
impl AgentDispatchAdapterFactory for NoHerdr {
    fn validate_profile(&self, _: &AgentProfile) -> Result<(), AgentAdapterError> {
        panic!("no Herdr fallback")
    }
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        panic!("no Herdr fallback")
    }
}

/// The one production composition, shared by the daemon and this harness.
#[derive(Clone)]
struct Shared(Arc<retirement::Factory>);
impl AgentDispatchAdapterFactory for Shared {
    fn validate_profile(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        self.0.validate_profile(profile)
    }
    fn validate_preflight(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        self.0.validate_preflight(profile)
    }
    fn adapter_for(
        &self,
        profile: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        self.0.adapter_for(profile)
    }
}

struct Gate {
    root: PathBuf,
    database: PathBuf,
    endpoint: String,
    cli: PathBuf,
    git: PathBuf,
    production: Shared,
    launch: LaunchControl,
}

impl Gate {
    fn app(&self) -> Application {
        let open = || Box::new(SqliteProjectRepository::open(&self.database).unwrap());
        Application::with_managed_worktrees_and_agent_dispatch(
            open(),
            open(),
            Box::new(forge_server::GitWorktreeAdapter),
            open(),
            forge_server::assignment_delivery::adapter_factory(
                Box::new(ReleaseGate {
                    control: self.launch.clone(),
                    rollback_store: Some(self.database.clone()),
                    inner: Box::new(self.production.clone()),
                }),
                self.endpoint.clone(),
                self.root.join("assignment-capabilities"),
                self.cli.clone(),
            ),
            Box::new(forge_server::GitInspector),
        )
        .with_workspace_repository(open())
    }

    fn state(&self, assignment: &str) -> Option<DriverState> {
        self.production
            .0
            .launcher
            .as_ref()
            .unwrap()
            .state(assignment)
    }

    /// Real model turns take minutes, not the deterministic tracer's seconds.
    fn settle(&self, assignment: &str) -> DriverState {
        let deadline = Instant::now() + Duration::from_mins(15);
        let mut last = None;
        loop {
            let state = self.state(assignment);
            if state != last {
                log(&format!("assignment {assignment}: driver state {state:?}"));
                last = state;
            }
            if let Some(state) = state
                && !matches!(
                    state,
                    DriverState::Live(AgentObservation::Working | AgentObservation::Unknown)
                )
            {
                return state;
            }
            assert!(Instant::now() < deadline, "real-provider turn timed out");
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    fn cli(&self, args: &[&str]) -> (bool, String, String) {
        let output = Command::new(&self.cli)
            .env_clear()
            .args(args)
            .args(["--server-url", &format!("{}/rpc", self.endpoint)])
            .output()
            .unwrap();
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    fn retire(&self, assignment: &str) {
        let (ok, _, stderr) = self.cli(&["dispatcher", "assignment-retire", assignment, "--json"]);
        assert!(ok, "retirement failed: {stderr}");
    }

    fn transcript(&self, assignment: &str, name: &str) {
        let (ok, stdout, stderr) = self.cli(&[
            "dispatcher",
            "assignment-transcript",
            assignment,
            "--workspace",
            "test",
        ]);
        let path = self.root.join(format!("evidence-{name}-transcript.txt"));
        fs::write(
            &path,
            if ok {
                stdout
            } else {
                format!("transcript unavailable: {stderr}")
            },
        )
        .unwrap();
        log(&format!("{name} transcript saved to {}", path.display()));
    }

    fn git(&self, cwd: &Path, args: &[&str]) -> String {
        let output = Command::new(&self.git)
            .env_clear()
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?} failed");
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
}

fn log(line: &str) {
    println!("[real-provider-gate] {line}");
}

fn now_millis() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

fn executable(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join(name))
        .find(|path| path.is_file())
        .unwrap()
        .canonicalize()
        .unwrap()
}

/// The real `forge-runner-host` built next to this test binary, never `PATH`,
/// and refused when it reports another Revision.
fn host_binary() -> PathBuf {
    let host = std::env::current_exe()
        .unwrap()
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("forge-runner-host");
    let output = Command::new(&host)
        .env_clear()
        .arg("--compatibility")
        .output()
        .expect("build forge-runner-host first");
    let reported: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        reported["forge_revision"],
        current_forge_revision(),
        "forge-runner-host is stale"
    );
    host.canonicalize().unwrap()
}

fn component_hash(node: &Path, sources: &Path) -> String {
    forge_server::durable_authorization::launch::attest_component_hash(node, sources).unwrap()
}

/// Heuristic leak scan that never needs the key bytes: no OpenAI-style secret
/// token appears in retained runtime storage or the repository.
fn assert_no_secrets(path: &Path) {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            assert_no_secrets(&path);
        } else if path.is_file() {
            let text = String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned();
            let leaked = text.match_indices("sk-").any(|(at, _)| {
                text[at + 3..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                    .count()
                    >= 20
            });
            assert!(
                !leaked,
                "an API-key-shaped token appears in {}",
                path.display()
            );
        }
    }
}

fn commit_of(evidence: &Evidence) -> String {
    match evidence {
        Evidence::Repository { commit, .. } => commit.clone(),
        Evidence::Artifact { .. } => panic!("unexpected artifact evidence"),
    }
}

#[allow(clippy::too_many_lines)] // One ordered, isolated release-gate journey.
async fn run(key_file: PathBuf) {
    let started = Instant::now();
    let root = tempfile::Builder::new()
        .prefix("forge-real-provider-")
        .tempdir()
        .unwrap()
        .keep()
        .canonicalize()
        .unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir(root.join(".forge")).unwrap();
    fs::set_permissions(root.join(".forge"), fs::Permissions::from_mode(0o700)).unwrap();
    log(&format!("scratch workspace {}", root.display()));
    let node = executable("node");
    let git = executable("git");
    let runner = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../runner")
        .canonicalize()
        .unwrap();
    let sources = runner.join("src");
    let repository = root.join("repository");
    fs::create_dir(&repository).unwrap();
    let run_git = |args: &[&str]| {
        let output = Command::new(&git)
            .env_clear()
            .current_dir(&repository)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    run_git(&["init", "-q", "-b", "main"]);
    run_git(&["config", "user.name", "Forge Gate Agent"]);
    run_git(&["config", "user.email", "agent@example.invalid"]);
    fs::write(
        repository.join("README.md"),
        "# Scratch repository for the Forge real-provider gate\n",
    )
    .unwrap();
    run_git(&["add", "README.md"]);
    run_git(&["commit", "-qm", "Initial commit"]);
    let base = run_git(&["rev-parse", "HEAD"]);

    let database = root.join("forge.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    store
        .create_workspace("test", "Real-provider gate", &root)
        .unwrap();
    store
        .register(&Project {
            reference: "test/api".into(),
            workspace_id: "test".into(),
            slug: "api".into(),
            project_root: repository.clone(),
            origin_url: "https://example.invalid/api.git".into(),
        })
        .unwrap();
    fs::create_dir_all(root.join(".agents/profiles")).unwrap();
    fs::write(
        root.join(".agents/profiles/worker.md"),
        format!("---\nversion: 1\nruntime: pi-durable\nmodel: openai/gpt-4.1\nthinkingLevel: off\ntools: [read, write, edit, bash]\n---\n{BEHAVIOR}"),
    )
    .unwrap();
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
                message: "Pin the durable profile".into(),
                criteria: vec![
                    CheckCriterion {
                        name: "Implementation".into(),
                        description: "Implement the Task and commit the result".into(),
                        profile_revision: Some(profile.revision),
                    },
                    CheckCriterion {
                        name: "Review".into(),
                        description: "Independently review the exact Implementation output".into(),
                        profile_revision: Some(profile.revision),
                    },
                ],
            },
        )
        .unwrap();

    let execution = Execution {
        components: recovery::Components {
            host: host_binary(),
            host_arguments: vec![],
            node: node.clone(),
            runner: sources.join("check-entry.ts"),
            inspector: sources.join("inspect-recovery.ts"),
            answer_inspector: sources.join("inspect-answer.ts"),
        },
        effect_inspector: sources.join("inspect-effects.ts"),
    };
    let production = forge_server::durable_authorization::launch::compose(
        &database,
        None,
        Some(&execution),
        &[],
        Some(key_file.clone()),
        Box::new(NoHerdr),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let gate = Arc::new(Gate {
        root: root.clone(),
        database: database.clone(),
        endpoint: format!("http://{addr}"),
        cli: Path::new(env!("CARGO_BIN_EXE_forge"))
            .canonicalize()
            .unwrap(),
        git: git.clone(),
        production: Shared(Arc::new(production)),
        launch: {
            let launch = LaunchControl::default();
            assert!(launch.enable_controlled(), "controlled eligibility");
            launch
        },
    });
    let server = Daemon::new(gate.app())
        .with_assignment_capability_root(root.join("assignment-capabilities"))
        // Matched inspection components, as `serve` composes them, so operator
        // transcripts are readable as evidence.
        .with_durable_operator_catalog(
            &database,
            forge_server::durable_authorization::launch::ComponentCatalog::new(
                None,
                Some(&execution),
                &[],
            ),
        )
        .unwrap();
    let daemon = tokio::spawn(async move {
        axum::serve(
            listener,
            router(server).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let record = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: TASK.into(),
            project_slugs: vec!["test/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap()
        .id;
    store.set_task_base_commit(record, &base).unwrap();
    let expected_hash = component_hash(&node, &sources);

    let journey = gate.clone();
    let summary = tokio::task::spawn_blocking(move || {
        let gate = journey;
        let store = SqliteProjectRepository::open(&gate.database).unwrap();
        let mut summary = json!({
            "forge_revision": current_forge_revision(),
            "component_hash": expected_hash,
            "model": "openai/gpt-4.1",
            "launch_eligibility": "LaunchControl::enable_controlled (test-only)",
            "base_commit": base,
        });

        // ---- Implementation ----
        let (implementation, existing) = gate.app().dispatch_record(record).unwrap();
        assert!(!existing);
        log(&format!(
            "Implementation Assignment {} dispatched",
            implementation.id
        ));
        let launch = store
            .runtime("test", &implementation.id)
            .unwrap()
            .unwrap()
            .launch;
        assert_eq!(launch.forge_revision, current_forge_revision());
        assert_eq!(launch.component_hash, expected_hash);
        let state = gate.settle(&implementation.id);
        gate.transcript(&implementation.id, "implementation");
        assert!(
            store.assignment_context_read(&implementation.id).unwrap(),
            "authenticated bootstrap read no context"
        );
        let answer = store.observe_answer("test", &launch).unwrap();
        log(&format!(
            "Implementation settled: {state:?}, answer {answer:?}"
        ));
        assert_eq!(
            answer,
            AnswerObservation::OutcomeAccepted,
            "Implementation Outcome not accepted"
        );
        let checks = store.list_record_checks(record).unwrap();
        let implemented = checks[0].latest_assessment.clone().unwrap();
        assert_eq!(
            implemented.assignment_id.as_deref(),
            Some(implementation.id.as_str())
        );
        assert_eq!(implemented.verdict, Verdict::Pass);
        let output = commit_of(&implemented.evidence);
        assert_ne!(output, base, "Implementation must change the output commit");
        assert!(checks[1].latest_assessment.is_none());
        let content = gate.git(&repository, &["show", &format!("{output}:hello.txt")]);
        let changed = gate.git(&repository, &["diff", "--name-only", &base, &output]);
        log(&format!(
            "output commit {output}: files changed [{changed}], hello.txt = {content:?}"
        ));

        // No next Check before acceptance AND retirement.
        gate.app()
            .reconcile_agent_dispatch("gate-scheduler", now_millis())
            .unwrap();
        assert_eq!(
            store.list_agent_assignments(record).unwrap().len(),
            1,
            "Review launched before retirement"
        );
        let info = store.agent_assignment_info(&implementation.id).unwrap();
        if info.assignment.closed_at_millis.is_none() {
            assert!(
                info.capacity_reserved(),
                "live Assignment must hold capacity"
            );
            gate.retire(&implementation.id);
        }
        let retired = store.agent_assignment_info(&implementation.id).unwrap();
        assert!(retired.assignment.closed_at_millis.is_some());
        assert!(
            !retired.capacity_reserved(),
            "retirement must release capacity from proof"
        );
        log("Implementation retired; capacity released");
        if retired.dispatch_hold.is_some() {
            // Explicit operator decision, never an incidental side effect.
            gate.app().resume_task_dispatch(record).unwrap();
            log("operator cleared the post-retirement Dispatch Hold");
        }

        // ---- Independent Review ----
        gate.app()
            .reconcile_agent_dispatch("gate-scheduler", now_millis())
            .unwrap();
        let assignments = store.list_agent_assignments(record).unwrap();
        assert_eq!(assignments.len(), 2, "scheduler did not launch the Review");
        let review = assignments
            .into_iter()
            .find(|a| a.id != implementation.id)
            .unwrap();
        log(&format!("Review Assignment {} dispatched", review.id));
        let review_launch = store.runtime("test", &review.id).unwrap().unwrap().launch;
        assert_ne!(review_launch.runtime_id, launch.runtime_id);
        assert_ne!(review_launch.conversation_id, launch.conversation_id);
        let attempt = store.assignment_check_attempt(&review.id).unwrap().unwrap();
        assert_eq!(attempt.check_id, checks[1].id);
        assert_eq!(
            attempt.evidence, implemented.evidence,
            "Review must bind the exact output commit"
        );
        let review_state = gate.settle(&review.id);
        gate.transcript(&review.id, "review");
        let review_answer = store.observe_answer("test", &review_launch).unwrap();
        log(&format!(
            "Review settled: {review_state:?}, answer {review_answer:?}"
        ));
        assert_eq!(
            review_answer,
            AnswerObservation::OutcomeAccepted,
            "Review Outcome not accepted"
        );
        let checks = store.list_record_checks(record).unwrap();
        let reviewed = checks[1].latest_assessment.clone().unwrap();
        assert_eq!(reviewed.assignment_id.as_deref(), Some(review.id.as_str()));
        assert_eq!(reviewed.evidence, implemented.evidence);
        assert_eq!(gate.git(&repository, &["rev-parse", &output]), output);
        let review_info = store.agent_assignment_info(&review.id).unwrap();
        if review_info.assignment.closed_at_millis.is_none() {
            gate.retire(&review.id);
        }
        assert!(
            !store
                .agent_assignment_info(&review.id)
                .unwrap()
                .capacity_reserved()
        );
        log("Review retired; capacity released");

        for launch in [&launch, &review_launch] {
            let directory = forge_server::durable_authorization::DaemonAuthority::new()
                .unwrap()
                .prepare_directory(&store, "test", launch)
                .unwrap();
            assert_no_secrets(&directory);
        }
        assert_no_secrets(&gate.root.join("repository"));

        summary["implementation"] = json!({
            "assignment": implementation.id, "driver_state": format!("{state:?}"),
            "answer": format!("{answer:?}"), "verdict": format!("{:?}", implemented.verdict),
            "output_commit": output, "files_changed": changed, "hello_txt": content,
            "message": implemented.message,
        });
        summary["review"] = json!({
            "assignment": review.id, "driver_state": format!("{review_state:?}"),
            "answer": format!("{review_answer:?}"), "verdict": format!("{:?}", reviewed.verdict),
            "bound_commit": commit_of(&attempt.evidence), "message": reviewed.message,
        });
        summary["review_verdict_pass"] = json!(reviewed.verdict == Verdict::Pass);
        summary
    })
    .await
    .unwrap();
    daemon.abort();
    let _ = daemon.await;
    let mut summary = summary;
    summary["elapsed_seconds"] = json!(started.elapsed().as_secs());
    let path = root.join("evidence-summary.json");
    fs::write(&path, serde_json::to_vec_pretty(&summary).unwrap()).unwrap();
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    log(&format!(
        "PASSED: real-provider Implementation -> independent Review cycle; evidence in {}",
        root.display()
    ));
}

fn main() {
    if std::env::args().any(|arg| arg == "--list") {
        println!("durable real-provider gate (opt-in)");
        return;
    }
    let Some(key_file) = std::env::var_os("FORGE_REAL_PROVIDER_KEY_FILE") else {
        println!(
            "durable real-provider gate: skipped (opt-in; set FORGE_REAL_PROVIDER_KEY_FILE to an owner-only key file)"
        );
        return;
    };
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap()
        .block_on(run(PathBuf::from(key_file)));
}
