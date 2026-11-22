//! Controlled store-level Check profile migration; no runner/model execution is claimed.
use forge_app::check_profile_migration::{Receipt, Request, Selection};
use forge_app::{
    AgentAssignment, AgentDispatchRepository, CheckDispatchPreflight, ManagedWorktreeRepository,
    NewTypedRecord, Project, ProjectRepository, RecordRepository, RecordType,
    WorkspaceDispatchRepository, WorkspaceRepository,
    agent_profiles::ResourceCatalog,
    check_templates::{CheckCriterion, TemplateReplacement},
    outcomes::Evidence,
};
use forge_store::SqliteProjectRepository;
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    store: SqliteProjectRepository,
    task: u64,
    check: u64,
    revision: u32,
    durable: u32,
}
impl Fixture {
    fn new(runtime: &str) -> Self {
        Self::with_target(runtime, "pi-durable")
    }
    fn with_target(runtime: &str, target: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forge-check-migration-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join(".agents/profiles")).unwrap();
        std::fs::write(root.join(".agents/profiles/work.md"), format!("---\nversion: 1\nruntime: {runtime}\nmodel: openai/gpt-4.1\nthinkingLevel: off\ntools: [read]\n---\nWork")).unwrap();
        let store = SqliteProjectRepository::open(&root.join("store.sqlite")).unwrap();
        store.create_workspace("test", "Test", &root).unwrap();
        store
            .register(&Project {
                reference: "test/api".into(),
                workspace_id: "test".into(),
                slug: "api".into(),
                project_root: root.clone(),
                origin_url: "https://example.test/api.git".into(),
            })
            .unwrap();
        std::fs::write(root.join(".agents/profiles/durable.md"), format!("---\nversion: 1\nruntime: {target}\nmodel: openai/gpt-4.1\nthinkingLevel: off\ntools: [read]\n---\nWork")).unwrap();
        let policy = ResourceCatalog::discover(&root.join("operator"), &root)
            .unwrap()
            .resolve(&root, ".agents/profiles/work.md")
            .unwrap();
        let profile = store
            .register_agent_profile("test", RecordType::Task, &policy)
            .unwrap();
        let durable_policy = ResourceCatalog::discover(&root.join("operator"), &root)
            .unwrap()
            .resolve(&root, ".agents/profiles/durable.md")
            .unwrap();
        let durable = store
            .register_agent_profile("test", RecordType::Task, &durable_policy)
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
                project_slugs: vec!["test/api".into()],
                body: "Deliver".into(),
                generated_by_ids: vec![],
            })
            .unwrap();
        let check = store.list_record_checks(task.id).unwrap()[0].id;
        store
            .set_task_base_commit(task.id, &"a".repeat(40))
            .unwrap();
        store.reserve_worktree_claim(task.id).unwrap();
        store.activate_worktree_claim(task.id).unwrap();
        Self {
            root,
            store,
            task: task.id,
            check,
            revision: profile.revision,
            durable: durable.revision,
        }
    }
    fn evidence(&self) -> Evidence {
        Evidence::Repository {
            repository: self.root.display().to_string(),
            commit: "a".repeat(40),
        }
    }
    fn claim(
        &self,
        selection: &CheckDispatchPreflight,
    ) -> Result<(AgentAssignment, bool), forge_app::RegistrationError> {
        self.store.claim_task_check_acceptance(
            self.task,
            "controlled-check",
            selection,
            &self.evidence(),
            self.check,
            self.revision,
            None,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn request(f: &Fixture, op: &str, generation: u64, revision: u32) -> Request {
    Request {
        task_id: f.task,
        operation_id: op.into(),
        expected_generation: generation,
        selections: vec![Selection {
            check_id: f.check,
            profile_revision: revision,
        }],
        actor: "operator".into(),
        message: "adopt durable".into(),
    }
}
fn raw_pin(f: &Fixture) -> Option<u32> {
    rusqlite::Connection::open(f.root.join("store.sqlite"))
        .unwrap()
        .query_row(
            "SELECT profile_revision FROM record_checks WHERE id = ?1",
            [f.check.cast_signed()],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn rejects_incompatible_unchanged_cross_scope_and_stale_without_history() {
    let f = Fixture::new("herdr/pi");
    // Same revision, a missing revision and a stale generation all reject atomically.
    for (bad, code) in [
        (request(&f, "a", 1, f.revision), "check-profile-unchanged"),
        (request(&f, "b", 1, 999), "invalid-check-profile"),
        (request(&f, "c", 7, f.durable), "stale-work-generation"),
    ] {
        assert_eq!(f.store.migrate_check_profiles(&bad).unwrap_err().code, code);
    }
    let mut foreign = request(&f, "d", 1, f.durable);
    foreign.selections[0].check_id = 9999;
    assert_eq!(
        f.store.migrate_check_profiles(&foreign).unwrap_err().code,
        "check-not-found"
    );
    // A Herdr target is not durable-compatible.
    let herdr_target = Fixture::with_target("pi-durable", "herdr/pi");
    assert_eq!(
        herdr_target
            .store
            .migrate_check_profiles(&request(&herdr_target, "e", 1, herdr_target.durable))
            .unwrap_err()
            .code,
        "invalid-check-profile"
    );
    assert!(f.store.check_profile_migrations(f.task).unwrap().is_empty());
    assert_eq!(
        f.store.list_record_checks(f.task).unwrap()[0].profile_revision,
        Some(f.revision)
    );
}

#[test]
fn migration_preserves_original_pin_replays_exactly_and_drives_next_attempt() {
    let f = Fixture::new("herdr/pi");
    let first = f
        .store
        .migrate_check_profiles(&request(&f, "m1", 1, f.durable))
        .unwrap();
    assert!(!first.replayed);
    assert_eq!((first.generation_before, first.generation_after), (1, 1));
    assert_eq!(first.changes[0].prior_revision, f.revision);
    assert_eq!(
        raw_pin(&f),
        Some(f.revision),
        "historical pin is not rewritten"
    );
    assert_eq!(
        f.store.list_record_checks(f.task).unwrap()[0].profile_revision,
        Some(f.durable)
    );
    // Exact retry after the state changed returns the original, even with stale generation.
    let again = f
        .store
        .migrate_check_profiles(&request(&f, "m1", 1, f.durable))
        .unwrap();
    assert!(again.replayed);
    assert_eq!(
        Receipt {
            replayed: false,
            ..again
        },
        first
    );
    assert_eq!(f.store.check_profile_migrations(f.task).unwrap().len(), 1);
    // Conflicting reuse rejects.
    let mut conflict = request(&f, "m1", 1, f.durable);
    conflict.message = "different".into();
    assert_eq!(
        f.store.migrate_check_profiles(&conflict).unwrap_err().code,
        "check-profile-migration-conflict"
    );
    // The old pin no longer authorizes dispatch; the effective one does, through ordinary gates.
    assert_eq!(
        f.claim(&CheckDispatchPreflight::PiDurable)
            .unwrap_err()
            .code,
        "check-selection-stale"
    );
    let (assignment, _) = f
        .store
        .claim_task_check_acceptance(
            f.task,
            "controlled-check",
            &CheckDispatchPreflight::PiDurable,
            &f.evidence(),
            f.check,
            f.durable,
            None,
        )
        .unwrap();
    assert_eq!(assignment.profile.revision, f.durable);
    // Active ownership blocks a further migration and leaves history untouched.
    let err = f
        .store
        .migrate_check_profiles(&request(&f, "m2", 1, f.revision))
        .unwrap_err();
    assert!(
        matches!(
            err.code,
            "execution-mode-unsafe-ownership" | "attempt-active"
        ),
        "{}",
        err.code
    );
    assert_eq!(f.store.check_profile_migrations(f.task).unwrap().len(), 1);
    assert_eq!(
        f.store.list_agent_assignments(f.task).unwrap()[0]
            .profile
            .revision,
        f.durable
    );
}

#[test]
fn migration_with_work_invalidates_freshness_once_and_replay_does_not_advance() {
    let f = Fixture::new("herdr/pi");
    // Seed prior work evidence (generation 2) the way a completed first attempt would.
    rusqlite::Connection::open(f.root.join("store.sqlite"))
        .unwrap()
        .execute(
            "INSERT INTO record_work(record_id, generation) VALUES (?1, 2)",
            [f.task.cast_signed()],
        )
        .unwrap();
    let receipt = f
        .store
        .migrate_check_profiles(&request(&f, "m1", 2, f.durable))
        .unwrap();
    assert_eq!(
        (receipt.generation_before, receipt.generation_after),
        (2, 3)
    );
    assert_eq!(f.store.record_work_state(f.task).unwrap().generation, 3);
    let replay = f
        .store
        .migrate_check_profiles(&request(&f, "m1", 2, f.durable))
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(f.store.record_work_state(f.task).unwrap().generation, 3);
}

#[test]
fn history_is_immutable_and_retained() {
    let f = Fixture::new("herdr/pi");
    f.store
        .migrate_check_profiles(&request(&f, "m1", 1, f.durable))
        .unwrap();
    let c = rusqlite::Connection::open(f.root.join("store.sqlite")).unwrap();
    for sql in [
        "UPDATE check_profile_migrations SET actor = 'x'",
        "DELETE FROM check_profile_migrations",
        "UPDATE check_profile_migration_changes SET new_revision = 99",
        "DELETE FROM check_profile_migration_changes",
    ] {
        assert!(c.execute(sql, []).is_err(), "{sql}");
    }
}

#[test]
fn concurrent_claim_and_migration_serialize_to_exactly_one_winner() {
    for _ in 0..10 {
        let f = Fixture::new("herdr/pi");
        let other =
            forge_store::SqliteProjectRepository::open(&f.root.join("store.sqlite")).unwrap();
        let migrate_request = request(&f, "race", 1, f.durable);
        let evidence = f.evidence();
        let (task, check, old) = (f.task, f.check, f.revision);
        let (migrated, claimed) = std::thread::scope(|s| {
            let m = s.spawn(|| other.migrate_check_profiles(&migrate_request));
            let c = s.spawn(|| {
                f.store.claim_task_check_acceptance(
                    task,
                    "race-claim",
                    &CheckDispatchPreflight::Herdr(forge_app::DispatchPreflightSelection {
                        task_revision: old,
                        grilling_revision: 1,
                        binding_id: "x".into(),
                        provider_generation: 0,
                    }),
                    &evidence,
                    check,
                    old,
                    None,
                )
            });
            (m.join().unwrap(), c.join().unwrap())
        });
        // The claim can never bind the old pin after a migration committed, and a
        // migration can never commit once the Task is owned: at most one succeeds.
        assert!(!(migrated.is_ok() && claimed.is_ok()), "both succeeded");
        if migrated.is_ok() {
            assert_eq!(
                f.store.list_record_checks(task).unwrap()[0].profile_revision,
                Some(f.durable)
            );
        } else {
            assert_eq!(
                f.store.list_record_checks(task).unwrap()[0].profile_revision,
                Some(f.revision)
            );
            assert!(f.store.check_profile_migrations(task).unwrap().is_empty());
        }
    }
}
