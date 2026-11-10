use forge_app::{
    AgentAssignmentClosureReason, AgentDispatchRepository, DispatchWorkspace, NewAgentAssignment,
    NewAgentAssignmentResource, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordType, WorkspaceDispatchRepository, WorkspaceRepository,
};
use forge_store::SqliteProjectRepository;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

#[test]
fn registered_policy_survives_reopen_and_lifecycle_revision_without_selection() {
    use forge_app::agent_profiles::ResourceCatalog;
    let fixture = Fixture::new();
    let workspace = fixture.0.join("one");
    let skill = workspace.join(".agents/skills/entry");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::create_dir_all(workspace.join(".pi/extensions")).unwrap();
    std::fs::write(workspace.join(".pi/extensions/entry.ts"), "never execute").unwrap();
    std::fs::write(
        workspace.join(".pi/mcp.json"),
        r#"{"mcpServers":{"local":{"command":"never-connect"}}}"#,
    )
    .unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: entry\ndescription: entry\n---\nSkill",
    )
    .unwrap();
    std::fs::write(skill.join("forge-agent.md"), "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\ntools: [read]\nskills: [{name: entry}]\nextensions: [{name: entry}]\nmcpServers: [{name: local}]\n---\nUse entry.\n").unwrap();
    let policy = ResourceCatalog::discover(&fixture.0.join("two"), &workspace)
        .unwrap()
        .resolve(&workspace, ".agents/skills/entry/forge-agent.md")
        .unwrap();
    let registered = {
        let store = fixture.open();
        fixture.register(&store);
        let mut invalid = policy.clone();
        invalid.version = 2;
        assert!(
            store
                .register_agent_profile("one", RecordType::Task, &invalid)
                .is_err()
        );
        let registered = store
            .register_agent_profile("one", RecordType::Task, &policy)
            .unwrap();
        assert_eq!(registered.launch_policy.as_ref(), Some(&policy));
        assert_eq!(
            store
                .workspace_dispatch_configuration("one")
                .unwrap()
                .task_profile,
            None
        );
        registered
    };
    std::fs::remove_file(skill.join("forge-agent.md")).unwrap();
    let store = fixture.open();
    assert_eq!(
        store
            .agent_profile(RecordType::Task, registered.revision)
            .unwrap(),
        registered
    );
    store
        .select_workspace_profile("one", RecordType::Task, Some(registered.revision))
        .unwrap();
    let copied = store
        .configure_workspace_acknowledgement_timeout("one", Some(1234))
        .unwrap();
    assert_eq!(copied.launch_policy, registered.launch_policy);
    assert_eq!(copied.model, registered.model);
    assert_eq!(copied.acknowledgement_timeout_millis, 1234);
    let next = store
        .register_agent_profile("one", RecordType::Task, &policy)
        .unwrap();
    assert_eq!(next.acknowledgement_timeout_millis, 1234);
    assert!(next.acknowledgement_timeout_overridden);
    assert_eq!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .task_profile
            .unwrap()
            .revision,
        copied.revision
    );
    let grilling = store
        .register_agent_profile("one", RecordType::Grilling, &policy)
        .unwrap();
    assert_eq!(grilling.launch_policy, Some(policy));
    assert!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .grilling_profile
            .is_none()
    );
    let legacy_copy = store
        .configure_task_acknowledgement_timeout(Some(4321))
        .unwrap();
    assert_eq!(legacy_copy.launch_policy, next.launch_policy);
    assert_eq!(
        store
            .agent_profile(RecordType::Task, registered.revision)
            .unwrap(),
        registered
    );
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forge-workspace-dispatch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("one")).unwrap();
        std::fs::create_dir_all(root.join("two")).unwrap();
        Self(root)
    }
    fn open(&self) -> SqliteProjectRepository {
        SqliteProjectRepository::open(&self.0.join("store.sqlite")).unwrap()
    }
    fn register(&self, store: &SqliteProjectRepository) {
        for id in ["one", "two"] {
            store.create_workspace(id, id, &self.0.join(id)).unwrap();
        }
    }
    fn assignment(
        &self,
        store: &SqliteProjectRepository,
        workspace: &str,
        id: &str,
    ) -> forge_app::AgentAssignment {
        let reference = format!("{workspace}/api");
        store
            .register(&Project {
                reference: reference.clone(),
                workspace_id: workspace.into(),
                slug: "api".into(),
                project_root: self.0.join(workspace).canonicalize().unwrap(),
                origin_url: "https://example.test/api.git".into(),
            })
            .unwrap();
        let record = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "work".into(),
                project_slugs: vec![reference.clone()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: id.into(),
                record_id: record.id,
                project_slug: reference,
                profile_revision: 1,
            })
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn claims_require_complete_verified_configuration_and_bind_selected_revision_and_identity() {
    let fixture = Fixture::new();
    let store = fixture.open();
    fixture.register(&store);
    let old = fixture.assignment(&store, "one", "history");
    store
        .close_agent_assignment(&old.id, AgentAssignmentClosureReason::Retired)
        .unwrap();
    assert_eq!(
        store
            .claim_record_agent_assignment(old.record_id, "missing")
            .unwrap_err()
            .code,
        "workspace-dispatch-incomplete"
    );
    for kind in [RecordType::Task, RecordType::Grilling] {
        store
            .select_workspace_profile("one", kind, Some(1))
            .unwrap();
    }
    assert!(
        store
            .claim_record_agent_assignment(old.record_id, "no-binding")
            .is_err()
    );
    store
        .retain_verified_workspace_binding(
            "one",
            Some(&DispatchWorkspace { id: "exact".into() }),
            0,
        )
        .unwrap();
    let revised = store
        .configure_workspace_acknowledgement_timeout("one", Some(1234))
        .unwrap();
    let (claimed, _) = store
        .claim_record_agent_assignment(old.record_id, "claimed")
        .unwrap();
    assert_eq!(claimed.profile, revised);
    assert_eq!(
        store
            .assignment_dispatch_workspace(&claimed.id)
            .unwrap()
            .unwrap()
            .id,
        "exact"
    );
    store
        .select_workspace_profile("one", RecordType::Task, Some(1))
        .unwrap();
    assert_eq!(
        store.agent_assignment(&claimed.id).unwrap().profile,
        revised
    );
    assert!(
        store
            .schedule_agent_assignment_retry(
                &claimed.id,
                forge_app::AgentAssignmentState::Provisioning,
                2_000,
                1,
            )
            .unwrap()
    );
    assert_eq!(
        store.agent_assignment(&claimed.id).unwrap().profile,
        revised
    );
    store
        .close_agent_assignment(&claimed.id, AgentAssignmentClosureReason::Retired)
        .unwrap();
    store
        .retain_verified_workspace_binding(
            "one",
            Some(&DispatchWorkspace {
                id: "future-container".into(),
            }),
            0,
        )
        .unwrap();
    drop(store);
    let reopened = fixture.open();
    assert_eq!(
        reopened
            .assignment_dispatch_workspace(&claimed.id)
            .unwrap()
            .unwrap()
            .id,
        "exact"
    );
    assert_eq!(
        reopened.agent_assignment(&claimed.id).unwrap().profile,
        revised
    );
    assert_eq!(
        reopened.agent_assignment(&claimed.id).unwrap().retry_count,
        1
    );
    let (future, _) = reopened
        .claim_record_agent_assignment(old.record_id, "future")
        .unwrap();
    assert_eq!(future.profile.revision, 1);
    assert_ne!(future.profile, claimed.profile);
    assert_eq!(
        reopened
            .assignment_dispatch_workspace(&future.id)
            .unwrap()
            .unwrap()
            .id,
        "future-container"
    );
    assert_eq!(
        reopened.agent_assignment(&claimed.id).unwrap().profile,
        revised
    );
}

