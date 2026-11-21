use super::prepare;
use forge_app::{
    AgentDispatchRepository, AgentProfile, RecordType, agent_profiles::ResolvedLaunchPolicy,
};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
};

struct Fixture {
    root: PathBuf,
    key: PathBuf,
    worktree: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).unwrap();
        let id = u128::from_ne_bytes(random);
        let root = std::env::temp_dir().join(format!("forge-t06-{id}"));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let worktree = root.join("worktree");
        fs::create_dir(&worktree).unwrap();
        let key = root.join("private-openai-key");
        fs::write(&key, "sk-private-model-key-canary\n").unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        Self {
            root,
            key,
            worktree,
        }
    }

    fn profile() -> AgentProfile {
        let store = forge_store::SqliteProjectRepository::in_memory().unwrap();
        let mut profile = store.agent_profile(RecordType::Task, 1).unwrap();
        profile.provider = "pi-durable".into();
        profile.model = "openai/gpt-4.1".into();
        profile.launch_policy = Some(serde_json::from_value::<ResolvedLaunchPolicy>(serde_json::json!({
            "version":1, "runtime":"pi-durable", "model":"openai/gpt-4.1", "thinkingLevel":"off", "tools":["read","write","edit","bash"],
            "behavior":"Explicit behavior", "skills":[], "extensions":[], "mcpServers":[]
        })).unwrap());
        profile
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn explicit_preflight_resolves_configuration_without_disclosing_or_issuing_authority() {
    let fixture = Fixture::new();
    let profile = Fixture::profile();
    let prepared = prepare(&profile, &fixture.worktree, Some(&fixture.key)).unwrap();
    assert_eq!(prepared.configuration.cwd, fixture.worktree);
    assert_eq!(
        prepared.configuration.context.instructions,
        "Explicit behavior"
    );
    assert_eq!(prepared.configuration.model, "openai/gpt-4.1");
    assert_eq!(prepared.credential_file, fixture.key);
    let json = serde_json::to_string(&prepared.configuration).unwrap();
    let debug = format!("{prepared:?}");
    for output in [&json, &debug] {
        assert!(!output.contains("sk-private-model-key-canary"));
        assert!(!output.contains("private-openai-key"));
    }
    assert!(!debug.contains("Explicit behavior"));
    // A credential source grants no Forge capability: this boundary accepts no
    // repository/capability-delivery port and performs no database mutations.
    assert!(prepare(&profile, &fixture.worktree, None).is_err());
}

#[test]
fn missing_malformed_and_unprotected_auth_fail_closed_with_fixed_diagnostics() {
    let fixture = Fixture::new();
    let profile = Fixture::profile();
    let assert_denied = || {
        let error = prepare(&profile, &fixture.worktree, Some(&fixture.key)).unwrap_err();
        assert_eq!(error.code, "durable-model-auth-unavailable");
        assert!(!error.message.contains("private-model-key"));
        assert!(!error.message.contains("private-openai-key"));
    };
    for token in [
        String::new(),
        "a b".into(),
        "a\nb".into(),
        "a\r\n".into(),
        "é".into(),
        "x".repeat(4097),
    ] {
        fs::write(&fixture.key, token).unwrap();
        assert_denied();
    }
    fs::write(&fixture.key, "sk-private-model-key-canary").unwrap();
    fs::set_permissions(&fixture.key, fs::Permissions::from_mode(0o644)).unwrap();
    assert_denied();
    fs::set_permissions(&fixture.key, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o755)).unwrap();
    assert_denied();
    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_file(&fixture.key).unwrap();
    assert_denied();
}

#[test]
fn redirected_and_hardlinked_auth_sources_are_not_adopted() {
    let fixture = Fixture::new();
    let profile = Fixture::profile();
    let alias = fixture.root.join("alias");
    std::os::unix::fs::symlink(&fixture.key, &alias).unwrap();
    assert!(prepare(&profile, &fixture.worktree, Some(&alias)).is_err());
    fs::remove_file(&alias).unwrap();
    fs::hard_link(&fixture.key, &alias).unwrap();
    assert!(prepare(&profile, &fixture.worktree, Some(&fixture.key)).is_err());
    fs::remove_file(&alias).unwrap();
    prepare(&profile, &fixture.worktree, Some(&fixture.key)).unwrap();
}

