//! T16: operator rollback of NEW durable launches beside a live, accepted Assignment.
//! The original Assignment keeps its backend for status and retirement; nothing falls
//! back to Herdr (the controlled adapter panics on any Herdr call).
use super::{
    AgentDispatchRepository, Command, ManagedWorktreeRepository, Ordering, RecordRepository,
    SqliteProjectRepository, SupervisedDispatch,
};
use forge_app::{AgentDispatchAdapterFactory, AgentProfile};

fn cli(adapter: &SupervisedDispatch, args: &[&str]) -> std::process::Output {
    Command::new(&adapter.cli)
        .env_clear()
        .arg("dispatcher")
        .args(args)
        .args([
            "--json",
            "--server-url",
            &format!("{}/rpc", adapter.endpoint),
        ])
        .output()
        .unwrap()
}

fn status(adapter: &SupervisedDispatch) -> forge_rpc::DurableLaunchStatus {
    let output = cli(adapter, &["durable-launch-status"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Record the rollback through the actual CLI/RPC while the original is still open.
pub(super) fn disable(
    adapter: &SupervisedDispatch,
    record: u64,
    assignment: &str,
    capability: &str,
    profile: &AgentProfile,
) {
    use forge_rpc::DurableLaunchState as State;
    let before = status(adapter);
    assert_eq!(before.state, State::Withheld, "operator view never grants");
    assert!(!before.new_launches_admitted && before.rollback.is_none());
    // The verification composition admits durable launches before rollback.
    assert!(adapter.app().validate_record_check_profiles(record).is_ok());
    let output = cli(
        adapter,
        &[
            "durable-launch-disable",
            "--operation-id",
            "rollback-1",
            "--reason",
            "T16 staged rollback",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let disabled: forge_rpc::DurableLaunchStatus = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(disabled.state, State::Disabled);
    assert!(!disabled.replayed && !disabled.new_launches_admitted);
    let retry = cli(
        adapter,
        &[
            "durable-launch-disable",
            "--operation-id",
            "rollback-1",
            "--reason",
            "T16 staged rollback",
        ],
    );
    let retry: forge_rpc::DurableLaunchStatus = serde_json::from_slice(&retry.stdout).unwrap();
    assert!(retry.replayed);
    assert_eq!(retry.rollback, disabled.rollback);
    assert_eq!(status(adapter).rollback, disabled.rollback);
    // The gate, not the inner provider, refuses: the same profile still passes the
    // controlled provider's own preflight.
    assert!(adapter.validate_preflight(profile).is_ok());
    assert!(
        adapter
            .app()
            .validate_record_check_profiles(record)
            .is_err()
    );
    // Historical inspection of the original durable Assignment stays available.
    let inspected = cli(
        adapter,
        &["assignment-status", assignment, "--workspace", "test"],
    );
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let text = String::from_utf8(inspected.stdout).unwrap();
    assert!(text.contains(assignment));
    assert!(!text.contains(capability));
}

/// After the original retired through its own host, a new durable launch is still
/// refused across a fresh application/store open and no Assignment is created.
pub(super) fn refused_after_retirement(
    adapter: &SupervisedDispatch,
    store: &SqliteProjectRepository,
    replacement: u64,
) {
    for app in [adapter.app(), adapter.app()] {
        assert!(app.dispatch_record(replacement).is_err());
    }
    let reopened = SqliteProjectRepository::open(&adapter.database).unwrap();
    assert!(
        reopened
            .list_agent_assignments(replacement)
            .unwrap()
            .is_empty()
    );
    assert!(store.worktree_claim(replacement).unwrap().is_none());
    assert_eq!(adapter.provisions.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.show_record(replacement).unwrap().status,
        forge_app::RecordStatus::Open
    );
}
