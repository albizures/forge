//! Real protected verification and commit, not the earlier synthetic digest host.
use super::*;
use forge_runner_host::control::ControlPipe;
use forge_server::durable_authorization::waiting_recovery_inspection::inspect;

#[test]
fn trusted_handoff_persists_proof_and_commits_before_executable_delivery() {
    let runner = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../runner/test/synthetic-runner.ts")
        .canonicalize()
        .unwrap();
    let (mut f, claim, path) = recovery_progress::waiting_with_runner(Some(&runner));
    fs::write(path.join("tracked"), "retained implementation\n").unwrap();
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = quiesced_inspection::runtime();
    let mut pipe = ControlPipe::take(&mut f.child).unwrap();
    let attempt = f.store.assignment_check_attempt("assignment").unwrap();
    let bytes = recovery_inspection::runtime_bytes(&f.directory);
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
    let grant = handoff.prepared().grant().clone();
    let proofs: Vec<_> = fs::read_dir(&f.directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("recovery-proof-")
        })
        .collect();
    assert_eq!(proofs.len(), 1);
    assert_eq!(
        fs::metadata(&proofs[0]).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let proof: Value = serde_json::from_slice(&fs::read(&proofs[0]).unwrap()).unwrap();
    assert_eq!(proof["claim"], serde_json::to_value(&claim).unwrap());
    assert_eq!(proof["progress"]["dirty"], true);
    assert_eq!(
        proof["waiting"]["owner"],
        serde_json::to_value(&grant.lease.owner).unwrap()
    );
    let mut active = runtime
        .block_on(handoff.start(&f.store, &mut pipe))
        .unwrap();
    event(&f.output, "recovery-prepared");
    assert_eq!(
        event(&f.output, "authorized")["sequence"],
        grant.lease.sequence
    );
    assert_eq!(
        active.tick(&f.store, &mut pipe).unwrap(),
        Tick::Waiting {
            next_renewal_millis: grant.lease.issued_boottime_millis + 10_000
        }
    );
    assert_eq!(
        f.store.assignment_check_attempt("assignment").unwrap(),
        attempt
    );
    assert_eq!(recovery_inspection::runtime_bytes(&f.directory), bytes);
    assert_eq!(
        fs::read_to_string(path.join("tracked")).unwrap(),
        "retained implementation\n"
    );
    active.stop(&f.store).unwrap();
}

#[test]
fn trusted_handoff_denies_changed_protected_proof_or_revocation_before_delivery() {
    let runner = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../runner/test/synthetic-runner.ts")
        .canonicalize()
        .unwrap();
    for fault in 0..3 {
        let (mut f, claim, path) = recovery_progress::waiting_with_runner(Some(&runner));
        let components = effect_admission::components();
        let inspector = components.inspector.with_file_name("inspect-recovery.ts");
        let runtime = quiesced_inspection::runtime();
        let mut pipe = ControlPipe::take(&mut f.child).unwrap();
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
        match fault {
            0 => {
                let proof = fs::read_dir(&f.directory)
                    .unwrap()
                    .map(|e| e.unwrap().path())
                    .find(|p| {
                        p.file_name()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .starts_with("recovery-proof-")
                    })
                    .unwrap();
                fs::write(proof, "PRIVATE-CANARY").unwrap();
            }
            1 => {
                f.store
                    .close_agent_assignment(
                        "assignment",
                        forge_app::AgentAssignmentClosureReason::Retired,
                    )
                    .unwrap();
            }
            2 => fs::write(path.join("tracked"), "changed since proof\n").unwrap(),
            _ => unreachable!(),
        }
        assert!(
            runtime
                .block_on(handoff.start(&f.store, &mut pipe))
                .is_err()
        );
        assert_eq!(
            rusqlite::Connection::open(&f.database)
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM assignment_authorization_leases",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
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
                .any(|l| l.contains("authorized") || l.contains("PRIVATE-CANARY"))
        );
    }
}
