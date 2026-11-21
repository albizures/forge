use super::*;
use forge_app::{AgentDispatchAdapterFactory, RecordType};
use std::process::Command;

struct NoOriginal;
impl AgentDispatchAdapterFactory for NoOriginal {
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn forge_app::AgentDispatchAdapter>, AgentAdapterError> {
        panic!("no fallback to the original backend")
    }
}

/// Isolated per-test directory (no added dependency), removed on drop.
struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "forge-launch-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn durable_profile() -> AgentProfile {
    let store = SqliteProjectRepository::in_memory().unwrap();
    let mut profile = store.agent_profile(RecordType::Task, 1).unwrap();
    profile.provider = "pi-durable".into();
    profile
}

#[test]
fn execution_selection_is_explicit_complete_and_absolute() {
    let p = |s: &str| Some(PathBuf::from(s));
    assert_eq!(Execution::select(None, None, None, None).unwrap(), None);
    for (host, entry) in [(p("/h"), None), (None, p("/r/src/check-entry.ts"))] {
        assert!(
            Execution::select(p("/node"), p("/r/src/inspect-effects.ts"), host, entry).is_err()
        );
    }
    // A runner selection needs the matched Node and effect inspector too.
    assert!(Execution::select(None, None, p("/h"), p("/r/src/check-entry.ts")).is_err());
    assert!(Execution::select(p("node"), p("/i"), p("/h"), p("/r/src/check-entry.ts")).is_err());
    let selected = Execution::select(
        p("/node"),
        p("/r/src/inspect-effects.ts"),
        p("/h"),
        p("/r/src/check-entry.ts"),
    )
    .unwrap()
    .unwrap();
    // Packaged layout: inspectors are the entry's siblings; no host arguments.
    assert!(selected.components.host_arguments.is_empty());
    assert_eq!(
        selected.components.inspector,
        PathBuf::from("/r/src/inspect-recovery.ts")
    );
    assert_eq!(
        selected.components.answer_inspector,
        PathBuf::from("/r/src/inspect-answer.ts")
    );
}

#[test]
fn composition_without_matched_components_refuses_launch_and_recovery_without_fallback() {
    let fixture = Fixture::new("unconfigured");
    let database = fixture.path().join("forge.sqlite3");
    drop(SqliteProjectRepository::open(&database).unwrap());
    let factory = compose(&database, None, None, &[], None, Box::new(NoOriginal)).unwrap();
    assert!(factory.launcher.is_none() && factory.recovery.is_none());
    let profile = durable_profile();
    assert!(factory.validate_profile(&profile).is_err());
    assert!(factory.validate_preflight(&profile).is_err());
    // Existing work keeps the durable backend; it is never routed elsewhere.
    let adapter = factory.adapter_for(&profile).unwrap();
    assert!(adapter.probe().is_err());
}

#[test]
fn composition_rejects_unattestable_components() {
    let fixture = Fixture::new("unattestable");
    let database = fixture.path().join("forge.sqlite3");
    let execution = Execution::select(
        Some(fixture.path().join("missing-node")),
        Some(fixture.path().join("src/inspect-effects.ts")),
        Some(fixture.path().join("missing-host")),
        Some(fixture.path().join("src/check-entry.ts")),
    )
    .unwrap()
    .unwrap();
    assert!(
        compose(
            &database,
            None,
            Some(&execution),
            &[],
            None,
            Box::new(NoOriginal)
        )
        .is_err()
    );
}

#[test]
fn launcher_errors_map_to_retaining_adapter_categories() {
    let code = |code: &'static str| adapter_error(&RegistrationError::new(code, "private"));
    assert!(matches!(
        code("durable-launch-claimed"),
        AgentAdapterError::Conflict(_)
    ));
    assert!(matches!(
        code("invalid-durable-profile"),
        AgentAdapterError::InvalidRequest(_)
    ));
    assert!(matches!(
        code("durable-launch-uncertain"),
        AgentAdapterError::Transient(_)
    ));
    // Private detail never crosses the adapter boundary.
    assert!(!format!("{:?}", code("durable-launch-uncertain")).contains("private"));
}

#[test]
fn forge_revision_is_the_build_commit() {
    let revision = current_forge_revision();
    assert!(revision.starts_with(forge_build_info::current().commit));
}

/// The real Node 24 behind this test's own `PATH` probe. `PATH` is used only to
/// find it here, never by the composition under test.
fn real_node() -> PathBuf {
    let probe = Command::new("node")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .args(["--print", "process.execPath"])
        .output()
        .expect("Node 24 is required for durable launch tests");
    assert!(probe.status.success());
    PathBuf::from(String::from_utf8(probe.stdout).unwrap().trim())
}

fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn compatible_host() -> String {
    format!(
        r#"{{"protocol":{RUNNER_PROTOCOL_VERSION},"runtime_schema":{DURABLE_RUNTIME_SCHEMA},"forge_revision":"{}"}}"#,
        current_forge_revision()
    )
}