#[test]
fn concurrent_binding_claims_have_one_owner_and_unsetting_releases_the_identity() {
    let fixture = Fixture::new();
    let store = fixture.open();
    fixture.register(&store);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let stores = [fixture.open(), fixture.open()];
    let workers: Vec<_> = stores
        .into_iter()
        .zip(["one", "two"])
        .map(|(store, workspace)| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                (
                    workspace,
                    store.retain_verified_workspace_binding(
                        workspace,
                        Some(&DispatchWorkspace {
                            id: "exclusive".into(),
                        }),
                        0,
                    ),
                )
            })
        })
        .collect();
    let outcomes: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        outcomes.iter().filter(|(_, result)| result.is_ok()).count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .find(|(_, result)| result.is_err())
            .unwrap()
            .1
            .as_ref()
            .unwrap_err()
            .code,
        "dispatch-workspace-conflict"
    );
    let winner = outcomes
        .iter()
        .find(|(_, result)| result.is_ok())
        .unwrap()
        .0;
    let loser = if winner == "one" { "two" } else { "one" };
    store
        .retain_verified_workspace_binding(winner, None, 0)
        .unwrap();
    store
        .retain_verified_workspace_binding(
            loser,
            Some(&DispatchWorkspace {
                id: "exclusive".into(),
            }),
            0,
        )
        .unwrap();
    store.remove_workspace(winner).unwrap();
    assert!(fixture.0.join(winner).is_dir());
}

