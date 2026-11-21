use super::*;
use forge_app::{
    assignment::BoundIdentity,
    authorization_lease::Lease,
    durable_profiles::ExecutionConfiguration,
    outcomes::{Attempt, Evidence},
};
use forge_rpc::durable_execution::{CHECK_RUNTIME_VERSION, CheckCapability};
use std::os::unix::fs::{MetadataExt, PermissionsExt};

fn input(directory: &Path) -> CheckRuntimeInput {
    let lease: Lease = serde_json::from_str(include_str!(
        "../../../runner/test/authorization-lease.json"
    ))
    .unwrap();
    let mut configuration: ExecutionConfiguration =
        serde_json::from_str(include_str!("../../../runner/test/profile-v1.json")).unwrap();
    configuration.cwd = "/bound/worktree".into();
    let attempt = Attempt {
        id: lease.launch.attempt_id.clone(),
        record_id: 7,
        check_id: 3,
        generation: lease.generation,
        evidence: Evidence::Repository {
            repository: "repository".into(),
            commit: "a".repeat(40),
        },
        actor: lease.launch.assignment_id.clone(),
        body: "bound task".into(),
        continuation: None,
        recovery: None,
    };
    CheckRuntimeInput {
        version: CHECK_RUNTIME_VERSION,
        generation: lease.generation,
        bound: BoundIdentity {
            assignment_id: lease.launch.assignment_id.clone(),
            record_id: 7,
            profile_revision: lease.launch.profile_revision,
            profile_record_type: "task".into(),
            provider: "pi-durable".into(),
            agent_kind: "pi".into(),
            model: "openai/gpt-4.1".into(),
            attempt_number: 1,
            check_attempt: Some(attempt),
            execution_directory: "/bound/worktree".into(),
            provider_workspace: None,
        },
        configuration,
        credential_file: directory.join("model-key"),
        capability: CheckCapability {
            assignment_id: lease.launch.assignment_id.clone(),
            daemon_url: "http://127.0.0.1:1".into(),
            secret_file: directory.join("capability"),
            forge_cli: "/bin/forge".into(),
        },
        launch: lease.launch,
    }
}

fn runtime() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(root.path()).unwrap().join("runtime");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    (root, directory)
}

#[test]
fn writes_one_owner_only_strict_input_and_never_rewrites_it() {
    let (_root, directory) = runtime();
    let value = input(&directory);
    write(&directory, &value).unwrap();
    let path = directory.join(CHECK_RUNTIME_FILE);
    let meta = fs::symlink_metadata(&path).unwrap();
    assert!(meta.is_file());
    assert_eq!(meta.mode() & 0o7777, 0o600);
    assert_eq!(meta.nlink(), 1);
    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        serde_json::from_slice::<CheckRuntimeInput>(&bytes).unwrap(),
        value
    );
    // Immutable binding: a second write (including a recovery) cannot rebind it.
    let mut other = value.clone();
    other.capability.daemon_url = "http://127.0.0.1:2".into();
    assert_eq!(
        write(&directory, &other).unwrap_err().code,
        "runner-check-input-unavailable"
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    // Unknown fields are not part of protocol 1.
    let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    json["behavior"] = "injected".into();
    assert!(serde_json::from_value::<CheckRuntimeInput>(json).is_err());
    // Secrets never appear in Debug output.
    assert!(!format!("{value:?}").contains("model-key"));
}

#[test]
fn refuses_inconsistent_input_and_unprotected_directories() {
    let (_root, directory) = runtime();
    let base = input(&directory);
    let cases: [fn(&mut CheckRuntimeInput); 6] = [
        |v| v.version = 2,
        |v| v.generation += 1,
        |v| v.bound.assignment_id = "other".into(),
        |v| v.configuration.cwd = "/other".into(),
        |v| v.credential_file = v.capability.secret_file.clone(),
        |v| v.bound.check_attempt = None,
    ];
    for change in cases {
        let mut value = base.clone();
        change(&mut value);
        assert_eq!(
            write(&directory, &value).unwrap_err().code,
            "runner-check-input-invalid"
        );
    }
    assert!(!directory.join(CHECK_RUNTIME_FILE).exists());
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        write(&directory, &base).unwrap_err().code,
        "runner-check-input-unavailable"
    );
    assert!(!directory.join(CHECK_RUNTIME_FILE).exists());
}
