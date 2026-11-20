//! Resolution adapter composition over the actual interrupted Bash-store fixture.
//! The lock is a labelled fixture, not supervised retirement or process-death proof.
use super::{git, node};
use forge_app::{
    AgentAssignmentClosureReason, AgentDispatchRepository,
    durable_execution::{
        Launch,
        effect_resolution::{ExternalOutcome, Resolution, ResolutionReceipt},
        effects::InterruptedEffect,
    },
};
use forge_server::durable_authorization::effects::resolve_exclusive;
use forge_store::SqliteProjectRepository;
use std::{path::Path, process::Stdio};
use tokio::{io::AsyncWriteExt, process::Command};

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    database: Vec<u8>,
    status: String,
    index: String,
    tracked: Vec<u8>,
    untracked: Vec<u8>,
}
impl Snapshot {
    fn read(runtime: &Path, checkout: &Path) -> Self {
        Self {
            database: std::fs::read(runtime.join("runtime.sqlite")).unwrap(),
            status: git(checkout, &["status", "--porcelain=v1"]),
            index: git(checkout, &["show", ":tracked.txt"]),
            tracked: std::fs::read(checkout.join("tracked.txt")).unwrap(),
            untracked: std::fs::read(checkout.join("untracked.txt")).unwrap(),
        }
    }
}

pub(super) async fn verify_resolution(
    store: &SqliteProjectRepository,
    launch: &Launch,
    runtime: &Path,
    checkout: &Path,
    inspector: &Path,
    endpoint: &str,
) -> ResolutionReceipt {
    let before = store.agent_assignment_info(&launch.assignment_id).unwrap();
    let resource = before
        .resources
        .iter()
        .find(|r| r.resource_kind == "interrupted-effect")
        .unwrap();
    let effect: InterruptedEffect = serde_json::from_str(&resource.resource_id).unwrap();
    let resolution = Resolution {
        operation_id: "inspected-publication".into(),
        effect,
        outcome: ExternalOutcome::Applied,
        actor: "controlled-test-operator".into(),
        evidence: "Controlled endpoint reported exactly one accepted mutation; test worker group killed/reaped and endpoint stopped".into(),
        message: "Resolve only this external effect; no replay or runtime receipt repair".into(),
        confirm_old_agent_cannot_act: true,
        confirm_external_work_safe: true,
    };
    let snapshot = Snapshot::read(runtime, checkout);
    verify_wire_denials(endpoint, &launch.assignment_id, &resolution).await;
    verify_denials(store, launch, runtime, inspector, endpoint, &resolution).await;
    verify_operator_denials(store, launch, runtime, endpoint, &resolution).await;
    let receipt = verify_acceptance(store, launch, runtime, inspector, endpoint, &resolution).await;
    assert_eq!(Snapshot::read(runtime, checkout), snapshot);
    verify_unsettled_runtime(runtime, inspector, launch, &resolution).await;
    inspect_audit(endpoint, &launch.assignment_id).await;
    receipt
}

async fn verify_denials(
    store: &SqliteProjectRepository,
    launch: &Launch,
    runtime: &Path,
    inspector: &Path,
    endpoint: &str,
    resolution: &Resolution,
) {
    let before = store.agent_assignment_info(&launch.assignment_id).unwrap();
    // Acquiring the trusted lock is mandatory, even with explicit assertions.
    let owner = forge_runner_host::inspection::ExclusiveInspection::acquire(runtime).unwrap();
    assert!(
        resolve_exclusive(store, "test", launch, &node(), inspector, resolution)
            .await
            .is_err()
    );
    assert_eq!(
        store.agent_assignment_info(&launch.assignment_id).unwrap(),
        before
    );
    drop(owner);
    assert_eq!(
        resolve_exclusive(store, "test", launch, &node(), inspector, resolution)
            .await
            .unwrap_err()
            .code,
        "durable-effect-resolution-revocation-required"
    );
    assert_eq!(
        store.agent_assignment_info(&launch.assignment_id).unwrap(),
        before
    );
    let output = cli_command(endpoint, &launch.assignment_id, resolution, Some("test"))
        .args([
            "--confirm-old-agent-cannot-act",
            "--confirm-external-work-safe",
            "--json",
        ])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        store.agent_assignment_info(&launch.assignment_id).unwrap(),
        before
    );
    store
        .close_agent_assignment(&launch.assignment_id, AgentAssignmentClosureReason::Retired)
        .unwrap();
    let revoked = store.agent_assignment_info(&launch.assignment_id).unwrap();
    let absent = Path::new("/missing-node");
    assert!(
        resolve_exclusive(store, "foreign", launch, absent, inspector, resolution)
            .await
            .is_err()
    );
    let mut foreign = launch.clone();
    foreign.incarnation_id = "foreign".into();
    assert!(
        resolve_exclusive(store, "test", &foreign, absent, inspector, resolution)
            .await
            .is_err()
    );
    let mut unconfirmed = resolution.clone();
    unconfirmed.confirm_external_work_safe = false;
    assert!(
        resolve_exclusive(store, "test", launch, &node(), inspector, &unconfirmed)
            .await
            .is_err()
    );
    assert_eq!(
        store.agent_assignment_info(&launch.assignment_id).unwrap(),
        revoked
    );
}