#[test]
fn socket_changes_invalidate_every_binding_and_reject_stale_verification() {
    let fixture = Fixture::new();
    let store = fixture.open();
    fixture.register(&store);
    for workspace in ["one", "two"] {
        store
            .retain_verified_workspace_binding(
                workspace,
                Some(&DispatchWorkspace {
                    id: format!("herdr-{workspace}"),
                }),
                0,
            )
            .unwrap();
    }
    store
        .set_herdr_socket_path(Some(std::path::Path::new("/tmp/first.sock")))
        .unwrap();
    let first = store.workspace_dispatch_configuration("one").unwrap();
    assert_eq!(first.provider_generation, 1);
    assert!(!first.provider_verified);
    assert!(
        !store
            .workspace_dispatch_configuration("two")
            .unwrap()
            .provider_verified
    );
    assert_eq!(
        store
            .retain_verified_workspace_binding("one", first.binding.as_ref(), 0)
            .unwrap_err()
            .code,
        "provider-verification-stale"
    );
    store
        .retain_verified_workspace_binding("one", first.binding.as_ref(), 1)
        .unwrap();
    assert!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .provider_verified
    );
    store
        .set_herdr_socket_path(Some(std::path::Path::new("/tmp/first.sock")))
        .unwrap();
    assert!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .provider_verified
    );
    assert!(
        store
            .set_herdr_socket_path(Some(std::path::Path::new("relative.sock")))
            .is_err()
    );
    assert_eq!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .provider_generation,
        1
    );
    store.set_herdr_socket_path(None).unwrap();
    assert_eq!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .provider_generation,
        2
    );
    assert!(
        !store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .provider_verified
    );
    drop(store);
    assert_eq!(
        fixture
            .open()
            .workspace_dispatch_configuration("one")
            .unwrap()
            .provider_generation,
        2
    );
}

