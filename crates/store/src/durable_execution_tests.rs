use super::*;
use forge_app::{
    AgentDispatchRepository, NewAgentAssignment, NewTypedRecord, Project, ProjectRepository,
    RecordRepository, RecordType, WorkspaceRepository,
    durable_execution::{Transport, admit},
    outcomes::Attempt,
};
use std::sync::{
    Arc, Barrier, Mutex,
    atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: std::path::PathBuf,
    store: SqliteProjectRepository,
    launch: Launch,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "forge-durable-admission-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let store = SqliteProjectRepository::open(&root.join("forge.sqlite")).unwrap();
        store.create_workspace("test", "Test", &root).unwrap();
        let store = store.in_record_workspace("test").unwrap();
        store
            .register(&Project {
                reference: "alpha".into(),
                workspace_id: "test".into(),
                slug: "alpha".into(),
                project_root: root.clone(),
                origin_url: "https://example.test/repo.git".into(),
            })
            .unwrap();
        // T07 will provide the dispatch tracer. This fixture models its bound claim,
        // without registering a launchable durable profile in production.
        store.connection.lock().unwrap().execute("INSERT INTO agent_profiles(record_type, revision, provider, agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis) SELECT record_type, 2, 'pi-durable', agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis FROM agent_profiles WHERE record_type = 'task' AND revision = 1", []).unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "work".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "assignment".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 2,
            })
            .unwrap();
        let check = store.list_record_checks(task.id).unwrap().remove(0);
        let attempt = Attempt {
            id: "attempt".into(),
            record_id: task.id,
            check_id: check.id,
            generation: 1,
            evidence: forge_app::outcomes::Evidence::Repository {
                repository: "https://example.test/repo.git".into(),
                commit: "a".repeat(40),
            },
            actor: "assignment".into(),
            body: "work".into(),
            continuation: None,
            recovery: None,
        };
        {
            let c = store.connection.lock().unwrap();
            c.execute("INSERT INTO record_attempts(id, record_id, check_id, attempt_json) VALUES ('attempt', ?1, ?2, ?3)", params![task.id.cast_signed(), check.id.cast_signed(), serde_json::to_string(&attempt).unwrap()]).unwrap();
            c.execute(
                "INSERT INTO assignment_check_attempts VALUES ('assignment', 'attempt')",
                [],
            )
            .unwrap();
        }
        let launch = Launch {
            assignment_id: "assignment".into(),
            operation_id: "launch".into(),
            attempt_id: "attempt".into(),
            profile_revision: 2,
            runtime_id: "runtime".into(),
            incarnation_id: "incarnation".into(),
            conversation_id: "conversation".into(),
            store_id: "store".into(),
            forge_revision: "test-revision".into(),
            component_hash: "pinned-component".into(),
            runtime_schema: 2,
        };
        Self {
            root,
            store,
            launch,
        }
    }
    fn reopen(&self) -> SqliteProjectRepository {
        SqliteProjectRepository::open(&self.root.join("forge.sqlite"))
            .unwrap()
            .in_record_workspace("test")
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn authorization_failure_retains_atomic_scoped_history_after_closure_and_reopen() {
    use forge_app::authorization_lease::session::{Failure, SessionRepository as _};
    let f = Fixture::new();
    f.store.reserve("test", &f.launch).unwrap();
    let record = f
        .store
        .agent_assignment_info("assignment")
        .unwrap()
        .assignment
        .record_id;
    f.store
        .hold_dispatch(record, "preserve unrelated hold")
        .unwrap();
    f.store
        .close_agent_assignment(
            "assignment",
            forge_app::AgentAssignmentClosureReason::Crashed,
        )
        .unwrap();
    f.store
        .retain_failure("test", &f.launch, Failure::DeliveryUncertain)
        .unwrap();
    let reopened = f.reopen();
    let resources = reopened
        .list_agent_assignment_resources("assignment")
        .unwrap();
    reopened
        .retain_failure("test", &f.launch, Failure::DeliveryUncertain)
        .unwrap();
    assert_eq!(
        reopened
            .list_agent_assignment_resources("assignment")
            .unwrap(),
        resources
    );
    assert!(resources.iter().any(
        |resource| resource.resource_kind == "authorization-uncertain"
            && resource.resource_id == "authorization-delivery-uncertain"
            && resource.retired_at_millis.is_none()
    ));
    assert_eq!(
        reopened.dispatch_hold(record).unwrap().unwrap().reason,
        "preserve unrelated hold"
    );
    assert!(
        reopened
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    assert!(
        reopened
            .claim_next_agent_assignment("alpha", "replacement")
            .unwrap()
            .is_none()
    );
    assert!(
        reopened
            .retain_failure("foreign", &f.launch, Failure::Stopped)
            .is_err()
    );
    let mut changed = f.launch.clone();
    changed.store_id = "foreign".into();
    assert!(
        reopened
            .retain_failure("test", &changed, Failure::Stopped)
            .is_err()
    );
    assert_eq!(
        reopened
            .list_agent_assignment_resources("assignment")
            .unwrap(),
        resources
    );
}

#[test]
fn authorization_failure_resource_and_hold_rollback_together() {
    use forge_app::authorization_lease::session::{Failure, SessionRepository as _};
    let f = Fixture::new();
    f.store.reserve("test", &f.launch).unwrap();
    f.store.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_authorization_hold BEFORE INSERT ON dispatch_holds BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").unwrap();
    assert!(
        f.store
            .retain_failure("test", &f.launch, Failure::LeaseExpired)
            .is_err()
    );
    assert!(
        !f.store
            .list_agent_assignment_resources("assignment")
            .unwrap()
            .iter()
            .any(|resource| resource.resource_kind == "authorization-uncertain")
    );
    f.store
        .connection
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_authorization_hold")
        .unwrap();
    f.store
        .retain_failure("test", &f.launch, Failure::LeaseExpired)
        .unwrap();
    assert!(
        f.reopen()
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    let record = f
        .store
        .agent_assignment_info("assignment")
        .unwrap()
        .assignment
        .record_id;
    assert!(f.reopen().dispatch_hold(record).unwrap().is_some());
}

fn lease(f: &Fixture) -> forge_app::authorization_lease::Lease {
    use forge_app::authorization_lease::{Lease, Owner};
    Lease {
        launch: f.launch.clone(),
        generation: 1,
        owner: Owner {
            boot_id: "test-boot".into(),
            time_namespace: "time:[1]".into(),
            host_pid: 10,
            host_start: "100".into(),
            worker_pid: 11,
            worker_start: "101".into(),
            lock_device: "1".into(),
            lock_inode: "2".into(),
        },
        daemon_epoch: "daemon-lifetime-1".into(),
        sequence: 1,
        issued_boottime_millis: 1_000,
        expires_boottime_millis: 31_000,
    }
}

#[test]
fn lease_history_is_atomic_idempotent_and_not_restart_authority() {
    use forge_app::authorization_lease::Repository as _;
    let f = Fixture::new();
    f.store.reserve("test", &f.launch).unwrap();
    let first = lease(&f);
    f.store.grant_lease("test", &first, 1_000).unwrap();
    f.reopen().grant_lease("test", &first, 1_001).unwrap();
    let mut renewed = first.clone();
    renewed.sequence = 2;
    renewed.issued_boottime_millis += 10_000;
    renewed.expires_boottime_millis += 10_000;
    f.reopen().grant_lease("test", &renewed, 11_000).unwrap();
    let mut changed = renewed.clone();
    changed.daemon_epoch = "restarted-daemon".into();
    assert_eq!(
        f.reopen()
            .grant_lease("test", &changed, 11_001)
            .unwrap_err()
            .code,
        "assignment-lease-denied"
    );
    assert_eq!(
        f.reopen()
            .grant_lease("test", &first, 11_001)
            .unwrap_err()
            .code,
        "assignment-lease-denied"
    );
    assert_eq!(
        f.reopen()
            .grant_lease("test", &renewed, 41_000)
            .unwrap_err()
            .code,
        "assignment-lease-denied"
    );
    let c = f.store.connection.lock().unwrap();
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM assignment_authorization_leases",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert!(
        c.execute("DELETE FROM assignment_authorization_leases", [])
            .is_err()
    );
    assert!(
        c.execute(
            "UPDATE assignment_authorization_leases SET lease_json = '{}'",
            []
        )
        .is_err()
    );
    assert_eq!(c.query_row("SELECT count(*) FROM agent_assignment_resources WHERE retired_at_millis IS NULL AND resource_kind = 'durable-runtime'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn lease_issuer_reauthorizes_against_reopened_store_and_retains_revoked_capacity() {
    use forge_app::authorization_lease::{AuthorityHost, Issuer, Observation, Owner};
    struct Host {
        owner: Owner,
        now: u64,
        calls: usize,
    }
    impl AuthorityHost for Host {
        fn observe(&mut self) -> Result<Observation, RegistrationError> {
            self.calls += 1;
            Ok(Observation {
                owner: self.owner.clone(),
                boottime_millis: self.now,
            })
        }
    }
    let f = Fixture::new();
    f.store.reserve("test", &f.launch).unwrap();
    let mut host = Host {
        owner: lease(&f).owner,
        now: 1_000,
        calls: 0,
    };
    let mut issuer = Issuer::new("test".into(), f.launch.clone(), 1, "first-epoch".into()).unwrap();
    assert_eq!(issuer.issue(&f.store, &mut host).unwrap().sequence, 1);
    host.now = 11_000;
    assert_eq!(issuer.issue(&f.reopen(), &mut host).unwrap().sequence, 2);
    // A new daemon cannot continue from retained history, even before expiry.
    let mut restarted =
        Issuer::new("test".into(), f.launch.clone(), 1, "new-epoch".into()).unwrap();
    assert!(restarted.issue(&f.reopen(), &mut host).is_err());
    f.store.connection.lock().unwrap().execute(
        "UPDATE agent_assignments SET closed_at_millis = 1, closure_reason = 'retired' WHERE id = 'assignment'",
        [],
    ).unwrap();
    host.now = 21_000;
    assert!(issuer.issue(&f.reopen(), &mut host).is_err());
    let calls = host.calls;
    assert!(issuer.issue(&f.reopen(), &mut host).is_err());
    assert_eq!(host.calls, calls);
    let c = f.store.connection.lock().unwrap();
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM assignment_authorization_leases",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(c.query_row("SELECT count(*) FROM agent_assignment_resources WHERE retired_at_millis IS NULL AND resource_kind = 'durable-runtime'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn lease_grants_revalidate_authority_before_exact_retry() {
    use forge_app::authorization_lease::Repository as _;
    for denial in [
        "scope",
        "binding",
        "stale",
        "closed",
        "attempt-closed",
        "held",
        "resource",
        "state",
    ] {
        let f = Fixture::new();
        f.store.reserve("test", &f.launch).unwrap();
        let grant = lease(&f);
        f.store.grant_lease("test", &grant, 1_000).unwrap();
        let mut changed = grant.clone();
        let workspace = if denial == "scope" { "other" } else { "test" };
        {
            let c = f.store.connection.lock().unwrap();
            match denial {
                "scope" => {}
                "binding" => changed.launch.profile_revision += 1,
                "stale" => {
                    c.execute("INSERT INTO record_work(record_id, generation) SELECT record_id, 2 FROM agent_assignments WHERE id = 'assignment' ON CONFLICT(record_id) DO UPDATE SET generation = 2", []).unwrap();
                }
                "closed" => {
                    c.execute("UPDATE agent_assignments SET closed_at_millis = 1, closure_reason = 'retired' WHERE id = 'assignment'", []).unwrap();
                }
                "attempt-closed" => {
                    c.execute(
                        "UPDATE record_attempts SET closed = 1 WHERE id = 'attempt'",
                        [],
                    )
                    .unwrap();
                }
                "held" => {
                    c.execute("INSERT INTO dispatch_holds(record_id, reason) SELECT record_id, 'test' FROM agent_assignments WHERE id = 'assignment'", []).unwrap();
                }
                "resource" => {
                    c.execute("INSERT INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) VALUES ('assignment', 'retirement-pending', 'test', 1)", []).unwrap();
                }
                "state" => {
                    c.execute(
                        "UPDATE agent_assignments SET state = 'unknown' WHERE id = 'assignment'",
                        [],
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
        }
        assert!(
            f.reopen().grant_lease(workspace, &changed, 1_001).is_err(),
            "{denial}"
        );
        assert_eq!(
            f.store
                .connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM assignment_authorization_leases",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
    }
}

#[test]
fn concurrent_lease_renewals_cannot_fork_history_or_extend_an_expired_grant() {
    use forge_app::authorization_lease::Repository as _;
    let f = Fixture::new();
    f.store.reserve("test", &f.launch).unwrap();
    let first = lease(&f);
    f.store.grant_lease("test", &first, 1_000).unwrap();
    let mut next = first.clone();
    next.sequence = 2;
    next.issued_boottime_millis = 11_000;
    next.expires_boottime_millis = 41_000;
    let mut conflict = next.clone();
    conflict.issued_boottime_millis += 1;
    conflict.expires_boottime_millis += 1;
    let barrier = Arc::new(Barrier::new(2));
    let results = std::thread::scope(|s| {
        let a = f.reopen();
        let b = f.reopen();
        let left_barrier = Arc::clone(&barrier);
        let left = s.spawn(move || {
            left_barrier.wait();
            a.grant_lease("test", &next, 11_001)
        });
        let right = s.spawn(move || {
            barrier.wait();
            b.grant_lease("test", &conflict, 11_001)
        });
        [left.join().unwrap(), right.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let c = f.store.connection.lock().unwrap();
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM assignment_authorization_leases",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
}

#[derive(Default)]
struct FakeTransport {
    calls: Mutex<Vec<Launch>>,
    accepted: Mutex<Option<Admission>>,
    drop_reply: bool,
}
impl Transport for FakeTransport {
    fn admit(&self, launch: &Launch) -> Result<Admission, RegistrationError> {
        self.calls.lock().unwrap().push(launch.clone());
        let result = Admission::Accepted {
            admission_id: "original".into(),
        };
        *self.accepted.lock().unwrap() = Some(result.clone());
        if self.drop_reply {
            Err(error("dropped-reply"))
        } else {
            Ok(result)
        }
    }
    fn observe_admission(&self, _: &Launch) -> Result<Admission, RegistrationError> {
        Ok(self
            .accepted
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Admission::Unknown))
    }
}

#[test]
fn durable_admission_replays_exact_result_and_rejects_scope_and_payload_conflicts() {
    let f = Fixture::new();
    let transport = FakeTransport::default();
    let result = admit(&f.store, &transport, "test", &f.launch).unwrap();
    assert_eq!(
        admit(&f.reopen(), &transport, "test", &f.launch).unwrap(),
        result
    );
    assert_eq!(transport.calls.lock().unwrap().len(), 1);
    assert_eq!(
        f.store
            .reserve("wrong-workspace", &f.launch)
            .unwrap_err()
            .code,
        "durable-scope-denied"
    );
    let mut conflict = f.launch.clone();
    conflict.conversation_id = "other".into();
    assert_eq!(
        f.store.reserve("test", &conflict).unwrap_err().code,
        "durable-operation-conflict"
    );
    assert_eq!(
        f.store
            .record_result(
                "test",
                &f.launch,
                &Admission::Rejected {
                    code: "different".into()
                }
            )
            .unwrap_err()
            .code,
        "durable-result-conflict"
    );
    assert_eq!(
        f.reopen().runtime("test", "assignment").unwrap(),
        Some(result)
    );
}

#[test]
fn durable_admission_dropped_reply_is_observed_not_resubmitted_after_restart() {
    let f = Fixture::new();
    let transport = FakeTransport {
        drop_reply: true,
        ..Default::default()
    };
    assert_eq!(
        admit(&f.store, &transport, "test", &f.launch)
            .unwrap_err()
            .code,
        "dropped-reply"
    );
    assert_eq!(
        f.reopen()
            .runtime("test", "assignment")
            .unwrap()
            .unwrap()
            .admission,
        Admission::Unknown
    );
    assert!(matches!(
        admit(&f.reopen(), &transport, "test", &f.launch)
            .unwrap()
            .admission,
        Admission::Accepted { .. }
    ));
    assert_eq!(transport.calls.lock().unwrap().len(), 1);
}

#[test]
fn durable_admission_interruptions_before_send_and_before_result_commit_retain_intent() {
    let f = Fixture::new();
    f.store.reserve("test", &f.launch).unwrap();
    assert_eq!(
        f.reopen()
            .runtime("test", "assignment")
            .unwrap()
            .unwrap()
            .admission,
        Admission::Pending
    );
    assert!(f.reopen().begin_send("test", &f.launch).unwrap());
    // Crash before the request leaves Forge: no remote result can be inferred.
    let transport = FakeTransport::default();
    assert_eq!(
        admit(&f.reopen(), &transport, "test", &f.launch)
            .unwrap()
            .admission,
        Admission::Unknown
    );
    assert!(transport.calls.lock().unwrap().is_empty());
    // Remote commit succeeds, then Forge loses its local result commit.
    transport.admit(&f.launch).unwrap();
    assert!(matches!(
        admit(&f.reopen(), &transport, "test", &f.launch)
            .unwrap()
            .admission,
        Admission::Accepted { .. }
    ));
    assert_eq!(transport.calls.lock().unwrap().len(), 1);
}

#[test]
fn durable_admission_concurrent_claims_have_one_identity_and_one_initial_sender() {
    let f = Fixture::new();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = f.reopen();
            let launch = f.launch.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.reserve("test", &launch).unwrap();
                store.begin_send("test", &launch).unwrap()
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| usize::from(h.join().unwrap()))
            .sum::<usize>(),
        1
    );
    let c = f.store.connection.lock().unwrap();
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM durable_assignment_runtimes",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn durable_admission_closed_unknown_reserves_capacity_and_preserves_historical_backend() {
    let f = Fixture::new();
    f.store.reserve("test", &f.launch).unwrap();
    f.store.begin_send("test", &f.launch).unwrap();
    f.store
        .close_agent_assignment(
            "assignment",
            forge_app::AgentAssignmentClosureReason::Crashed,
        )
        .unwrap();
    let reopened = f.reopen();
    let info = reopened.agent_assignment_info("assignment").unwrap();
    assert!(info.capacity_reserved());
    assert!(
        reopened
            .claim_next_agent_assignment("alpha", "replacement")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        reopened.begin_send("test", &f.launch).unwrap_err().code,
        "durable-binding-denied"
    );
    assert_eq!(
        reopened
            .runtime("test", "assignment")
            .unwrap()
            .unwrap()
            .backend,
        Backend::PiDurable
    );
    assert!(
        reopened
            .retire_agent_assignment_resource("assignment", "durable-runtime", "runtime")
            .is_err()
    );
    // Late result records history without reopening authorization or releasing capacity.
    reopened
        .record_result(
            "test",
            &f.launch,
            &Admission::Accepted {
                admission_id: "late".into(),
            },
        )
        .unwrap();
    assert!(
        reopened
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}

#[test]
fn durable_admission_intent_and_result_commit_failures_are_atomic() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let f = Fixture::new();
    // Fail between identity insert and its capacity reservation.
    f.store
        .connection
        .lock()
        .unwrap()
        .authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::Insert {
                table_name: "agent_assignment_resources",
            } => Authorization::Deny,
            _ => Authorization::Allow,
        }))
        .unwrap();
    assert!(f.store.reserve("test", &f.launch).is_err());
    f.store
        .connection
        .lock()
        .unwrap()
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert!(f.reopen().runtime("test", "assignment").unwrap().is_none());
    f.store.reserve("test", &f.launch).unwrap();
    f.store.begin_send("test", &f.launch).unwrap();
    let transport = FakeTransport::default();
    let accepted = transport.admit(&f.launch).unwrap();
    // Remote result exists, but the local writer cannot commit it.
    f.store
        .connection
        .lock()
        .unwrap()
        .commit_hook(Some(|| true))
        .unwrap();
    assert!(f.store.record_result("test", &f.launch, &accepted).is_err());
    f.store
        .connection
        .lock()
        .unwrap()
        .commit_hook(None::<fn() -> bool>)
        .unwrap();
    assert_eq!(
        f.reopen()
            .runtime("test", "assignment")
            .unwrap()
            .unwrap()
            .admission,
        Admission::Unknown
    );
    assert_eq!(
        admit(&f.reopen(), &transport, "test", &f.launch)
            .unwrap()
            .admission,
        accepted
    );
    assert_eq!(transport.calls.lock().unwrap().len(), 1);
}

#[test]
fn durable_admission_rejected_result_is_stable_and_external_calls_have_no_writer_lock() {
    struct Reject<'a>(&'a SqliteProjectRepository);
    impl Transport for Reject<'_> {
        fn admit(&self, _: &Launch) -> Result<Admission, RegistrationError> {
            assert!(
                self.0.connection.try_lock().is_ok(),
                "external call inside writer transaction"
            );
            Ok(Admission::Rejected {
                code: "ownership-unavailable".into(),
            })
        }
        fn observe_admission(&self, _: &Launch) -> Result<Admission, RegistrationError> {
            panic!("accepted rejection must replay locally")
        }
    }
    let f = Fixture::new();
    let transport = Reject(&f.store);
    let rejected = admit(&f.store, &transport, "test", &f.launch).unwrap();
    assert_eq!(
        admit(&f.reopen(), &transport, "test", &f.launch).unwrap(),
        rejected
    );
    assert!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}

#[test]
fn durable_admission_binding_changes_and_conflicting_concurrent_reservations_fail_closed() {
    let f = Fixture::new();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|i| {
            let store = f.reopen();
            let mut launch = f.launch.clone();
            let barrier = barrier.clone();
            launch.conversation_id = format!("conversation-{i}");
            std::thread::spawn(move || {
                barrier.wait();
                store.reserve("test", &launch)
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().find_map(|r| r.as_ref().err()).unwrap().code,
        "durable-operation-conflict"
    );
    let original = f.store.runtime("test", "assignment").unwrap().unwrap();
    // Mutable current selection cannot replace a historical runtime binding.
    forge_app::WorkspaceDispatchRepository::select_workspace_profile(
        &f.store,
        "test",
        RecordType::Task,
        Some(1),
    )
    .unwrap();
    assert_eq!(
        f.reopen().runtime("test", "assignment").unwrap(),
        Some(original.clone())
    );
    let mut wrong_attempt = original.launch.clone();
    wrong_attempt.attempt_id = "another-attempt".into();
    assert_eq!(
        f.store.reserve("test", &wrong_attempt).unwrap_err().code,
        "durable-binding-denied"
    );
    f.store.connection.lock().unwrap().execute("INSERT INTO record_work(record_id, generation) SELECT record_id, 2 FROM agent_assignments WHERE id = 'assignment' ON CONFLICT(record_id) DO UPDATE SET generation = 2", []).unwrap();
    assert_eq!(
        f.store
            .begin_send("test", &original.launch)
            .unwrap_err()
            .code,
        "durable-binding-stale"
    );
}

#[test]
fn durable_admission_incompatible_store_rejects_without_writes() {
    let f = Fixture::new();
    let path = f.root.join("incompatible.sqlite");
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES ('unchanged'); PRAGMA application_id=1179603527; PRAGMA user_version=19;").unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        SqliteProjectRepository::open(&path).err().unwrap().code,
        "fresh-store-required"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
