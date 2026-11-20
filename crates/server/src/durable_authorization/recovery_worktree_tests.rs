//! Real local Git; the recovery snapshot port is a deterministic protocol fake.
use super::*;
use forge_app::{
    authorization_lease::Lease,
    durable_execution::recovery::{
        Candidate, Claim,
        worktree::{self, Repository},
    },
    outcomes::Attempt,
};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    binding: RefCell<Binding>,
    claim: Claim,
    live: Cell<bool>,
}
impl Repository for Fixture {
    fn claimed_worktree_binding(
        &self,
        workspace: &str,
        claim: &Claim,
    ) -> Result<Binding, RegistrationError> {
        if workspace != "test" || *claim != self.claim || !self.live.get() {
            return Err(denied());
        }
        Ok(self.binding.borrow().clone())
    }
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "forge-recovery-git-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let project = root.join("repository");
        fs::create_dir(&project).unwrap();
        git(&project, &["init"]).unwrap();
        fs::write(project.join("tracked"), "base\n").unwrap();
        git(&project, &["add", "tracked"]).unwrap();
        commit(&project);
        let base = crate::GitInspector.head_commit(&project).unwrap();
        let path = forge_app::managed_worktree_path(&root, "alpha", 1);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        git(
            &project,
            &[
                "worktree",
                "add",
                "-b",
                "feat/forge-1",
                path.to_str().unwrap(),
                &base,
            ],
        )
        .unwrap();
        let lease: Lease = serde_json::from_str(include_str!(
            "../../../../runner/test/authorization-lease.json"
        ))
        .unwrap();
        let candidate = Candidate {
            launch: lease.launch.clone(),
            admission_id: "original".into(),
            attempt: Attempt {
                id: lease.launch.attempt_id.clone(),
                record_id: 1,
                check_id: 1,
                generation: lease.generation,
                evidence: Evidence::Repository {
                    repository: project.display().to_string(),
                    commit: base.clone(),
                },
                actor: lease.launch.assignment_id.clone(),
                body: "bound Task".into(),
                continuation: None,
                recovery: None,
            },
            execution_directory: path.display().to_string(),
            last_lease: lease,
        };
        let claim =
            Claim::for_candidate(&candidate, "recover".into(), "fresh-epoch".into()).unwrap();
        Self {
            root,
            claim,
            live: Cell::new(true),
            binding: RefCell::new(Binding {
                candidate,
                project_root: project,
                worktree_path: path,
                base_commit: base,
                check_name: "Implementation".into(),
            }),
        }
    }
    fn path(&self) -> PathBuf {
        self.binding.borrow().worktree_path.clone()
    }
    fn inspect(&self) -> Result<worktree::Inspection, RegistrationError> {
        worktree::inspect(self, &GitInspector, "test", &self.claim)
    }
    fn index(&self) -> Vec<u8> {
        let path = git(
            &self.path(),
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        )
        .unwrap();
        fs::read(String::from_utf8(path).unwrap().trim()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn commit(path: &Path) {
    git(
        path,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "progress",
        ],
    )
    .unwrap();
}

// Actual SQLite authority and actual Git composition, with synthetic original
// admission/lease evidence only. This does not verify a live/recovered process.
fn store(f: &Fixture) -> forge_store::SqliteProjectRepository {
    use forge_app::durable_execution::{Admission, Repository as _};
    use forge_app::{
        AgentDispatchRepository, NewAgentAssignment, NewTypedRecord, Project, ProjectRepository,
        RecordRepository, RecordType, WorkspaceRepository,
    };
    let database = f.root.join("forge.sqlite");
    let store = forge_store::SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("test", "Test", &f.root).unwrap();
    let store = store.in_record_workspace("test").unwrap();
    let mut binding = f.binding.borrow_mut();
    store
        .register(&Project {
            reference: "alpha".into(),
            workspace_id: "test".into(),
            slug: "alpha".into(),
            project_root: binding.project_root.clone(),
            origin_url: "https://example.test/repo.git".into(),
        })
        .unwrap();
    let c = rusqlite::Connection::open(&database).unwrap();
    c.execute("INSERT INTO agent_profiles(record_type, revision, provider, agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis) SELECT record_type, 2, 'pi-durable', agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis FROM agent_profiles WHERE record_type = 'task' AND revision = 1", []).unwrap();
    let task = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "bound Task".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    assert_eq!(task.id, binding.candidate.attempt.record_id);
    store
        .create_agent_assignment(&NewAgentAssignment {
            id: "assignment".into(),
            record_id: task.id,
            project_slug: "alpha".into(),
            profile_revision: 2,
        })
        .unwrap();
    binding.candidate.attempt.check_id = store.list_record_checks(task.id).unwrap()[0].id;
    binding.candidate.launch.profile_revision = 2;
    binding.candidate.last_lease.launch = binding.candidate.launch.clone();
    let attempt = &binding.candidate.attempt;
    c.execute("INSERT INTO record_attempts(id, record_id, check_id, attempt_json) VALUES (?1, ?2, ?3, ?4)", rusqlite::params![attempt.id, task.id.cast_signed(), attempt.check_id.cast_signed(), serde_json::to_string(attempt).unwrap()]).unwrap();
    c.execute(
        "INSERT INTO assignment_check_attempts VALUES ('assignment', 'attempt')",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO assignment_execution_bindings VALUES ('assignment', ?1)",
        [&binding.candidate.execution_directory],
    )
    .unwrap();
    c.execute("INSERT INTO worktree_claims SELECT record_id, project_slug, ?1, 'active' FROM agent_assignments WHERE id = 'assignment'", [&binding.candidate.execution_directory]).unwrap();
    c.execute(
        "INSERT INTO task_base_commits VALUES (?1, ?2)",
        rusqlite::params![task.id.cast_signed(), binding.base_commit],
    )
    .unwrap();
    let launch = &binding.candidate.launch;
    store.reserve("test", launch).unwrap();
    store.begin_send("test", launch).unwrap();
    store
        .record_result(
            "test",
            launch,
            &Admission::Accepted {
                admission_id: "original".into(),
            },
        )
        .unwrap();
    forge_app::authorization_lease::Repository::grant_lease(
        &store,
        "test",
        &binding.candidate.last_lease,
        1_000,
    )
    .unwrap();
    drop(binding);
    store
}

#[test]
fn recovery_worktree_real_store_and_git_preserve_attempt_capacity_and_evidence() {
    use forge_app::{
        AgentDispatchRepository as _, RecordRepository as _,
        durable_execution::recovery::Repository as _,
    };
    let f = Fixture::new();
    let store = store(&f);
    let candidate = store
        .recovery_candidate("test", &f.binding.borrow().candidate.launch)
        .unwrap();
    let claim = Claim::for_candidate(&candidate, "recover".into(), "fresh-epoch".into()).unwrap();
    store.claim_recovery("test", &claim).unwrap();
    let before = store.agent_assignment_info("assignment").unwrap();
    let work = store
        .record_work_state(candidate.attempt.record_id)
        .unwrap();
    let index = f.index();
    fs::write(f.path().join("tracked"), "interrupted progress\n").unwrap();
    let proof = worktree::inspect(&store, &GitInspector, "test", &claim).unwrap();
    proof.verify(&store, &GitInspector).unwrap();
    assert_eq!(proof.binding().candidate, candidate);
    assert!(proof.observation().dirty);
    assert_eq!(store.agent_assignment_info("assignment").unwrap(), before);
    assert!(before.capacity_reserved());
    assert_eq!(
        store
            .record_work_state(candidate.attempt.record_id)
            .unwrap(),
        work
    );
    assert_eq!(f.index(), index);
    assert_eq!(
        fs::read(f.path().join("tracked")).unwrap(),
        b"interrupted progress\n"
    );
    store
        .close_agent_assignment(
            "assignment",
            forge_app::AgentAssignmentClosureReason::Retired,
        )
        .unwrap();
    assert!(proof.verify(&store, &GitInspector).is_err());
    assert!(
        store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}

#[test]
fn recovery_worktree_observes_implementation_progress_without_adopting_or_repairing_it() {
    let f = Fixture::new();
    let original = f.binding.borrow().clone();
    let initial = f.inspect().unwrap();
    assert!(!initial.observation().dirty);
    fs::write(f.path().join("tracked"), "committed progress\n").unwrap();
    git(&f.path(), &["add", "tracked"]).unwrap();
    commit(&f.path());
    fs::write(f.path().join("tracked"), "staged progress\n").unwrap();
    git(&f.path(), &["add", "tracked"]).unwrap();
    fs::write(f.path().join("tracked"), "working progress\n").unwrap();
    fs::write(f.path().join("untracked"), b"untracked\0\xff").unwrap();
    let index = f.index();
    let proof = f.inspect().unwrap();
    assert!(proof.observation().dirty);
    assert_ne!(
        proof.observation().head,
        original.candidate.attempt.evidence
    );
    assert_ne!(proof.observation().retained, initial.observation().retained);
    proof.verify(&f, &GitInspector).unwrap();
    assert_eq!(*f.binding.borrow(), original);
    assert_eq!(f.index(), index);
    assert_eq!(
        fs::read(f.path().join("tracked")).unwrap(),
        b"working progress\n"
    );
    assert_eq!(
        fs::read(f.path().join("untracked")).unwrap(),
        b"untracked\0\xff"
    );
    assert!(initial.verify(&f, &GitInspector).is_err());
    fs::write(f.path().join("untracked"), "later bytes").unwrap();
    assert!(proof.verify(&f, &GitInspector).is_err());
}

#[test]
fn recovery_worktree_nonimplementation_requires_exact_clean_bound_input() {
    for name in ["Review", "Publication"] {
        let f = Fixture::new();
        f.binding.borrow_mut().check_name = name.into();
        f.inspect().unwrap();
        fs::write(f.path().join("tracked"), "modified\n").unwrap();
        assert!(f.inspect().is_err());
        git(&f.path(), &["add", "tracked"]).unwrap();
        assert!(f.inspect().is_err());
        commit(&f.path());
        assert!(
            f.inspect().is_err(),
            "clean but advanced HEAD must not rebind a Check"
        );
    }
}

#[test]
fn recovery_worktree_hidden_index_flags_cannot_certify_a_clean_review() {
    for flag in ["--assume-unchanged", "--skip-worktree"] {
        let f = Fixture::new();
        f.binding.borrow_mut().check_name = "Review".into();
        git(&f.path(), &["update-index", flag, "tracked"]).unwrap();
        fs::write(f.path().join("tracked"), "hidden bytes\n").unwrap();
        let index = f.index();
        assert!(f.inspect().is_err());
        f.binding.borrow_mut().check_name = "Implementation".into();
        assert!(f.inspect().unwrap().observation().dirty);
        assert_eq!(f.index(), index);
        assert_eq!(
            fs::read(f.path().join("tracked")).unwrap(),
            b"hidden bytes\n"
        );
    }
}

#[test]
fn recovery_worktree_rejects_wrong_branch_base_repository_and_original_input_ancestry() {
    for fault in 0..5 {
        let f = Fixture::new();
        match fault {
            0 => {
                git(&f.path(), &["checkout", "--detach"]).unwrap();
            }
            1 => {
                git(&f.path(), &["checkout", "-b", "foreign"]).unwrap();
            }
            2 => {
                f.binding.borrow_mut().base_commit = "f".repeat(40);
            }
            3 => {
                f.binding.borrow_mut().project_root = f.root.clone();
            }
            4 => {
                // A real descendant of the pinned base, absent from current HEAD.
                let project = f.binding.borrow().project_root.clone();
                fs::write(project.join("tracked"), "different input\n").unwrap();
                git(&project, &["add", "tracked"]).unwrap();
                commit(&project);
                let head = crate::GitInspector.head_commit(&project).unwrap();
                if let Evidence::Repository { commit, .. } =
                    &mut f.binding.borrow_mut().candidate.attempt.evidence
                {
                    *commit = head;
                }
            }
            _ => unreachable!(),
        }
        let index = f.index();
        assert!(f.inspect().is_err());
        assert_eq!(f.index(), index);
        assert_eq!(fs::read(f.path().join("tracked")).unwrap(), b"base\n");
    }
}

#[test]
fn recovery_worktree_revalidates_live_forge_binding_after_external_observation() {
    struct Revoke<'a>(&'a Fixture, Cell<u32>);
    impl Inspector for Revoke<'_> {
        fn observe(&self, binding: &Binding) -> Result<Observation, RegistrationError> {
            let observed = GitInspector.observe(binding)?;
            self.1.set(self.1.get() + 1);
            if self.1.get() == 2 {
                self.0.live.set(false);
            }
            Ok(observed)
        }
    }
    let f = Fixture::new();
    let revoke = Revoke(&f, Cell::new(0));
    assert!(worktree::inspect(&f, &revoke, "test", &f.claim).is_err());
    assert_eq!(
        revoke.1.get(),
        2,
        "revocation is injected during the final external observation"
    );
    f.live.set(true);
    let proof = f.inspect().unwrap();
    f.binding.borrow_mut().base_commit = "b".repeat(40);
    assert!(proof.verify(&f, &GitInspector).is_err());
}

