//! Actual store/claim -> original subreaper receipt -> pinned SQLite inspector.
//! No waiting-owner handoff, writable resume or recovery grant is exercised.
use super::*;
use forge_app::durable_execution::recovery::quiescence::Operation;
use forge_runner_host::{control::ControlPipe, inspection::ExclusiveInspection};
use forge_server::durable_authorization::quiesced_inspection;

pub(super) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

pub(super) fn prepared(mode: &str) -> (Fixture, ControlPipe, Operation) {
    let components = effect_admission::components();
    let mut f = Fixture::with_component(
        "retire-delayed",
        effect_admission::component_hash(&components),
    );
    let (pipe, operation, _) = quiescence::prepare_admission(&mut f, Some(mode));
    (f, pipe, operation)
}

pub(super) fn quiesce(f: &mut Fixture, pipe: &mut ControlPipe, operation: &Operation) {
    pipe.quiesce(operation).unwrap();
    diagnostic(&f.errors, "runner-quiescence-verified");
    assert!(f.child.wait().unwrap().success());
}

#[test]
fn quiesced_inspection_combines_real_receipt_and_pending_input_under_one_lock() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = runtime();
    for mode in ["queued", "placed"] {
        let (mut f, mut pipe, operation) = prepared(mode);
        assert!(
            runtime
                .block_on(quiesced_inspection::inspect(
                    &f.store,
                    "test",
                    &operation.claim,
                    &components.node,
                    &inspector,
                ))
                .is_err()
        );
        let dirty = f.root.join("worktree/dirty-progress");
        fs::write(&dirty, "uncommitted progress, not adopted evidence").unwrap();
        quiesce(&mut f, &mut pipe, &operation);
        let before = recovery_inspection::runtime_bytes(&f.directory);
        let owner = fs::read(f.directory.join("owner.json")).unwrap();
        let info = f.store.agent_assignment_info("assignment").unwrap();
        let resources = f
            .store
            .list_agent_assignment_resources("assignment")
            .unwrap();
        for _ in 0..2 {
            let store = open(&f.database);
            let guard = runtime
                .block_on(quiesced_inspection::inspect(
                    &store,
                    "test",
                    &operation.claim,
                    &components.node,
                    &inspector,
                ))
                .unwrap();
            assert_eq!(guard.candidate().last_lease, operation.predecessor);
            assert_eq!(guard.input().admission_id, guard.candidate().admission_id);
            guard.verify(&store, "test").unwrap();
            assert!(guard.verify(&store, "foreign").is_err());
            assert!(ExclusiveInspection::acquire(&f.directory).is_err());
            assert!(
                runtime
                    .block_on(quiesced_inspection::inspect(
                        &store,
                        "test",
                        &operation.claim,
                        &components.node,
                        &inspector,
                    ))
                    .is_err()
            );
            assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
            assert_eq!(fs::read(f.directory.join("owner.json")).unwrap(), owner);
            assert_eq!(
                fs::read_to_string(&dirty).unwrap(),
                "uncommitted progress, not adopted evidence"
            );
            assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), info);
            assert_eq!(
                f.store
                    .list_agent_assignment_resources("assignment")
                    .unwrap(),
                resources
            );
            assert!(info.capacity_reserved());
            drop(guard);
        }
    }
}

#[test]
fn quiesced_inspection_rejects_missing_changed_or_unsafe_receipt_without_repair() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = runtime();
    for fault in 0..5 {
        let (mut f, mut pipe, operation) = prepared("queued");
        quiesce(&mut f, &mut pipe, &operation);
        let receipt = f.directory.join("recovery-quiescence-receipt.json");
        match fault {
            0 => fs::remove_file(&receipt).unwrap(),
            1 => fs::write(&receipt, "PRIVATE-CANARY invalid proof").unwrap(),
            2 => fs::set_permissions(&receipt, fs::Permissions::from_mode(0o644)).unwrap(),
            3 => {
                let mut changed = serde_json::to_value(&operation).unwrap();
                changed["claim"]["operation_id"] = "foreign".into();
                fs::write(
                    f.directory.join("recovery-quiescence-intent.json"),
                    changed.to_string(),
                )
                .unwrap();
            }
            4 => fs::write(f.directory.join("watchdog.json"), "retained host fault").unwrap(),
            _ => unreachable!(),
        }
        let before = recovery_inspection::runtime_bytes(&f.directory);
        let proof_before = fs::read(&receipt).ok();
        let info = f.store.agent_assignment_info("assignment").unwrap();
        let result = runtime.block_on(quiesced_inspection::inspect(
            &f.store,
            "test",
            &operation.claim,
            &components.node,
            &inspector,
        ));
        let Err(error) = result else {
            panic!("unsafe proof accepted")
        };
        assert!(!error.message.contains("PRIVATE-CANARY"));
        assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
        assert_eq!(fs::read(&receipt).ok(), proof_before);
        assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), info);
        assert!(info.capacity_reserved());
    }
}