#[test]
fn server_preflight_composes_with_the_actual_runner_without_execution_permission() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let fixture = Fixture::new();
    let prepared = prepare(&Fixture::profile(), &fixture.worktree, Some(&fixture.key)).unwrap();
    let runner = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../runner")
        .canonicalize()
        .unwrap();
    let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("node"))
        .find(|path| path.is_file())
        .unwrap()
        .canonicalize()
        .unwrap();
    let mut child = Command::new(node)
        .arg(runner.join("test/profile-preflight.mjs"))
        .env_clear()
        .env("HOME", &fixture.root)
        .env("OPENAI_API_KEY", "ambient-key-must-not-be-used")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = serde_json::to_vec(&serde_json::json!({
        "configuration": prepared.configuration,
        "credential_file": prepared.credential_file,
    }))
    .unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for bytes in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains("sk-private-model-key-canary"));
        assert!(!text.contains("ambient-key-must-not-be-used"));
    }
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["configured"], true);
    assert_eq!(result["permission"], false);
    assert_eq!(result["model"]["modelId"], "gpt-4.1");
    assert_eq!(
        result["tools"],
        serde_json::json!(["read", "write", "edit", "bash"])
    );
    assert_eq!(fs::read_dir(&fixture.worktree).unwrap().count(), 0);
}

#[test]
fn profile_identity_and_new_launch_sources_cannot_be_bypassed() {
    let fixture = Fixture::new();
    let profile = Fixture::profile();
    for change in [0, 1, 2, 3, 4] {
        let mut invalid = profile.clone();
        match change {
            0 => invalid.record_type = RecordType::Grilling,
            1 => invalid.provider = "herdr".into(),
            2 => invalid.agent_kind = "unknown".into(),
            3 => invalid.launch_policy = None,
            _ => invalid.model = "other/model".into(),
        }
        assert!(prepare(&invalid, &fixture.worktree, Some(&fixture.key)).is_err());
    }
    assert!(
        prepare(
            &profile,
            &fixture.root.join("missing-worktree"),
            Some(&fixture.key)
        )
        .is_err()
    );
    let mut missing = profile;
    missing.launch_policy.as_mut().unwrap().skills.push(
        forge_app::agent_profiles::DirectResource {
            name: "approved".into(),
            path: fixture.root.join("missing-source.md"),
            sha256: "0".repeat(64),
        },
    );
    assert!(prepare(&missing, &fixture.worktree, Some(&fixture.key)).is_err());
    // Inspection uses retained configuration, not prepare() or mutable sources.
    let json = serde_json::to_string(missing.launch_policy.as_ref().unwrap()).unwrap();
    let historical: ResolvedLaunchPolicy = serde_json::from_str(&json).unwrap();
    historical.validate_durable_configuration().unwrap();
}

#[tokio::test]
async fn operator_preview_and_registration_require_explicit_auth_before_persisting() {
    use forge_app::WorkspaceRepository;
    use forge_rpc::{AgentProfileApiServer, AgentProfileDocumentRequest};
    let fixture = Fixture::new();
    let database = fixture.root.join("forge.sqlite");
    let store = forge_store::SqliteProjectRepository::open(&database).unwrap();
    store
        .create_workspace("test", "Test", &fixture.worktree)
        .unwrap();
    let source = fixture.worktree.join(".agents/profiles/durable.md");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "---\nversion: 1\nruntime: pi-durable\nmodel: openai/gpt-4.1\nthinkingLevel: off\ntools: [bash]\n---\nPrivate behavior").unwrap();
    let config = crate::DaemonConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        store_path: database,
        durable_model_key_file: None,
        durable_inspection: None,
        durable_execution: None,
        managed: None,
    };
    let daemon = crate::Daemon::new(crate::application_from_config(&config).unwrap())
        .with_profile_operator_root(fixture.root.join("operator"));
    let request = || AgentProfileDocumentRequest {
        workspace: "test".into(),
        document: ".agents/profiles/durable.md".into(),
        record_type: "task".into(),
    };
    for error in [
        daemon.preview(request()).await.unwrap_err(),
        daemon.register(request()).await.unwrap_err(),
    ] {
        assert!(error.to_string().contains("durable-model-auth-unavailable"));
        assert!(!error.to_string().contains("sk-private-model-key-canary"));
    }
    assert!(store.agent_profile(RecordType::Task, 2).is_err());
    assert!(!fixture.root.join("assignment-capabilities").exists());
    let daemon = daemon.with_durable_model_key_file(fixture.key.clone());
    let preview = daemon.preview(request()).await.unwrap();
    assert_eq!(preview.revision, None);
    let registered = daemon.register(request()).await.unwrap();
    let revision = registered.revision.unwrap();
    let profile = store.agent_profile(RecordType::Task, revision).unwrap();
    assert_eq!(profile.provider, "pi-durable");
    for output in [
        serde_json::to_string(&preview).unwrap(),
        serde_json::to_string(&registered).unwrap(),
    ] {
        assert!(!output.contains("Private behavior"));
        assert!(!output.contains("sk-private-model-key-canary"));
        assert!(!output.contains("private-openai-key"));
    }
    fs::remove_file(&fixture.key).unwrap();
    fs::remove_file(&source).unwrap();
    assert_eq!(
        store.agent_profile(RecordType::Task, revision).unwrap(),
        profile
    );
    assert!(!fixture.root.join("assignment-capabilities").exists());
}
