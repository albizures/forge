// Nudges through the daemon's real composition: the application built from the daemon config,
use super::recovery::registered_policy;
// whose provider adapters are `ServerAgentDispatchAdapterFactory` wrapping Herdr in
// `CapabilityDeliveryAdapter`, against the fake Herdr socket. A wrapper that did not forward
// turns and prompts left every Nudge silently skipped.
use forge_app::check_templates::{CheckCriterion, TemplateReplacement};

/// A clean Managed Worktree whose HEAD the test controls; no Git is invoked.
struct CleanCheckout(Arc<Mutex<String>>);
impl forge_app::RepositoryInspector for CleanCheckout {
    fn inspect(&self, _: &Path) -> Result<forge_app::Project, forge_app::RegistrationError> {
        unreachable!()
    }
    fn head_commit(&self, _: &Path) -> Result<String, forge_app::RegistrationError> {
        Ok(self.0.lock().unwrap().clone())
    }
}
struct CleanWorktrees;
impl forge_app::ManagedWorktreeAdapter for CleanWorktrees {
    fn validate_recovery(
        &self,
        _: &Path,
        _: &Path,
        _: &str,
        _: &str,
    ) -> Result<(), forge_app::RegistrationError> {
        Ok(())
    }
    fn create(&self, _: &Path, _: &Path, _: &str, _: &str) -> Result<(), forge_app::RegistrationError> {
        unreachable!()
    }
    fn remove(&self, _: &Path, _: bool) -> Result<(), forge_app::RegistrationError> {
        unreachable!()
    }
    fn assess(&self, _: &Path) -> forge_app::WorktreeSafetyAssessment {
        forge_app::WorktreeSafetyAssessment {
            facts: vec![],
            blockers: vec![],
            removable: true,
        }
    }
    fn run_setup_hook(
        &self,
        _: &Path,
        _: &str,
        _: &str,
        _: u64,
        _: &str,
        _: &str,
    ) -> Result<(), forge_app::RegistrationError> {
        unreachable!()
    }
}

/// The daemon's production composition, except that Git is stubbed out: provider adapters come
/// from `ServerAgentDispatchAdapterFactory`, which wraps Herdr in `CapabilityDeliveryAdapter`.
fn daemon_application(fixture: &Fixture, head: &Arc<Mutex<String>>) -> Application {
    let open = || Box::new(fixture.store());
    Application::with_managed_worktrees_and_agent_dispatch(
        open(),
        open(),
        Box::new(CleanWorktrees),
        open(),
        Box::new(ServerAgentDispatchAdapterFactory {
            herdr: HerdrAdapterFactory::from_configuration_store(fixture.database.clone()),
            daemon_url: "http://127.0.0.1:1".into(),
            capability_secret_root: fixture.root.join("assignment-capabilities"),
            cli_override: Some(std::env::current_exe().unwrap()),
        }),
        Box::new(CleanCheckout(head.clone())),
    )
    .with_workspace_repository(open())
}

fn prompts(provider: &Provider) -> Vec<String> {
    provider
        .state
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|call| call["method"] == "agent.prompt")
        .map(|call| call["params"]["text"].as_str().unwrap().to_owned())
        .collect()
}

fn finish_turn(provider: &Provider, seq: u64) {
    provider.set_agent_status("idle");
    for agent in provider.state.lock().unwrap().agents.values_mut() {
        agent["completion_seq"] = json!(seq);
    }
}

#[test]
fn an_idle_review_agent_is_nudged_once_through_the_daemon_adapter_wrapper() {
    let fixture = Fixture::new();
    let provider = Provider::new(fixture.root.join("herdr.sock"));
    let head = Arc::new(Mutex::new("a".repeat(40)));
    let app = daemon_application(&fixture, &head);
    fixture.configure(&app, &provider, "one");
    let (profile, _) = registered_policy(&fixture, RecordType::Task);
    let store = fixture.store();
    store
        .replace_check_template(
            "one/api",
            &TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Pin".into(),
                criteria: ["Implementation", "Review Implementation"]
                    .map(|name| CheckCriterion {
                        name: name.into(),
                        description: name.into(),
                        profile_revision: Some(profile.revision),
                    })
                    .into(),
            },
        )
        .unwrap();
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Deliver work".into(),
            project_slugs: vec!["one/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let claim = store.reserve_worktree_claim(task.id).unwrap();
    std::fs::create_dir_all(&claim.path).unwrap();
    store
        .set_task_base_commit(task.id, &"a".repeat(40))
        .unwrap();
    store.activate_worktree_claim(task.id).unwrap();

    let secrets = fixture.root.join("assignment-capabilities");
    let secret_of = |id: &str| std::fs::read_to_string(secrets.join(id)).unwrap();
    // Implementation asks for review on a new commit; reconciliation then starts the review.
    let (implementation, _) = app.execute_task_check_acceptance(task.id).unwrap();
    let secret = secret_of(&implementation.id);
    let session = app
        .assignment(&secrets, &implementation.id, secret.trim())
        .unwrap();
    session.bootstrap().unwrap();
    session.context().unwrap();
    *head.lock().unwrap() = "b".repeat(40);
    session.request_review().unwrap();
    app.reconcile_agent_dispatch("review-request", 1_000_000)
        .unwrap();
    let (review, _) = app.execute_task_check_acceptance(task.id).unwrap();
    assert_ne!(review.id, implementation.id);
    let secret = secret_of(&review.id);
    let session = app.assignment(&secrets, &review.id, secret.trim()).unwrap();
    session.bootstrap().unwrap();
    session.context().unwrap();
    app.reconcile_agent_dispatch("review-launched", 2_000_000)
        .unwrap();
    // Bootstrap prompts are not Nudges, and without a `completion_seq` nothing is nudged.
    let before = prompts(&provider).len();

    finish_turn(&provider, 7);
    app.reconcile_agent_dispatch("review-idle", 3_000_000)
        .unwrap();
    app.reconcile_agent_dispatch("review-idle-again", 4_000_000)
        .unwrap();
    let sent = prompts(&provider);
    assert_eq!(sent.len(), before + 1, "exactly one Nudge: {sent:?}");
    let text = &sent[before];
    assert!(text.contains("Review Implementation"), "names the Check: {text}");
    assert!(
        text.contains(&claim.path.display().to_string()),
        "names the worktree: {text}"
    );
    assert!(text.contains("forge assignment outcome --file"), "{text}");
    assert!(text.ends_with("Nudges without a verdict: 1 of 3."), "{text}");
}