/// A bundle-layout component set: the shipped runner sources (copied, so they
/// can be edited), a Node wrapper around the real Node, and a host script that
/// reports `host` as its compatibility. `marker` makes the sources' component
/// hash distinct per set.
fn bundle_layout(root: &Path, host: &str, node_version: Option<&str>, marker: &str) -> Execution {
    use forge_app::durable_execution::bundle;
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runner");
    let sources = root.join("runner/src");
    std::fs::create_dir_all(&sources).unwrap();
    for entry in std::fs::read_dir(shipped.join("src")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), sources.join(entry.file_name())).unwrap();
    }
    for file in ["package-lock.json", "prepare-reader.mjs"] {
        std::fs::copy(shipped.join(file), root.join("runner").join(file)).unwrap();
    }
    std::fs::write(sources.join("marker.ts"), marker).unwrap();
    let real = real_node();
    let version = node_version.map_or(String::new(), |version| {
        format!("if [ \"$1\" = --version ]; then echo {version}; exit 0; fi\n")
    });
    script(
        &root.join(bundle::NODE),
        &format!("{version}exec '{}' \"$@\"\n", real.display()),
    );
    script(
        &root.join(bundle::HOST),
        &format!("if [ \"$1\" = --compatibility ]; then echo '{host}'; exit 0; fi\nexit 1\n"),
    );
    Execution::from_bundle(&root.canonicalize().unwrap()).unwrap()
}

fn composed(execution: &Execution, retained: &[Execution], database: &Path) -> retirement::Factory {
    compose(
        database,
        None,
        Some(execution),
        retained,
        None,
        Box::new(NoOriginal),
    )
    .unwrap()
}

fn refused(factory: &retirement::Factory) -> String {
    let error = factory.launcher.as_ref().unwrap().preflight().unwrap_err();
    assert_eq!(error.code, "durable-components-incompatible");
    // The same refusal reaches the pre-allocation hooks the application calls.
    let profile = durable_profile();
    assert!(matches!(
        factory.validate_preflight(&profile),
        Err(AgentAdapterError::InvalidRequest(code)) if code == "durable-components-incompatible"
    ));
    assert!(matches!(
        factory.adapter_for(&profile).unwrap().probe(),
        Err(AgentAdapterError::InvalidRequest(code)) if code == "durable-components-incompatible"
    ));
    error.message
}

#[test]
fn bundle_root_derives_every_component_from_the_fixed_layout() {
    use forge_app::durable_execution::bundle;
    let root = Path::new("/installed/runner-bundle");
    let execution = Execution::from_bundle(root).unwrap();
    assert_eq!(execution.components.host, root.join(bundle::HOST));
    assert!(execution.components.host_arguments.is_empty());
    assert_eq!(execution.components.node, root.join(bundle::NODE));
    assert_eq!(execution.components.runner, root.join(bundle::ENTRY));
    assert_eq!(
        execution.effect_inspector,
        root.join(bundle::EFFECT_INSPECTOR)
    );
    assert_eq!(
        execution.components.inspector.parent(),
        execution.effect_inspector.parent()
    );
    assert!(Execution::from_bundle(Path::new("relative/bundle")).is_err());
}