#[test]
fn recovery_worktree_submodules_remain_unknown_without_repair() {
    let f = Fixture::new();
    let commit = crate::GitInspector.head_commit(&f.path()).unwrap();
    git(
        &f.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{commit},nested"),
        ],
    )
    .unwrap();
    fs::create_dir(f.path().join("nested")).unwrap();
    let index = f.index();
    assert!(f.inspect().is_err());
    assert_eq!(f.index(), index);
}

#[test]
fn recovery_worktree_racing_content_cannot_produce_a_stable_observation() {
    struct Change<'a>(&'a Fixture, Cell<u32>);
    impl Inspector for Change<'_> {
        fn observe(&self, binding: &Binding) -> Result<Observation, RegistrationError> {
            let observed = GitInspector.observe(binding)?;
            self.1.set(self.1.get() + 1);
            if self.1.get() == 1 {
                fs::write(self.0.path().join("tracked"), "racing writer\n").unwrap();
            }
            Ok(observed)
        }
    }
    let clean = Fixture::new();
    let change = Change(&clean, Cell::new(0));
    assert!(worktree::inspect(&clean, &change, "test", &clean.claim).is_err());
    assert_eq!(change.1.get(), 2);
    assert_eq!(
        fs::read(clean.path().join("tracked")).unwrap(),
        b"racing writer\n"
    );
}

