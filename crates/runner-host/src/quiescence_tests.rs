use super::*;
use forge_app::durable_execution::recovery::quiescence::Operation;

fn operation() -> Operation {
    let control: forge_rpc::durable_execution::SupervisorRecovery =
        serde_json::from_str(include_str!("../../../runner/test/recovery-control.json")).unwrap();
    let mut operation = Operation {
        claim: control.grant.claim,
        predecessor: control.grant.lease,
    };
    operation.predecessor.sequence = 1;
    operation.predecessor.daemon_epoch = "original-daemon".into();
    operation
}
fn armed(operation: &Operation) -> State {
    let mut state = State::new(operation.predecessor.owner.clone(), false);
    state
        .policy
        .accept(operation.predecessor.clone(), 1000)
        .unwrap();
    state
}

#[test]
fn quiescence_requires_an_exact_current_grant_and_preserves_first_shutdown() {
    let op = operation();
    let mut unarmed = State::new(op.predecessor.owner.clone(), false);
    assert!(
        unarmed
            .request_shutdown(Shutdown::Quiescence(Box::new(op.clone())))
            .is_err()
    );
    assert!(unarmed.shutdown_denied);
    let mut state = armed(&op);
    let shutdown = Shutdown::Quiescence(Box::new(op.clone()));
    state.request_shutdown(shutdown.clone()).unwrap();
    state.request_shutdown(shutdown.clone()).unwrap();
    assert_eq!(state.policy.last_lease(), Some(&op.predecessor));
    assert!(state.policy.check(1001).is_err());
    let mut foreign = op;
    foreign.claim.operation_id.push('x');
    assert!(
        state
            .request_shutdown(Shutdown::Quiescence(Box::new(foreign)))
            .is_err()
    );
    assert!(state.shutdown.as_ref() == Some(&shutdown));
    assert!(state.request_shutdown(shutdown).is_err());
    assert!(state.shutdown_denied);
}

#[test]
fn stale_grants_retained_markers_prior_faults_and_shutdown_kind_conflicts_cannot_stop_again() {
    let op = operation();
    for fault in 0..5 {
        let mut state = armed(&op);
        match fault {
            0 => {
                let mut renewal = op.predecessor.clone();
                renewal.sequence += 1;
                renewal.issued_boottime_millis += 10_000;
                renewal.expires_boottime_millis += 10_000;
                state.policy.accept(renewal, 11_000).unwrap();
            }
            1 => state.retained_blocker = true,
            2 => state.fence("prior-uncertainty"),
            3 => state.owner.worker_start.push('x'),
            _ => {
                state
                    .request_shutdown(Shutdown::Retirement(Box::new(
                        forge_app::durable_execution::retirement::Operation {
                            operation_id: "retire".into(),
                            launch: op.claim.launch.clone(),
                            owner: op.predecessor.owner.clone(),
                        },
                    )))
                    .unwrap();
            }
        }
        assert!(
            state
                .request_shutdown(Shutdown::Quiescence(Box::new(op.clone())))
                .is_err()
        );
        assert!(state.shutdown_denied);
    }
}
