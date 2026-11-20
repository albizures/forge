//! Actual Forge mapping/store + kernel lock + matched Node read-only inspector.
//! Stopping our ungranted fixture is test teardown, not product recovery proof.
use super::*;
use forge_app::durable_execution::Admission;
use forge_rpc::durable_execution::recovery_inspection::PendingStatus;
use forge_runner_host::inspection::ExclusiveInspection;
use forge_server::durable_authorization::recovery_inspection::inspect_exclusive;
use std::io::Write;

pub(super) fn seed(f: &Fixture, mode: &str) -> Value {
    let components = effect_admission::components();
    // Resolve the explicit test component, never production PATH discovery.
    let script = components
        .inspector
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test/recovery-inspection-fixture.mjs");
    let mut child = Command::new(&components.node)
        .env_clear()
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            &serde_json::to_vec(&serde_json::json!({
                "directory": f.directory, "launch": f.launch, "mode": mode,
            }))
            .unwrap(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let identity: Value = serde_json::from_slice(&output.stdout).unwrap();
    f.store.begin_send("test", &f.launch).unwrap();
    f.store
        .record_result(
            "test",
            &f.launch,
            &Admission::Accepted {
                admission_id: identity["admission_id"].as_str().unwrap().into(),
            },
        )
        .unwrap();
    identity
}

fn stop_ungranted(f: &mut Fixture) {
    // We own these never-authorized fixture processes. No external children or
    // actions have been admitted. This is not a production cleanup certificate.
    let pid = u32::try_from(f.evidence["worker_pid"].as_u64().unwrap()).unwrap();
    rustix::process::kill_process_group(
        rustix::process::Pid::from_raw(pid.cast_signed()).unwrap(),
        rustix::process::Signal::KILL,
    )
    .unwrap();
    diagnostic(&f.errors, "runner-worker-exited");
    f.child.kill().unwrap();
    f.child.wait().unwrap();
}

pub(super) fn runtime_bytes(directory: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![];
    for name in ["runtime.sqlite", "runtime.sqlite-wal"] {
        if let Ok(bytes) = fs::read(directory.join(name))
            && !bytes.is_empty()
        {
            files.push((name.into(), bytes));
        }
    }
    files
}

#[test]
fn canonical_recovery_inspection_preserves_original_input_and_holds_exclusive_lock() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for mode in ["queued", "placed"] {
        let mut f = Fixture::with_component("model", effect_admission::component_hash(&components));
        let identity = seed(&f, mode);
        let admission = identity["admission_id"].as_str().unwrap();
        let inspect = || {
            inspect_exclusive(
                &f.store,
                "test",
                &f.launch,
                admission,
                &components.node,
                &inspector,
            )
        };
        assert!(
            runtime.block_on(inspect()).is_err(),
            "live owner must fence inspection"
        );
        stop_ungranted(&mut f);
        let before = runtime_bytes(&f.directory);
        let attempt = f.store.assignment_check_attempt("assignment").unwrap();
        let info = f.store.agent_assignment_info("assignment").unwrap();
        for _ in 0..2 {
            let guard = runtime
                .block_on(inspect_exclusive(
                    &open(&f.database),
                    "test",
                    &f.launch,
                    admission,
                    &components.node,
                    &inspector,
                ))
                .unwrap();
            assert_eq!(guard.input().root, identity["root"].as_u64().unwrap());
            assert_eq!(guard.input().admission_id, admission);
            assert_eq!(
                guard.input().status,
                if mode == "queued" {
                    PendingStatus::Queued
                } else {
                    PendingStatus::Placed
                }
            );
            guard.verify().unwrap();
            assert!(ExclusiveInspection::acquire(&f.directory).is_err());
            assert!(
                runtime
                    .block_on(inspect_exclusive(
                        &f.store,
                        "test",
                        &f.launch,
                        admission,
                        &components.node,
                        &inspector
                    ))
                    .is_err()
            );
            drop(guard);
            assert!(ExclusiveInspection::acquire(&f.directory).is_ok());
            assert_eq!(runtime_bytes(&f.directory), before);
            assert_eq!(
                f.store.assignment_check_attempt("assignment").unwrap(),
                attempt
            );
            assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), info);
        }
        // Historical observation is deliberately not live authority: revocation
        // does not make it a grant or require source profiles/provider credentials.
        f.store
            .close_agent_assignment(
                "assignment",
                forge_app::AgentAssignmentClosureReason::Retired,
            )
            .unwrap();
        let guard = runtime
            .block_on(inspect_exclusive(
                &f.store,
                "test",
                &f.launch,
                admission,
                &components.node,
                &inspector,
            ))
            .unwrap();
        guard.verify().unwrap();
        assert_eq!(runtime_bytes(&f.directory), before);
        let leases: i64 = rusqlite::Connection::open(&f.database)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM assignment_authorization_leases",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(leases, 0);
    }
}