#[test]
fn launch_preflight_accepts_matched_components_and_rejects_each_fault_before_allocation() {
    let fixture = Fixture::new("preflight");
    let database = fixture.path().join("forge.sqlite3");
    let good = bundle_layout(
        &fixture.path().join("good"),
        &compatible_host(),
        None,
        "// a",
    );
    let factory = composed(&good, &[], &database);
    factory.launcher.as_ref().unwrap().preflight().unwrap();
    factory
        .adapter_for(&durable_profile())
        .unwrap()
        .probe()
        .unwrap();

    // Tampered runner sources no longer attest the composed component hash.
    let entry = good.components.runner.clone();
    let original = std::fs::read(&entry).unwrap();
    std::fs::write(&entry, [original.as_slice(), b"// tampered"].concat()).unwrap();
    assert!(refused(&factory).contains("tampered or replaced"));
    std::fs::write(&entry, &original).unwrap();
    factory.launcher.as_ref().unwrap().preflight().unwrap();

    // A missing component.
    std::fs::remove_file(&good.components.answer_inspector).unwrap();
    assert!(refused(&factory).contains("missing"));

    // Host replaced after composition (e.g. a different build dropped in).
    let replaced = bundle_layout(
        &fixture.path().join("replaced"),
        &compatible_host(),
        None,
        "// r",
    );
    let factory = composed(&replaced, &[], &database);
    script(&replaced.components.host, "exit 1\n");
    assert!(refused(&factory).contains("host changed"));

    // Node replaced after composition.
    let swapped = bundle_layout(
        &fixture.path().join("swapped"),
        &compatible_host(),
        None,
        "// s",
    );
    let factory = composed(&swapped, &[], &database);
    script(&swapped.components.node, "echo v24.18.0\n");
    assert!(refused(&factory).contains("Node changed"));

    // Wrong Node version.
    let old = bundle_layout(
        &fixture.path().join("old-node"),
        &compatible_host(),
        Some("v22.19.0"),
        "// o",
    );
    assert!(refused(&composed(&old, &[], &database)).contains("pinned v24.18.0"));

    // A host implementing another protocol or runtime schema.
    let protocol = format!(
        r#"{{"protocol":{},"runtime_schema":{DURABLE_RUNTIME_SCHEMA},"forge_revision":"{}"}}"#,
        RUNNER_PROTOCOL_VERSION + 1,
        current_forge_revision()
    );
    let mismatched = bundle_layout(&fixture.path().join("protocol"), &protocol, None, "// p");
    assert!(refused(&composed(&mismatched, &[], &database)).contains("different runner protocol"));

    // A host from another Forge Revision.
    let foreign = format!(
        r#"{{"protocol":{RUNNER_PROTOCOL_VERSION},"runtime_schema":{DURABLE_RUNTIME_SCHEMA},"forge_revision":"{}"}}"#,
        "f".repeat(40)
    );
    let other = bundle_layout(&fixture.path().join("revision"), &foreign, None, "// f");
    assert!(refused(&composed(&other, &[], &database)).contains("different Forge Revision"));

    // A host that does not answer.
    let silent = bundle_layout(&fixture.path().join("silent"), "not json", None, "// q");
    assert!(refused(&composed(&silent, &[], &database)).contains("does not report"));
}

fn launch_with(hash: &str) -> Launch {
    Launch {
        assignment_id: "assignment".into(),
        operation_id: "operation".into(),
        attempt_id: "attempt".into(),
        profile_revision: 1,
        runtime_id: "runtime".into(),
        incarnation_id: "incarnation".into(),
        conversation_id: "conversation".into(),
        store_id: "store".into(),
        forge_revision: "revision".into(),
        component_hash: hash.into(),
        runtime_schema: DURABLE_RUNTIME_SCHEMA,
    }
}

#[test]
fn retained_launches_select_their_original_components_or_refuse_explicitly() {
    let fixture = Fixture::new("catalog");
    let current = bundle_layout(
        &fixture.path().join("current"),
        &compatible_host(),
        None,
        "// new",
    );
    let previous = bundle_layout(
        &fixture.path().join("previous"),
        &compatible_host(),
        None,
        "// old",
    );
    let hash = |execution: &Execution| {
        attest_component_hash(
            &execution.components.node,
            execution.effect_inspector.parent().unwrap(),
        )
        .unwrap()
    };
    let (current_hash, previous_hash) = (hash(&current), hash(&previous));
    assert_ne!(current_hash, previous_hash);

    let catalog = ComponentCatalog::new(None, Some(&current), std::slice::from_ref(&previous));
    let old = launch_with(&previous_hash);
    assert_eq!(catalog.execution_for(&old).unwrap(), previous);
    assert_eq!(
        catalog.inspection_for(&old).unwrap(),
        Some(previous.inspection())
    );
    let new = launch_with(&current_hash);
    assert_eq!(catalog.execution_for(&new).unwrap(), current);

    // Without the original Revision's components: an explicit refusal naming the
    // hash and the Forgeup command, never the current Revision's components.
    let switched = ComponentCatalog::new(None, Some(&current), &[]);
    for error in [
        switched.execution_for(&old).unwrap_err(),
        switched.inspection_for(&old).unwrap_err(),
    ] {
        assert_eq!(error.code, "durable-components-unavailable");
        assert!(error.message.contains(&previous_hash));
        assert!(
            error
                .message
                .contains("forgeup runner resolve --component-hash")
        );
    }

    // Recovery selects components before any claim, storage access or spawn:
    // this database path does not exist, so reaching it would be a different error.
    let recovery = recovery::Controller::new(
        fixture.path().join("absent/forge.sqlite3"),
        current.components.clone(),
        Arc::new(retirement::Controller::default()),
    )
    .unwrap()
    .with_catalog(Arc::new(switched));
    assert_eq!(
        recovery.recover("workspace", &old).unwrap_err().code,
        "durable-components-unavailable"
    );

    // Nothing configured is "unconfigured", not a refusal; a legacy
    // unattestable inspection-only selection is still used as before.
    assert_eq!(
        ComponentCatalog::default().inspection_for(&old).unwrap(),
        None
    );
    let legacy = super::super::operator::InspectionComponents {
        node: fixture.path().join("no-node"),
        inspector: fixture.path().join("no-sources/inspect-effects.ts"),
    };
    let catalog = ComponentCatalog::new(Some(&legacy), None, &[]);
    assert_eq!(catalog.inspection_for(&old).unwrap(), Some(legacy));
}
