//! Original subreaper crash proof -> reopened Forge -> fresh claim/new owner ->
//! protected real verification -> committed lease -> exact original input resume.
//! Fake model/runtime configuration port; not a live provider/bootstrap demo.
use super::*;
use forge_app::durable_execution::recovery::{Claim, Repository as _};
use forge_runner_host::control::ControlPipe;
use forge_server::durable_authorization::waiting_recovery_inspection::inspect;

#[test]
fn surviving_descendant_prevents_crash_receipt_and_preserves_capacity() {
    let mut f = Fixture::new("retire-delayed");
    let authority = DaemonAuthority::new().unwrap();
    let mut service = authority
        .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
        .unwrap();
    service.tick(&f.store).unwrap();
    event(&f.output, "authorized");
    let started = event(&f.output, "started");
    let pipe = service.retirement_control();
    drop(service);
    let worker = rustix::process::Pid::from_raw(
        i32::try_from(f.evidence["worker_pid"].as_u64().unwrap()).unwrap(),
    )
    .unwrap();
    rustix::process::kill_process(worker, rustix::process::Signal::KILL).unwrap();
    diagnostic(&f.errors, "runner-worker-exited");
    let absent = !f.directory.join("recovery-crash-1.json").exists();
    let retained = f.child.try_wait().unwrap().is_none();
    // Test teardown of the deliberately surviving, fixture-created child only.
    let child =
        rustix::process::Pid::from_raw(i32::try_from(started["child"].as_u64().unwrap()).unwrap())
            .unwrap();
    rustix::process::kill_process(child, rustix::process::Signal::KILL).unwrap();
    drop(pipe);
    assert!(absent);
    assert!(retained);
    assert!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}

pub(super) fn crashed() -> (Fixture, Value, PathBuf) {
    let components = effect_admission::components();
    let mut f =
        Fixture::with_repository("model", effect_admission::component_hash(&components), true);
    let path = forge_app::managed_worktree_path(&f.root, "alpha", f.record);
    let authority = DaemonAuthority::new().unwrap();
    let mut service = authority
        .bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
        .unwrap();
    service.tick(&f.store).unwrap();
    event(&f.output, "authorized");
    event(&f.output, "started");
    let original_pipe = service.retirement_control();
    drop(service);
    let original = recovery_inspection::seed(&f, "queued-agent");
    let c = rusqlite::Connection::open(&f.database).unwrap();
    c.execute(
        "INSERT INTO assignment_execution_bindings VALUES ('assignment', ?1)",
        [path.to_str().unwrap()],
    )
    .unwrap();
    c.execute("INSERT INTO worktree_claims SELECT record_id, project_slug, ?1, 'active' FROM agent_assignments WHERE id = 'assignment'", [path.to_str().unwrap()]).unwrap();
    drop(c);
    let worker = rustix::process::Pid::from_raw(
        i32::try_from(f.evidence["worker_pid"].as_u64().unwrap()).unwrap(),
    )
    .unwrap();
    rustix::process::kill_process(worker, rustix::process::Signal::KILL).unwrap();
    let mut diagnostics = vec![];
    loop {
        let line = f
            .errors
            .recv_timeout(Duration::from_secs(40))
            .unwrap_or_else(|error| panic!("crash proof {error:?}: {diagnostics:?}"));
        let verified = line.contains("runner-crash-verified");
        diagnostics.push(line);
        if verified {
            break;
        }
    }
    assert!(f.child.wait().unwrap().success());
    drop(original_pipe);
    // A fresh store/epoch reads original identities, not an adopted PID or lease.
    f.store = open(&f.database);
    (f, original, path)
}

#[test]
fn recovery_after_owned_worker_death_and_store_restart_resumes_original_attempt() {
    let (mut f, original, path) = crashed();
    let components = effect_admission::components();
    let candidate = f.store.recovery_candidate("test", &f.launch).unwrap();
    let attempt = candidate.attempt.clone();
    let claim = Claim::for_candidate(
        &candidate,
        "after-crash".into(),
        "new-daemon-after-crash".into(),
    )
    .unwrap();
    f.store.claim_recovery("test", &claim).unwrap();
    let runner = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../runner/test/recovered-check-worker.mjs")
        .canonicalize()
        .unwrap();
    let (child, output, errors) = spawn_component(&f.directory, &path, &runner);
    f.child = child;
    f.output = output;
    f.errors = errors;
    event(&f.output, "owned");
    let mut pipe = ControlPipe::take(&mut f.child).unwrap();
    let runtime = quiesced_inspection::runtime();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let handoff = runtime.block_on(async {
        inspect(
            &f.store,
            "test",
            &claim,
            &mut f.child,
            &components.node,
            &inspector,
        )
        .await
        .unwrap()
        .inspect_progress(&f.store, "test", &components.node, &inspector)
        .await
        .unwrap()
        .prepare(&f.store)
        .await
        .unwrap()
    });
    let lease = handoff.prepared().grant().lease.clone();
    let mut active = runtime
        .block_on(handoff.start(&f.store, &mut pipe))
        .unwrap();
    event(&f.output, "recovery-prepared");
    event(&f.output, "authorized");
    assert_eq!(
        event(&f.output, "resumed")["admission_id"],
        original["admission_id"]
    );
    event(&f.output, "recovered-model");
    assert_eq!(event(&f.output, "settled")["status"], "done");
    assert_eq!(
        f.store
            .assignment_check_attempt("assignment")
            .unwrap()
            .unwrap(),
        attempt
    );
    let committed: String = rusqlite::Connection::open(&f.database)
        .unwrap()
        .query_row(
            "SELECT lease_json FROM assignment_authorization_leases ORDER BY sequence DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&committed).unwrap(),
        serde_json::to_value(lease).unwrap()
    );
    assert!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    // Completion prose is never an accepted Outcome or a fresh Check attempt.
    let db = rusqlite::Connection::open(&f.database).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM record_attempts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    active.stop(&f.store).unwrap();
}