#[test]
fn recovery_inspection_rejects_foreign_scope_bindings_components_effects_and_done_input() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for mode in ["queued", "done", "interrupted"] {
        let mut f = Fixture::with_component("model", effect_admission::component_hash(&components));
        let identity = seed(&f, mode);
        stop_ungranted(&mut f);
        let before = runtime_bytes(&f.directory);
        let info = f.store.agent_assignment_info("assignment").unwrap();
        let admission = identity["admission_id"].as_str().unwrap();
        if mode == "queued" {
            let mut foreign = f.launch.clone();
            foreign.incarnation_id = "foreign".into();
            for (workspace, launch, admission, node, inspector) in [
                (
                    "foreign",
                    &f.launch,
                    admission,
                    components.node.as_path(),
                    inspector.as_path(),
                ),
                (
                    "test",
                    &foreign,
                    admission,
                    components.node.as_path(),
                    inspector.as_path(),
                ),
                (
                    "test",
                    &f.launch,
                    "foreign",
                    components.node.as_path(),
                    inspector.as_path(),
                ),
                (
                    "test",
                    &f.launch,
                    admission,
                    Path::new("node"),
                    inspector.as_path(),
                ),
                (
                    "test",
                    &f.launch,
                    admission,
                    Path::new("/missing-node"),
                    inspector.as_path(),
                ),
                (
                    "test",
                    &f.launch,
                    admission,
                    components.node.as_path(),
                    Path::new("/missing-inspector"),
                ),
                (
                    "test",
                    &f.launch,
                    admission,
                    components.node.as_path(),
                    components.inspector.as_path(),
                ),
            ] {
                assert!(
                    runtime
                        .block_on(inspect_exclusive(
                            &f.store, workspace, launch, admission, node, inspector
                        ))
                        .is_err()
                );
                assert!(ExclusiveInspection::acquire(&f.directory).is_ok());
            }
        } else {
            let result = runtime.block_on(inspect_exclusive(
                &f.store,
                "test",
                &f.launch,
                admission,
                &components.node,
                &inspector,
            ));
            assert!(
                matches!(result, Err(ref e) if e.code == "durable-recovery-inspection-unavailable")
            );
        }
        assert_eq!(runtime_bytes(&f.directory), before);
        assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), info);
        assert!(info.capacity_reserved());
    }
}

#[test]
fn recovery_inspection_revalidates_admission_after_external_observation() {
    let components = effect_admission::components();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut f = Fixture::with_component("model", effect_admission::component_hash(&components));
    let identity = seed(&f, "queued");
    stop_ungranted(&mut f);
    let inspector = f.root.join("mutating-inspector.mjs");
    // Trusted transport fault fixture: change Forge's binding during the external
    // observation, then return an otherwise valid response for the old input.
    fs::write(&inspector, format!(r#"
import {{readFileSync}} from 'node:fs';
import {{DatabaseSync}} from 'node:sqlite';
const request = JSON.parse(readFileSync(0, 'utf8'));
const db = new DatabaseSync({database});
db.prepare("UPDATE durable_assignment_runtimes SET admission_json = ? WHERE assignment_id = ?").run(JSON.stringify({{state: 'accepted', admission_id: 'changed'}}), request.launch.assignment_id);
db.close();
console.log(JSON.stringify({{protocol: 1, launch: request.launch, candidate: {identity}}}));
"#, database = serde_json::json!(f.database), identity = serde_json::json!({
        "root": identity["root"], "admission_id": identity["admission_id"], "status": "queued",
    }))).unwrap();
    let before = runtime_bytes(&f.directory);
    let result = runtime.block_on(inspect_exclusive(
        &f.store,
        "test",
        &f.launch,
        identity["admission_id"].as_str().unwrap(),
        &components.node,
        &inspector,
    ));
    assert!(matches!(result, Err(ref e) if e.code == "durable-recovery-inspection-unavailable"));
    assert!(ExclusiveInspection::acquire(&f.directory).is_ok());
    assert_eq!(runtime_bytes(&f.directory), before);
    assert!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}

#[test]
fn recovery_inspection_guard_detects_replaced_ownership_evidence() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut f = Fixture::with_component("model", effect_admission::component_hash(&components));
    let identity = seed(&f, "queued");
    stop_ungranted(&mut f);
    let guard = runtime
        .block_on(inspect_exclusive(
            &f.store,
            "test",
            &f.launch,
            identity["admission_id"].as_str().unwrap(),
            &components.node,
            &inspector,
        ))
        .unwrap();
    fs::rename(
        f.directory.join("owner.lock"),
        f.directory.join("original.lock"),
    )
    .unwrap();
    fs::write(f.directory.join("owner.lock"), []).unwrap();
    fs::set_permissions(
        f.directory.join("owner.lock"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(guard.verify().is_err());
    assert!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
}