#[test]
fn recovery_worktree_never_invokes_configured_fsmonitor_or_clean_filters() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let marker = f.root.join("external-program-ran");
    let script = f.root.join("observer-hook");
    fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    git(
        &f.path(),
        &["config", "core.fsmonitor", script.to_str().unwrap()],
    )
    .unwrap();
    f.inspect().unwrap();
    assert!(!marker.exists());
    // Clean/process driver configuration is an explicit unsupported input;
    // status must not discover it by executing an external program.
    git(
        &f.path(),
        &["config", "filter.marker.clean", script.to_str().unwrap()],
    )
    .unwrap();
    fs::write(f.path().join(".gitattributes"), "tracked filter=marker\n").unwrap();
    fs::write(f.path().join("tracked"), "changed\n").unwrap();
    assert!(f.inspect().is_err());
    assert!(!marker.exists());
    assert_eq!(fs::read(f.path().join("tracked")).unwrap(), b"changed\n");
}

#[test]
fn recovery_worktree_redirected_placement_and_unsafe_files_are_read_only_errors() {
    use std::os::unix::fs::symlink;
    for redirect in [false, true] {
        let f = Fixture::new();
        if redirect {
            let path = f.path();
            let retained = f.root.join("retained");
            fs::rename(&path, &retained).unwrap();
            symlink(&retained, &path).unwrap();
        } else {
            symlink(f.root.join("repository/tracked"), f.path().join("link")).unwrap();
        }
        assert!(f.inspect().is_err());
        assert_eq!(fs::read(f.path().join("tracked")).unwrap(), b"base\n");
    }
}