#[test]
fn binding_guards_are_workspace_local_and_socket_guards_are_daemon_wide() {
    let fixture = Fixture::new();
    let store = fixture.open();
    fixture.register(&store);
    let one = DispatchWorkspace {
        id: "herdr-one".into(),
    };
    let two = DispatchWorkspace {
        id: "herdr-two".into(),
    };
    store
        .retain_verified_workspace_binding("one", Some(&one), 0)
        .unwrap();
    let assignment = fixture.assignment(&store, "one", "busy");
    assert_eq!(
        store
            .retain_verified_workspace_binding("one", Some(&two), 0)
            .unwrap_err()
            .code,
        "dispatch-workspace-busy"
    );
    assert_eq!(
        store
            .retain_verified_workspace_binding("one", None, 0)
            .unwrap_err()
            .code,
        "dispatch-workspace-busy"
    );
    // Verification of an unchanged identity does not strand an existing attempt.
    store
        .retain_verified_workspace_binding("one", Some(&one), 0)
        .unwrap();
    store
        .retain_verified_workspace_binding("two", Some(&two), 0)
        .unwrap();
    assert_eq!(
        store
            .set_herdr_socket_path(Some(std::path::Path::new("/tmp/changed.sock")))
            .unwrap_err()
            .code,
        "dispatch-endpoint-busy"
    );
    assert!(store.herdr_socket_path().unwrap().is_none());
    store
        .add_agent_assignment_resource(&NewAgentAssignmentResource {
            assignment_id: assignment.id.clone(),
            resource_kind: "agent-name".into(),
            resource_id: "original-agent".into(),
        })
        .unwrap();
    store
        .close_agent_assignment(&assignment.id, AgentAssignmentClosureReason::Retired)
        .unwrap();
    drop(store);
    let store = fixture.open();
    assert!(
        store
            .retain_verified_workspace_binding("one", None, 0)
            .is_err()
    );
    assert!(
        store
            .set_herdr_socket_path(Some(std::path::Path::new("/tmp/changed.sock")))
            .is_err()
    );
    assert_eq!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .binding,
        Some(one)
    );
    assert_eq!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .provider_generation,
        0
    );
    store
        .retire_agent_assignment_resource(&assignment.id, "agent-name", "original-agent")
        .unwrap();
    store
        .retain_verified_workspace_binding("one", None, 0)
        .unwrap();
    store
        .set_herdr_socket_path(Some(std::path::Path::new("/tmp/changed.sock")))
        .unwrap();
    assert!(
        !store
            .workspace_dispatch_configuration("two")
            .unwrap()
            .provider_verified
    );
}

#[test]
fn closed_uncertain_provisioning_blocks_changes_until_explicit_resource_resolution() {
    let fixture = Fixture::new();
    let store = fixture.open();
    fixture.register(&store);
    let binding = DispatchWorkspace {
        id: "retained".into(),
    };
    store
        .retain_verified_workspace_binding("one", Some(&binding), 0)
        .unwrap();
    let assignment = fixture.assignment(&store, "one", "uncertain");
    assert!(store.begin_agent_provision(&assignment.id).unwrap());
    store
        .close_agent_assignment(&assignment.id, AgentAssignmentClosureReason::Retired)
        .unwrap();
    drop(store);
    let store = fixture.open();
    assert_eq!(
        store
            .retain_verified_workspace_binding("one", None, 0)
            .unwrap_err()
            .code,
        "dispatch-workspace-busy"
    );
    assert_eq!(
        store
            .set_herdr_socket_path(Some(std::path::Path::new("/tmp/new-provider.sock")))
            .unwrap_err()
            .code,
        "dispatch-endpoint-busy"
    );
    assert_eq!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .binding,
        Some(binding)
    );
    store
        .resolve_unknown_agent_resources(&assignment.id)
        .unwrap();
    store
        .retain_verified_workspace_binding("one", None, 0)
        .unwrap();
    store
        .set_herdr_socket_path(Some(std::path::Path::new("/tmp/new-provider.sock")))
        .unwrap();
}