async fn verify_acceptance(
    store: &SqliteProjectRepository,
    launch: &Launch,
    runtime: &Path,
    inspector: &Path,
    endpoint: &str,
    resolution: &Resolution,
) -> ResolutionReceipt {
    let revoked = store.agent_assignment_info(&launch.assignment_id).unwrap();
    let output = cli_command(endpoint, &launch.assignment_id, resolution, Some("test"))
        .args([
            "--confirm-old-agent-cannot-act",
            "--confirm-external-work-safe",
            "--json",
        ])
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: ResolutionReceipt = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        resolve_exclusive(store, "test", launch, &node(), inspector, resolution)
            .await
            .unwrap(),
        receipt
    );
    let after = store.agent_assignment_info(&launch.assignment_id).unwrap();
    let mut conflict = resolution.clone();
    conflict.outcome = ExternalOutcome::NotApplied;
    assert!(
        resolve_exclusive(store, "test", launch, &node(), inspector, &conflict)
            .await
            .is_err()
    );
    assert_eq!(
        store.agent_assignment_info(&launch.assignment_id).unwrap(),
        after
    );
    assert_eq!(after.dispatch_hold, revoked.dispatch_hold);
    assert!(after.capacity_reserved());
    assert!(
        !after
            .recovery_blockers()
            .contains(&"interrupted-effect".into())
    );
    let owner = forge_runner_host::inspection::ExclusiveInspection::acquire(runtime).unwrap();
    verify_replay(endpoint, &launch.assignment_id, &receipt).await;
    drop(owner);
    let output = cli_command(endpoint, &launch.assignment_id, &conflict, Some("test"))
        .args([
            "--confirm-old-agent-cannot-act",
            "--confirm-external-work-safe",
            "--json",
        ])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        store.agent_assignment_info(&launch.assignment_id).unwrap(),
        after
    );
    receipt
}

async fn verify_unsettled_runtime(
    runtime: &Path,
    inspector: &Path,
    launch: &Launch,
    resolution: &Resolution,
) {
    // Committed runtime intent is deliberately STILL unsettled. No accepted
    // runtime result or automatic-recovery permission is manufactured by audit.
    let input = serde_json::to_vec(&serde_json::json!({"protocol": 1, "launch": launch})).unwrap();
    let mut child = Command::new(node())
        .env_clear()
        .arg(inspector)
        .arg(runtime)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&input).await.unwrap();
    let observed = child.wait_with_output().await.unwrap();
    assert!(observed.status.success());
    let observed: serde_json::Value = serde_json::from_slice(&observed.stdout).unwrap();
    assert!(
        observed["effects"]
            .as_array()
            .unwrap()
            .contains(&serde_json::to_value(&resolution.effect).unwrap())
    );
}

fn cli_command(
    endpoint: &str,
    assignment: &str,
    resolution: &Resolution,
    workspace: Option<&str>,
) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
    command.env_clear().current_dir("/").args([
        "dispatcher",
        "assignment-resolve-effect",
        assignment,
        "--server-url",
        &format!("{endpoint}/rpc"),
        "--operation-id",
        &resolution.operation_id,
        "--execution-task",
        &resolution.effect.task_id,
        "--conversation",
        &resolution.effect.conversation_id,
        "--call",
        &resolution.effect.call_id,
        "--outcome",
        match resolution.outcome {
            ExternalOutcome::Applied => "applied",
            ExternalOutcome::NotApplied => "not-applied",
        },
        "--actor",
        &resolution.actor,
        "--evidence",
        &resolution.evidence,
        "--message",
        &resolution.message,
    ]);
    // Operator commands must not read or forward ambient Assignment credentials.
    command
        .env("FORGE_ASSIGNMENT_ID", "AGENT-CREDENTIAL-CANARY")
        .env(
            "FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE",
            "/missing-AGENT-CREDENTIAL-CANARY",
        );
    if let Some(workspace) = workspace {
        command.args(["--workspace", workspace]);
    }
    command
}

