//! New directly spawned, ungranted owner; old host proof stays a separate fact.
use super::*;
use forge_app::durable_execution::recovery::quiescence::Operation;
use forge_runner_host::inspection::ExclusiveInspection;
use forge_server::durable_authorization::waiting_recovery_inspection;

fn waiting(mode: &str) -> (Fixture, Operation) {
    waiting_runner(mode, None)
}

pub(super) fn waiting_runner(mode: &str, runner: Option<&Path>) -> (Fixture, Operation) {
    let (mut f, mut pipe, operation) = quiesced_inspection::prepared(mode);
    quiesced_inspection::quiesce(&mut f, &mut pipe, &operation);
    let (child, output, errors) = if let Some(runner) = runner {
        spawn_component(&f.directory, &f.root.join("worktree"), runner)
    } else {
        spawn(&f.directory, &f.root.join("worktree"))
    };
    f.child = child;
    f.output = output;
    f.errors = errors;
    f.evidence = event(&f.output, "owned")["identity"].clone();
    (f, operation)
}

#[test]
fn waiting_inspection_rechecks_real_process_proof_runtime_and_claim_under_new_owner() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = quiesced_inspection::runtime();
    for mode in ["queued", "placed"] {
        let (mut f, operation) = waiting(mode);
        let before = recovery_inspection::runtime_bytes(&f.directory);
        let info = f.store.agent_assignment_info("assignment").unwrap();
        let resources = f
            .store
            .list_agent_assignment_resources("assignment")
            .unwrap();
        let attempt = f.store.assignment_check_attempt("assignment").unwrap();
        let mut inspection = runtime
            .block_on(waiting_recovery_inspection::inspect(
                &f.store,
                "test",
                &operation.claim,
                &mut f.child,
                &components.node,
                &inspector,
            ))
            .unwrap();
        let observed = inspection.observe(&f.store, "test").unwrap();
        assert_ne!(
            observed.owner.host_pid,
            operation.predecessor.owner.host_pid
        );
        assert_ne!(
            observed.owner.worker_pid,
            operation.predecessor.owner.worker_pid
        );
        assert_eq!(
            observed.owner.lock_inode,
            operation.predecessor.owner.lock_inode
        );
        assert_eq!(inspection.candidate().last_lease, operation.predecessor);
        assert_eq!(
            inspection.input().admission_id,
            inspection.candidate().admission_id
        );
        assert!(ExclusiveInspection::acquire(&f.directory).is_err());
        assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
        assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), info);
        assert_eq!(
            f.store
                .list_agent_assignment_resources("assignment")
                .unwrap(),
            resources
        );
        assert_eq!(
            f.store.assignment_check_attempt("assignment").unwrap(),
            attempt
        );
        assert!(
            !f.output
                .try_iter()
                .any(|line| line.contains("authorized") || line.contains("started"))
        );
        drop(inspection);
        // Dropping daemon proof does not unlock, kill or retire a waiting host.
        assert!(f.child.try_wait().unwrap().is_none());
        assert!(ExclusiveInspection::acquire(&f.directory).is_err());
        assert!(info.capacity_reserved());
    }
}

#[test]
fn waiting_inspection_rejects_effects_settled_inputs_and_unavailable_components() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = quiesced_inspection::runtime();
    for mode in ["done", "interrupted", "queued"] {
        let (mut f, operation) = waiting(mode);
        let before = recovery_inspection::runtime_bytes(&f.directory);
        let selected = if mode == "queued" {
            Path::new("/missing-inspector")
        } else {
            inspector.as_path()
        };
        assert!(
            runtime
                .block_on(waiting_recovery_inspection::inspect(
                    &f.store,
                    "test",
                    &operation.claim,
                    &mut f.child,
                    &components.node,
                    selected,
                ))
                .is_err()
        );
        assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
        assert!(ExclusiveInspection::acquire(&f.directory).is_err());
        assert!(
            f.store
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
    }
}

#[test]
fn waiting_inspection_loss_of_receipt_authority_or_owner_terminally_fences() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = quiesced_inspection::runtime();
    for fault in 0..4 {
        let (mut f, operation) = waiting("queued");
        let mut inspection = runtime
            .block_on(waiting_recovery_inspection::inspect(
                &f.store,
                "test",
                &operation.claim,
                &mut f.child,
                &components.node,
                &inspector,
            ))
            .unwrap();
        let receipt = f.directory.join("recovery-quiescence-receipt.json");
        let proof = fs::read(&receipt).unwrap();
        let owner = fs::read(f.directory.join("owner.json")).unwrap();
        match fault {
            0 => fs::remove_file(&receipt).unwrap(),
            1 => {
                f.store
                    .close_agent_assignment(
                        "assignment",
                        forge_app::AgentAssignmentClosureReason::Retired,
                    )
                    .unwrap();
            }
            2 => {
                fs::write(f.directory.join("watchdog.json"), "PRIVATE-CANARY").unwrap();
            }
            3 => fs::write(f.directory.join("owner.json"), "{}").unwrap(),
            _ => unreachable!(),
        }
        assert!(inspection.observe(&f.store, "test").is_err());
        if fault == 0 {
            fs::write(&receipt, &proof).unwrap();
            fs::set_permissions(&receipt, fs::Permissions::from_mode(0o600)).unwrap();
        } else if fault == 2 {
            fs::remove_file(f.directory.join("watchdog.json")).unwrap();
        } else if fault == 3 {
            fs::write(f.directory.join("owner.json"), &owner).unwrap();
        }
        assert!(inspection.observe(&f.store, "test").is_err());
        assert!(
            f.store
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
    }
}

#[test]
fn waiting_inspection_rejects_foreign_proof_and_live_candidate_drift() {
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = quiesced_inspection::runtime();
    for fault in 0..3 {
        let (mut f, operation) = waiting("queued");
        match fault {
            0 => fs::remove_file(f.directory.join("recovery-quiescence-receipt.json")).unwrap(),
            1 => {
                let mut foreign = serde_json::to_value(&operation).unwrap();
                foreign["claim"]["operation_id"] = "foreign".into();
                fs::write(
                    f.directory.join("recovery-quiescence-intent.json"),
                    foreign.to_string(),
                )
                .unwrap();
            }
            2 => {
                rusqlite::Connection::open(&f.database).unwrap().execute(
                    "UPDATE record_attempts SET attempt_json = json_set(attempt_json, '$.body', 'changed')", [],
                ).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            runtime
                .block_on(waiting_recovery_inspection::inspect(
                    &f.store,
                    "test",
                    &operation.claim,
                    &mut f.child,
                    &components.node,
                    &inspector,
                ))
                .is_err()
        );
        assert!(
            f.store
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
        assert!(
            !f.output
                .try_iter()
                .any(|line| line.contains("authorized") || line.contains("started"))
        );
    }
}