#[test]
fn quiesced_inspection_blocks_settled_input_and_interrupted_effects_despite_process_proof() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = runtime();
    for mode in ["done", "interrupted"] {
        let (mut f, mut pipe, operation) = prepared(mode);
        quiesce(&mut f, &mut pipe, &operation);
        let before = recovery_inspection::runtime_bytes(&f.directory);
        let info = f.store.agent_assignment_info("assignment").unwrap();
        assert!(
            runtime
                .block_on(quiesced_inspection::inspect(
                    &f.store,
                    "test",
                    &operation.claim,
                    &components.node,
                    &inspector,
                ))
                .is_err()
        );
        assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
        assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), info);
        assert!(info.capacity_reserved());
    }
}

#[test]
fn quiesced_inspection_revalidates_claim_context_and_receipt_after_external_io() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = runtime();
    for fault in 0..3 {
        let (mut f, mut pipe, operation) = prepared("queued");
        quiesce(&mut f, &mut pipe, &operation);
        let wrapper = f.root.join("mutating-inspector.mjs");
        let mutation = match fault {
            0 => "db.exec(\"UPDATE agent_assignment_resources SET retired_at_millis = 9 WHERE resource_kind = 'recovery-claim'\");".into(),
            1 => "db.exec(\"UPDATE record_attempts SET attempt_json = json_set(attempt_json, '$.body', 'changed during inspection')\");".into(),
            2 => format!("unlinkSync({});", serde_json::json!(f.directory.join("recovery-quiescence-receipt.json"))),
            _ => unreachable!(),
        };
        // The actual matched read-only inspector produces a valid reply first;
        // then controlled external mutation makes that observation stale.
        fs::write(&wrapper, format!(r"
import {{readFileSync, unlinkSync}} from 'node:fs';
import {{spawnSync}} from 'node:child_process';
import {{DatabaseSync}} from 'node:sqlite';
const result = spawnSync(process.execPath, [{inspector}, process.argv[2]], {{input: readFileSync(0), env: {{}}}});
if (result.status !== 0) process.exit(1);
const db = new DatabaseSync({database});
{mutation}
db.close();
process.stdout.write(result.stdout);
", inspector = serde_json::json!(inspector), database = serde_json::json!(f.database))).unwrap();
        let before = recovery_inspection::runtime_bytes(&f.directory);
        assert!(
            runtime
                .block_on(quiesced_inspection::inspect(
                    &f.store,
                    "test",
                    &operation.claim,
                    &components.node,
                    &wrapper,
                ))
                .is_err()
        );
        assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
        assert!(
            f.store
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
    }
}

#[test]
fn quiesced_inspection_guard_rechecks_live_claim_and_protected_proof() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = runtime();
    for fault in 0..3 {
        let (mut f, mut pipe, operation) = prepared("queued");
        quiesce(&mut f, &mut pipe, &operation);
        let guard = runtime
            .block_on(quiesced_inspection::inspect(
                &f.store,
                "test",
                &operation.claim,
                &components.node,
                &inspector,
            ))
            .unwrap();
        match fault {
            0 => {
                f.store
                    .close_agent_assignment(
                        "assignment",
                        forge_app::AgentAssignmentClosureReason::Retired,
                    )
                    .unwrap();
            }
            1 => {
                rusqlite::Connection::open(&f.database).unwrap().execute(
                    "UPDATE agent_assignment_resources SET retired_at_millis = 9 WHERE resource_kind = 'recovery-claim'", [],
                ).unwrap();
            }
            2 => fs::remove_file(f.directory.join("recovery-quiescence-receipt.json")).unwrap(),
            _ => unreachable!(),
        }
        assert!(guard.verify(&open(&f.database), "test").is_err());
        assert!(ExclusiveInspection::acquire(&f.directory).is_err());
        assert!(
            f.store
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
    }
}