async fn verify_operator_denials(
    store: &SqliteProjectRepository,
    launch: &Launch,
    runtime: &Path,
    endpoint: &str,
    resolution: &Resolution,
) {
    let before = store.agent_assignment_info(&launch.assignment_id).unwrap();
    for flags in [
        vec![],
        vec!["--confirm-old-agent-cannot-act"],
        vec!["--confirm-external-work-safe"],
    ] {
        let output = cli_command(
            "http://127.0.0.1:1",
            &launch.assignment_id,
            resolution,
            Some("test"),
        )
        .args(flags)
        .arg("--json")
        .output()
        .await
        .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("both --confirm")
        );
    }
    let owner = forge_runner_host::inspection::ExclusiveInspection::acquire(runtime).unwrap();
    let output = cli_command(endpoint, &launch.assignment_id, resolution, Some("test"))
        .args([
            "--confirm-old-agent-cannot-act",
            "--confirm-external-work-safe",
            "--json",
        ])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    drop(owner);
    for workspace in [None, Some("foreign")] {
        let output = cli_command(endpoint, &launch.assignment_id, resolution, workspace)
            .args([
                "--confirm-old-agent-cannot-act",
                "--confirm-external-work-safe",
                "--json",
            ])
            .output()
            .await
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let mut unconfirmed = resolution.clone();
    unconfirmed.confirm_old_agent_cannot_act = false;
    assert!(
        forge_cli::request_resolve_interrupted_effect(
            (format!("{endpoint}/rpc").as_str(), Some("test")),
            forge_rpc::ResolveInterruptedEffectRequest {
                assignment_id: launch.assignment_id.clone(),
                resolution: unconfirmed
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        store.agent_assignment_info(&launch.assignment_id).unwrap(),
        before
    );
}

pub(super) async fn verify_replay(endpoint: &str, assignment: &str, receipt: &ResolutionReceipt) {
    let response = forge_cli::request_resolve_interrupted_effect(
        (format!("{endpoint}/rpc").as_str(), Some("test")),
        forge_rpc::ResolveInterruptedEffectRequest {
            assignment_id: assignment.into(),
            resolution: receipt.resolution.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(response, *receipt);
    for json in [true, false] {
        let mut command = cli_command(endpoint, assignment, &receipt.resolution, Some("test"));
        command.args([
            "--confirm-old-agent-cannot-act",
            "--confirm-external-work-safe",
        ]);
        if json {
            command.arg("--json");
        }
        let output = command.output().await.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("AGENT-CREDENTIAL-CANARY"));
        if json {
            assert_eq!(
                serde_json::from_str::<ResolutionReceipt>(&text).unwrap(),
                *receipt
            );
        } else {
            assert!(text.contains("No command replay or recovery is authorized"));
        }
    }
}

pub(super) async fn verify_unconfigured(
    endpoint: &str,
    assignment: &str,
    receipt: &ResolutionReceipt,
) {
    let mut fresh = receipt.resolution.clone();
    fresh.operation_id = "unaccepted-operation".into();
    fresh.effect.task_id = "unaccepted-task".into();
    let output = cli_command(endpoint, assignment, &fresh, Some("test"))
        .args([
            "--confirm-old-agent-cannot-act",
            "--confirm-external-work-safe",
            "--json",
        ])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("configure trusted matched")
    );
}

async fn verify_wire_denials(endpoint: &str, assignment: &str, resolution: &Resolution) {
    use jsonrpsee::{core::client::ClientT, http_client::HttpClientBuilder, rpc_params};
    let client = HttpClientBuilder::default()
        .build(format!("{endpoint}/rpc"))
        .unwrap();
    for field in [
        "secret",
        "capability_id",
        "node",
        "inspector",
        "runtime_directory",
        "launch",
    ] {
        let mut value = serde_json::json!({"selection": {"workspace": "test"}, "record": {"assignment_id": assignment, "resolution": resolution}});
        value["record"][field] = "AGENT-CREDENTIAL-CANARY".into();
        let error = client
            .request::<serde_json::Value, _>(
                "agentDispatch_assignmentResolveEffect",
                rpc_params![value],
            )
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("AGENT-CREDENTIAL-CANARY"));
    }
    let mut value = serde_json::json!({"selection": {"workspace": "test"}, "record": {"assignment_id": assignment, "resolution": resolution}});
    value["record"]["resolution"]["outcome"] = "unknown".into();
    assert!(
        client
            .request::<serde_json::Value, _>(
                "agentDispatch_assignmentResolveEffect",
                rpc_params![value]
            )
            .await
            .is_err()
    );
}

async fn inspect_audit(endpoint: &str, assignment: &str) {
    // Ordinary Assignment info still exposes the original audit.
    for json_output in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forge"));
        command.env_clear().args([
            "dispatcher",
            "assignment-info",
            assignment,
            "--workspace",
            "test",
            "--server-url",
            &format!("{endpoint}/rpc"),
        ]);
        if json_output {
            command.arg("--json");
        }
        let output = command.output().await.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("interrupted-effect-resolution"));
        assert!(text.contains("controlled-test-operator"));
    }
}
