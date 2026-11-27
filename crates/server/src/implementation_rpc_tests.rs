// RPC-level proof that the verdict-free Implementation loop holds through the real gated
// handlers: an Implementation Assignment can only comment, read and request review, while the
// review Check keeps its full outcome surface. The app-level `submit_outcome` is ungated by
// design, so everything here goes through the JSON-RPC router.

struct ImplementationRig {
    _daemon: AssignmentTestDaemon,
    daemon: Daemon,
    store: SqliteProjectRepository,
    secrets: PathBuf,
    root: PathBuf,
    profile_revision: u32,
    generation: u64,
    next: std::cell::Cell<u32>,
}

struct RigTask {
    id: u64,
    checks: Vec<forge_app::checks::RecordCheck>,
    checkout: PathBuf,
}

impl ImplementationRig {
    #[allow(clippy::too_many_lines)]
    fn new() -> Self {
        use forge_app::{WorkspaceDispatchRepository, WorkspaceRepository};
        let (fixture, _, _) = assignment_test_daemon_scoped(true);
        let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
        for assignment in ["owner-assignment", "target-assignment"] {
            store
                .close_agent_assignment(assignment, forge_app::AgentAssignmentClosureReason::Retired)
                .unwrap();
        }
        let workspace = store.workspace("test").unwrap();
        let skill = workspace.root.join(".agents/skills/work");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: work\ndescription: work\n---\nWork").unwrap();
        std::fs::create_dir_all(workspace.root.join(".agents/profiles")).unwrap();
        std::fs::write(
            workspace.root.join(".agents/profiles/work.md"),
            "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: [read]\n---\nWork",
        )
        .unwrap();
        let policy = forge_app::agent_profiles::ResourceCatalog::discover(
            &workspace.root.join("operator"),
            &workspace.root,
        )
        .unwrap()
        .resolve(&workspace.root, ".agents/profiles/work.md")
        .unwrap();
        let profile = store.register_agent_profile("test", RecordType::Task, &policy).unwrap();
        store
            .replace_check_template(
                "alpha",
                &forge_app::check_templates::TemplateReplacement {
                    expected_version: 0,
                    actor: "operator".into(),
                    message: "Pin".into(),
                    criteria: ["Implementation", "Review"]
                        .map(|name| forge_app::check_templates::CheckCriterion {
                            name: name.into(),
                            description: name.into(),
                            profile_revision: Some(profile.revision),
                        })
                        .into(),
                },
            )
            .unwrap();
        store.select_workspace_profile("test", RecordType::Task, Some(profile.revision)).unwrap();
        store.select_workspace_profile("test", RecordType::Grilling, Some(1)).unwrap();
        let generation = store.workspace_dispatch_configuration("test").unwrap().provider_generation;
        store
            .retain_verified_workspace_binding(
                "test",
                Some(&forge_app::DispatchWorkspace { id: "provider".into() }),
                generation,
            )
            .unwrap();
        store.set_project_task_concurrency_limit("alpha", Some(10)).unwrap();
        let repository = workspace.root.join("alpha");
        std::fs::create_dir_all(&repository).unwrap();
        super::git(&repository, &["init"]).unwrap();
        super::git(
            &repository,
            &["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "--allow-empty", "-m", "Input"],
        )
        .unwrap();
        let secrets = fixture.database_path.with_extension("assignment-secrets");
        let mut daemon = Daemon::new(
            Application::with_managed_worktrees_and_agent_dispatch(
                Box::new(SqliteProjectRepository::open(&fixture.database_path).unwrap()),
                Box::new(SqliteProjectRepository::open(&fixture.database_path).unwrap()),
                Box::new(super::GitWorktreeAdapter),
                Box::new(SqliteProjectRepository::open(&fixture.database_path).unwrap()),
                Box::new(super::ServerAgentDispatchAdapterFactory {
                    herdr: super::HerdrAdapterFactory::from_configuration_store(fixture.database_path.clone()),
                    daemon_url: "http://127.0.0.1:45003".into(),
                    capability_secret_root: secrets.clone(),
                    cli_override: None,
                }),
                Box::new(super::GitInspector),
            )
            .with_workspace_repository(Box::new(SqliteProjectRepository::open(&fixture.database_path).unwrap())),
        );
        daemon.capability_secret_root = Some(secrets.clone());
        Self {
            daemon,
            store,
            secrets,
            root: workspace.root,
            profile_revision: profile.revision,
            generation,
            next: std::cell::Cell::new(0),
            _daemon: fixture,
        }
    }

    /// A fresh Task with its own Managed Worktree and Implementation + Review Checks.
    fn task(&self) -> RigTask {
        use forge_app::ManagedWorktreeRepository;
        let parent = self
            .store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Spec,
                body: "Validated execution plan".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let task = self.store.open_task_under_spec("Deliver", parent.id, vec![]).unwrap();
        self.store.start_record(parent.id).unwrap();
        let checks = self.store.list_record_checks(task.id).unwrap();
        let checkout = self.store.reserve_worktree_claim(task.id).unwrap().path;
        let repository = self.root.join("alpha");
        let branch = format!("feat/forge-{}", task.id);
        super::git(&repository, &["worktree", "add", "-b", &branch, checkout.to_str().unwrap()]).unwrap();
        self.store
            .set_task_base_commit(task.id, &super::git(&checkout, &["rev-parse", "HEAD"]).unwrap())
            .unwrap();
        self.store.activate_worktree_claim(task.id).unwrap();
        RigTask { id: task.id, checks, checkout }
    }

    fn evidence(&self, task: &RigTask) -> forge_app::outcomes::Evidence {
        forge_app::outcomes::Evidence::Repository {
            repository: self.root.join("alpha").display().to_string(),
            commit: super::git(&task.checkout, &["rev-parse", "HEAD"]).unwrap(),
        }
    }

    /// Bind a new Assignment to `check` and return its (id, credentials).
    fn claim(&self, task: &RigTask, check: usize) -> (String, serde_json::Value) {
        let n = self.next.get();
        self.next.set(n + 1);
        let id = format!("rig-{n}");
        self.store
            .claim_task_check_acceptance(
                task.id,
                &id,
                &forge_app::DispatchPreflightSelection {
                    task_revision: self.profile_revision,
                    grilling_revision: 1,
                    binding_id: "provider".into(),
                    provider_generation: self.generation,
                },
                &self.evidence(task),
                task.checks[check].id,
                self.profile_revision,
                None,
            )
            .unwrap();
        std::fs::write(self.secrets.join(&id), "rig-secret").unwrap();
        let credentials = serde_json::json!({"assignmentId": id, "secret": "rig-secret"});
        (id, credentials)
    }

    async fn rpc(&self, method: &str, params: serde_json::Value) -> serde_json::Value {
        assignment_rpc(self.daemon.clone(), method, params).await
    }

    /// Claim the Implementation Check, commit work, and request review through RPC; then
    /// retire the Implementation Assignment and bind the Review Check.
    async fn in_review(&self, task: &RigTask) -> (String, serde_json::Value, serde_json::Value) {
        let (_, implementation) = self.claim(task, 0);
        let boot = self.rpc("assignment.bootstrap", implementation.clone()).await;
        assert!(boot.get("error").is_none(), "{boot}");
        std::fs::write(task.checkout.join("deliverable.txt"), "Implemented\n").unwrap();
        super::git(&task.checkout, &["add", "deliverable.txt"]).unwrap();
        super::git(
            &task.checkout,
            &["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "Output"],
        )
        .unwrap();
        let accepted = self.rpc("assignment.reviewRequest", implementation.clone()).await;
        assert!(accepted.get("error").is_none(), "{accepted}");
        let id = implementation["assignmentId"].as_str().unwrap();
        self.store
            .close_agent_assignment(id, forge_app::AgentAssignmentClosureReason::Retired)
            .unwrap();
        let (review, credentials) = self.claim(task, 1);
        let boot = self.rpc("assignment.bootstrap", credentials.clone()).await;
        assert!(boot["result"]["context"].is_object(), "review keeps the standard bootstrap: {boot}");
        (review, credentials, accepted["result"]["evidence"].clone())
    }

    fn outcome(credentials: &serde_json::Value, evidence: &serde_json::Value) -> serde_json::Value {
        let mut request = credentials.clone();
        request["outcome"] = serde_json::json!({
            "attempt_id": credentials["assignmentId"],
            "generation": 1,
            "evidence": evidence,
            "verdict": "pass",
            "actor": "spoofed",
            "message": "Reviewed",
            "disposition": "complete",
            "findings": [],
            "verifications": [],
        });
        request
    }
}

#[tokio::test]
async fn an_implementation_assignment_reads_comments_and_requests_review_but_cannot_judge() {
    let rig = ImplementationRig::new();
    let task = rig.task();
    let (_, credentials) = rig.claim(&task, 0);
    let with = |extra: serde_json::Value| {
        let mut value = credentials.clone();
        value.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        value
    };

    // Allowed: the minimal bootstrap, context, comments, Findings, the Task record.
    let boot = rig.rpc("assignment.bootstrap", credentials.clone()).await;
    assert!(boot["result"]["commandContract"].is_string(), "{boot}");
    assert_eq!(boot["result"]["taskBody"], "Deliver", "{boot}");
    assert!(boot["result"].get("context").is_none(), "{boot}");
    let context = rig.rpc("assignment.context", credentials.clone()).await;
    assert!(context.get("error").is_none(), "{context}");
    let comment = rig.rpc("assignment.comment", with(serde_json::json!({"body": "Progress note"}))).await;
    assert!(comment.get("error").is_none(), "{comment}");
    let findings = rig.rpc("assignment.findingList", with(serde_json::json!({"after": null, "limit": 10, "compareRevision": null}))).await;
    assert!(findings.get("error").is_none(), "{findings}");
    let record = rig.rpc("assignment.record", with(serde_json::json!({"target": task.id}))).await;
    assert!(record.get("error").is_none(), "{record}");

    // Forbidden, and nothing is recorded.
    let before_checks = rig.store.list_record_checks(task.id).unwrap();
    let before_findings = rig.store.list_record_findings(task.id, None, 100).unwrap();
    let evidence = serde_json::to_value(rig.evidence(&task)).unwrap();
    let forbidden = [
        ("assignment.outcomeSubmit", ImplementationRig::outcome(&credentials, &evidence)),
        ("assignment.checkAssess", with(serde_json::json!({"check": task.checks[0].id, "expectedVersion": 0, "verdict": "pass", "message": "Done"}))),
        ("assignment.checkInvalidate", with(serde_json::json!({"check": task.checks[0].id, "expectedVersion": 0, "assessmentId": 1, "message": "No"}))),
        ("assignment.findingRaise", with(serde_json::json!({"severity": "major", "blocking": true, "message": "Self-raised"}))),
        ("assignment.findingMutate", with(serde_json::json!({"finding": 1, "expectedVersion": 1, "change": {"action": "resolve"}, "message": "Self-resolved"}))),
    ];
    for (method, params) in forbidden {
        let rejected = rig.rpc(method, params).await;
        assert_eq!(rejected["error"]["data"], "implementation-outcome-forbidden", "{method}: {rejected}");
    }
    assert_eq!(rig.store.list_record_checks(task.id).unwrap(), before_checks);
    assert_eq!(rig.store.list_record_findings(task.id, None, 100).unwrap(), before_findings);

    // `done` is not a back door: a Dispatched Task cannot be completed by its Agent.
    let done = rig
        .rpc("assignment.done", with(serde_json::json!({"resolution": "All done", "human": null})))
        .await;
    assert!(done.get("error").is_some(), "{done}");
    assert_eq!(rig.store.show_record(task.id).unwrap().status, forge_app::RecordStatus::InProgress);

    // A dirty worktree rejects the Review Request; a committed one is accepted, repeatably.
    std::fs::write(task.checkout.join("deliverable.txt"), "Implemented\n").unwrap();
    let dirty = rig.rpc("assignment.reviewRequest", credentials.clone()).await;
    assert_eq!(dirty["error"]["data"], "review-request-worktree-invalid", "{dirty}");
    super::git(&task.checkout, &["add", "deliverable.txt"]).unwrap();
    super::git(&task.checkout, &["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "Output"]).unwrap();
    let accepted = rig.rpc("assignment.reviewRequest", credentials.clone()).await;
    assert_eq!(accepted["result"]["check"]["latest_assessment"]["verdict"], "pass", "{accepted}");
    assert_eq!(rig.rpc("assignment.reviewRequest", credentials).await["result"], accepted["result"]);
}

#[tokio::test]
async fn a_review_check_may_pass_through_the_gated_handlers() {
    let rig = ImplementationRig::new();
    let task = rig.task();
    let (_, credentials, evidence) = rig.in_review(&task).await;
    let accepted = rig.rpc("assignment.outcomeSubmit", ImplementationRig::outcome(&credentials, &evidence)).await;
    assert_eq!(accepted["result"]["check"]["current_verdict"], "pass", "{accepted}");
    assert_eq!(accepted["result"]["attempt"]["actor"], credentials["assignmentId"], "{accepted}");
}

#[tokio::test]
async fn a_review_check_may_hand_back_with_findings_through_the_gated_handlers() {
    let rig = ImplementationRig::new();
    let task = rig.task();
    let (_, credentials, evidence) = rig.in_review(&task).await;
    let mut request = ImplementationRig::outcome(&credentials, &evidence);
    request["outcome"]["verdict"] = "fail".into();
    request["outcome"]["disposition"] = "handback".into();
    request["outcome"]["findings"] = serde_json::json!([{"severity": "major", "blocking": true, "message": "Result is missing"}]);
    let accepted = rig.rpc("assignment.outcomeSubmit", request).await;
    assert!(accepted.get("error").is_none(), "{accepted}");
    assert_eq!(accepted["result"]["findings"][0]["raised"]["message"], "Result is missing", "{accepted}");
    assert_eq!(accepted["result"]["work"]["generation"], 2, "{accepted}");
    let open = rig.store.list_record_findings(task.id, None, 100).unwrap();
    assert_eq!(open.findings.len(), 1);
    assert!(rig.store.dispatch_hold(task.id).unwrap().is_none());
}

#[tokio::test]
async fn a_review_check_may_request_human_input_or_report_an_interruption_through_the_gated_handlers() {
    let rig = ImplementationRig::new();
    let task = rig.task();
    let (_, credentials, evidence) = rig.in_review(&task).await;
    let mut interrupted = ImplementationRig::outcome(&credentials, &evidence);
    interrupted["outcome"]["verdict"] = "fail".into();
    interrupted["outcome"]["disposition"] = "interrupted".into();
    let accepted = rig.rpc("assignment.outcomeSubmit", interrupted).await;
    assert!(accepted.get("error").is_none(), "{accepted}");
    assert!(rig.store.dispatch_hold(task.id).unwrap().is_none(), "one interruption retries");
    assert_eq!(rig.store.record_work_state(task.id).unwrap().progress.interrupted_checks[0].interruptions, 1);

    let other = rig.task();
    let (_, credentials, evidence) = rig.in_review(&other).await;
    let mut human = ImplementationRig::outcome(&credentials, &evidence);
    human["outcome"]["verdict"] = "fail".into();
    human["outcome"]["disposition"] = "human-input".into();
    human["outcome"]["message"] = "Which API key?".into();
    let accepted = rig.rpc("assignment.outcomeSubmit", human).await;
    assert!(accepted.get("error").is_none(), "{accepted}");
    let hold = rig.store.dispatch_hold(other.id).unwrap().expect("human input holds the Task");
    assert!(hold.reason.contains("Which API key?"), "{}", hold.reason);
}