#[test]
fn timeout_revisions_change_one_selection_without_mutating_bound_profiles() {
    let fixture = Fixture::new();
    let store = fixture.open();
    fixture.register(&store);
    for workspace in ["one", "two"] {
        store
            .select_workspace_profile(workspace, RecordType::Task, Some(1))
            .unwrap();
    }
    let assignment = fixture.assignment(&store, "one", "original");
    let updated = store
        .configure_workspace_acknowledgement_timeout("one", Some(42_000))
        .unwrap();
    assert_eq!(updated.revision, 2);
    assert_eq!(updated.acknowledgement_timeout_millis, 42_000);
    assert_eq!(
        store.agent_assignment(&assignment.id).unwrap().profile,
        assignment.profile
    );
    assert_eq!(
        store
            .workspace_dispatch_configuration("two")
            .unwrap()
            .task_profile
            .unwrap()
            .revision,
        1
    );
    assert_eq!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .task_profile,
        Some(updated.clone())
    );
    assert_eq!(
        store.agent_profile(RecordType::Task, 1).unwrap(),
        assignment.profile
    );
    assert!(
        store
            .configure_workspace_acknowledgement_timeout("one", Some(0))
            .is_err()
    );
    assert_eq!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .task_profile,
        Some(updated)
    );
    let reset = store
        .configure_workspace_acknowledgement_timeout("one", None)
        .unwrap();
    assert_eq!(reset.revision, 3);
    assert_eq!(reset.acknowledgement_timeout_millis, 300_000);
    assert!(!reset.acknowledgement_timeout_overridden);
    store
        .select_workspace_profile("two", RecordType::Task, None)
        .unwrap();
    assert_eq!(
        store
            .configure_workspace_acknowledgement_timeout("two", Some(1000))
            .unwrap_err()
            .code,
        "task-profile-required"
    );
    drop(store);
    assert_eq!(
        fixture
            .open()
            .workspace_dispatch_configuration("one")
            .unwrap()
            .task_profile,
        Some(reset)
    );
}

#[test]
fn selections_and_verified_bindings_are_independent_exclusive_and_durable() {
    let fixture = Fixture::new();
    let store = fixture.open();
    fixture.register(&store);
    let empty = store.workspace_dispatch_configuration("one").unwrap();
    assert_eq!(empty.workspace_id, "one");
    assert!(empty.task_profile.is_none());
    assert!(empty.grilling_profile.is_none());
    assert!(empty.binding.is_none());
    assert!(!empty.provider_verified);
    store
        .select_workspace_profile("one", RecordType::Task, Some(1))
        .unwrap();
    store
        .select_workspace_profile("one", RecordType::Grilling, Some(1))
        .unwrap();
    let binding = DispatchWorkspace {
        id: "operator-selected-identity".into(),
    };
    store
        .retain_verified_workspace_binding("one", Some(&binding), empty.provider_generation)
        .unwrap();
    let configured = store.workspace_dispatch_configuration("one").unwrap();
    assert_eq!(configured.task_profile.unwrap().revision, 1);
    assert_eq!(
        configured.grilling_profile.unwrap().record_type,
        RecordType::Grilling
    );
    assert_eq!(configured.binding, Some(binding.clone()));
    assert!(configured.provider_verified);
    assert_eq!(
        store
            .retain_verified_workspace_binding("two", Some(&binding), empty.provider_generation)
            .unwrap_err()
            .code,
        "dispatch-workspace-conflict"
    );
    let other = store.workspace_dispatch_configuration("two").unwrap();
    assert!(other.task_profile.is_none());
    assert!(other.binding.is_none());
    assert_eq!(
        store
            .select_workspace_profile("one", RecordType::Task, Some(999))
            .unwrap_err()
            .code,
        "agent-profile-not-found"
    );
    assert!(
        store
            .select_workspace_profile("one", RecordType::Spec, Some(1))
            .is_err()
    );
    assert!(
        store
            .select_workspace_profile("absent", RecordType::Task, Some(1))
            .is_err()
    );
    assert_eq!(
        store
            .workspace_dispatch_configuration("one")
            .unwrap()
            .task_profile
            .unwrap()
            .revision,
        1
    );
    drop(store);
    let reopened = fixture.open();
    assert_eq!(
        reopened
            .workspace_dispatch_configuration("one")
            .unwrap()
            .binding,
        Some(binding)
    );
    assert!(
        reopened
            .workspace_dispatch_configuration("one")
            .unwrap()
            .provider_verified
    );
    assert_eq!(
        reopened.remove_workspace("one").unwrap_err().code,
        "workspace-not-empty"
    );
    reopened.remove_workspace("two").unwrap();
    assert!(fixture.0.join("two").is_dir());
}
