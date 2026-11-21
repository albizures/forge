use super::*;
use forge_app::{AgentDispatchAdapterFactory, RecordType};

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
    let factory = compose(&database, None, None, None, Box::new(NoOriginal)).unwrap();
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
