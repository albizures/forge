use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchRepository,
    AgentProfile, Application, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordType, RegistrationError, RepositoryInspector, WorkspaceDispatchRepository,
    WorkspaceRepository,
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
};
use forge_store::SqliteProjectRepository;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

struct NoGit;
impl RepositoryInspector for NoGit {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        panic!("configuration validation must not inspect Git")
    }
}
struct Profiles(std::sync::Arc<AtomicBool>);
impl AgentDispatchAdapterFactory for Profiles {
    fn validate_profile(&self, _: &AgentProfile) -> Result<(), AgentAdapterError> {
        if self.0.load(Ordering::SeqCst) {
            Err(AgentAdapterError::InvalidRequest(
                "unavailable prerequisite".into(),
            ))
        } else {
            Ok(())
        }
    }
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        panic!("configuration validation must never select or launch an adapter")
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn pinned_configuration_survives_template_selection_changes_and_restart_without_fallback() {
    let root = std::env::temp_dir().join(format!(
        "forge-check-profiles-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("workspace/.agents/skills/profile")).unwrap();
    let workspace = root.join("workspace");
    std::fs::write(
        workspace.join(".agents/skills/profile/SKILL.md"),
        "---\nname: profile\ndescription: profile\n---\nVerify work.\n",
    )
    .unwrap();
    let document = workspace.join(".agents/skills/profile/forge-agent.md");
    std::fs::write(&document, "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\ntools: [read]\n---\nVerify the deliverables.\n").unwrap();
    let policy = ResourceCatalog::discover(&root.join("operator"), &workspace)
        .unwrap()
        .resolve(&workspace, ".agents/skills/profile/forge-agent.md")
        .unwrap();
    let path = root.join("store.sqlite");
    let store = SqliteProjectRepository::open(&path).unwrap();
    store.create_workspace("test", "Test", &workspace).unwrap();
    store
        .register(&Project {
            reference: "test/api".into(),
            workspace_id: "test".into(),
            slug: "api".into(),
            project_root: workspace.clone(),
            origin_url: "https://example.test/api.git".into(),
        })
        .unwrap();
    let revisions: Vec<_> = (0..6)
        .map(|_| {
            store
                .register_agent_profile("test", RecordType::Task, &policy)
                .unwrap()
                .revision
        })
        .collect();
    let replacement = |version, revisions: &[u32]| TemplateReplacement {
        expected_version: version,
        actor: "operator".into(),
        message: "Select pinned profiles".into(),
        criteria: ["Implementation", "Review 1", "Review 2"]
            .iter()
            .zip(revisions)
            .map(|(name, revision)| CheckCriterion {
                name: (*name).into(),
                description: format!("Verify {name}"),
                profile_revision: Some(*revision),
            })
            .collect(),
    };
    store
        .replace_check_template("test/api", &replacement(0, &revisions[..3]))
        .unwrap();
    let open = || {
        store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                project_slugs: vec!["test/api".into()],
                body: "Research, review or implementation deliverables".into(),
                generated_by_ids: vec![],
            })
            .unwrap()
    };
    let first = open();
    let original = store.list_record_checks(first.id).unwrap();
    assert_eq!(
        original
            .iter()
            .map(|c| c.profile_revision)
            .collect::<Vec<_>>(),
        vec![Some(2), Some(3), Some(4)]
    );
    store
        .replace_check_template("test/api", &replacement(1, &revisions[3..]))
        .unwrap();
    store
        .select_workspace_profile("test", RecordType::Task, Some(revisions[5]))
        .unwrap();
    let second = open();
    assert_eq!(store.list_record_checks(first.id).unwrap(), original);
    assert_eq!(
        store
            .list_record_checks(second.id)
            .unwrap()
            .iter()
            .map(|c| c.profile_revision)
            .collect::<Vec<_>>(),
        vec![Some(5), Some(6), Some(7)]
    );
    let mut missing = replacement(2, &[999_999, revisions[4], revisions[5]]);
    store.replace_check_template("test/api", &missing).unwrap();
    let missing_task = open();
    missing.expected_version = 3;
    missing.criteria[0].profile_revision = None;
    store.replace_check_template("test/api", &missing).unwrap();
    let manual = open();
    missing.expected_version = 4;
    missing.criteria[0].profile_revision = Some(0);
    assert_eq!(
        store
            .replace_check_template("test/api", &missing)
            .unwrap_err()
            .code,
        "invalid-check-template"
    );
    assert_eq!(
        store
            .show_check_template("test/api", None, 100)
            .unwrap()
            .version,
        4
    );
    missing.criteria[0].profile_revision = Some(1);
    store.replace_check_template("test/api", &missing).unwrap();
    let unregistered_policy = open();
    let projectless = store
        .records_in_workspace("test")
        .unwrap()
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            project_slugs: vec![],
            body: "Manual deliverables".into(),
            generated_by_ids: vec![],
        })
        .unwrap();
    let baseline = store.list_record_checks(projectless.id).unwrap();
    assert_eq!(baseline.len(), 1);
    assert_eq!(baseline[0].name, "Implementation");
    assert_eq!(baseline[0].profile_revision, None);
    drop(store);
    let unavailable = std::sync::Arc::new(AtomicBool::new(false));
    let app = Application::with_agent_dispatch(
        Box::new(SqliteProjectRepository::open(&path).unwrap()),
        Box::new(SqliteProjectRepository::open(&path).unwrap()),
        Box::new(Profiles(unavailable.clone())),
        Box::new(NoGit),
    );
    assert_eq!(app.list_record_checks(first.id).unwrap(), original);
    app.validate_record_check_profiles(first.id).unwrap();
    let error = app
        .validate_record_check_profiles(missing_task.id)
        .unwrap_err();
    assert_eq!(error.code, "invalid-check-profile");
    assert!(error.message.contains("does not exist"));
    assert_eq!(
        app.validate_record_check_profiles(manual.id)
            .unwrap_err()
            .code,
        "invalid-check-profile"
    );
    assert!(
        app.validate_record_check_profiles(unregistered_policy.id)
            .unwrap_err()
            .message
            .contains("no registered launch policy")
    );
    assert_eq!(app.list_record_checks(projectless.id).unwrap(), baseline);
    unavailable.store(true, Ordering::SeqCst);
    assert!(
        app.validate_record_check_profiles(first.id)
            .unwrap_err()
            .message
            .contains("restore its prerequisites")
    );
    assert_eq!(app.list_record_checks(first.id).unwrap(), original);
    unavailable.store(false, Ordering::SeqCst);
    app.validate_record_check_profiles(first.id).unwrap();
    // A missing authoring document cannot rewrite a persisted immutable revision.
    std::fs::remove_file(document).unwrap();
    assert_eq!(
        SqliteProjectRepository::open(&path)
            .unwrap()
            .agent_profile(RecordType::Task, revisions[0])
            .unwrap()
            .launch_policy,
        Some(policy)
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
