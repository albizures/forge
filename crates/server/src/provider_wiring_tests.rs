//! The capability delivery boundary must not impose Herdr's launch/container contract.
use super::*;
use forge_app::{
    AgentAdapterError, AgentDispatchRepository, AgentObservation, NewAgentAssignment,
    NewTypedRecord, RecordRepository,
};
use std::sync::Mutex;

type Calls = Arc<Mutex<Vec<&'static str>>>;

struct AlternateFactory {
    calls: Calls,
    deny_launch: bool,
}
impl AgentDispatchAdapterFactory for AlternateFactory {
    fn validate_profile(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        if profile.provider != "test-headless" {
            return Err(AgentAdapterError::InvalidRequest(
                "unsupported provider".into(),
            ));
        }
        Ok(())
    }
    fn validate_preflight(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        self.validate_profile(profile)?;
        self.calls.lock().unwrap().push("preflight");
        Ok(())
    }
    fn adapter_for(
        &self,
        profile: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        self.validate_profile(profile)?;
        self.calls.lock().unwrap().push("select");
        Ok(Box::new(AlternateAdapter {
            calls: self.calls.clone(),
            deny_launch: self.deny_launch,
        }))
    }
}
struct AlternateAdapter {
    calls: Calls,
    deny_launch: bool,
}
impl AgentDispatchAdapter for AlternateAdapter {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        Ok(())
    }
    fn validate_provision(&self, context: &AgentDispatchContext) -> Result<(), AgentAdapterError> {
        assert!(context.dispatch_workspace.is_none());
        assert!(context.capability_delivery.is_none());
        self.calls.lock().unwrap().push("launch-check");
        if self.deny_launch {
            return Err(AgentAdapterError::InvalidRequest("launch denied".into()));
        }
        Ok(())
    }
    fn provision(
        &self,
        context: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        let delivery = context.capability_delivery.as_ref().unwrap();
        assert!(delivery.secret_file.is_file());
        assert!(
            context
                .bootstrap_prompt
                .as_ref()
                .unwrap()
                .contains("assignment bootstrap")
        );
        assert!(
            !context
                .bootstrap_prompt
                .as_ref()
                .unwrap()
                .contains("PRIVATE-BEHAVIOR")
        );
        self.calls.lock().unwrap().push("provision");
        Ok(())
    }
    fn observe(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentObservation, AgentAdapterError> {
        assert!(context.capability_delivery.is_none());
        self.calls.lock().unwrap().push("observe");
        Ok(AgentObservation::Unknown)
    }
    fn recover(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentObservation, AgentAdapterError> {
        assert!(context.capability_delivery.is_none());
        self.calls.lock().unwrap().push("recover");
        Ok(AgentObservation::Unknown)
    }
    fn retire(
        &self,
        context: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        assert!(context.capability_delivery.is_none());
        self.calls.lock().unwrap().push("retire");
        Ok(())
    }
}

#[test]
fn alternate_provider_uses_common_delivery_without_herdr_launch_validation() {
    for deny_launch in [false, true] {
        let store = SqliteProjectRepository::in_memory().unwrap();
        crate::test_projects::register(&store, "project", None);
        let record = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "task".into(),
                project_slugs: vec!["project".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let mut assignment = store
            .create_agent_assignment(&NewAgentAssignment {
                id: "alternate".into(),
                record_id: record.id,
                project_slug: "project".into(),
                profile_revision: 1,
            })
            .unwrap();
        assignment.profile.provider = "test-headless".into();
        assignment.profile.agent_kind = "not-pi-cli".into();
        // This policy is deliberately incompatible with Herdr flag construction.
        assignment.profile.launch_policy = Some(serde_json::from_value(serde_json::json!({
            "version": 1, "runtime": "test-headless/runner", "model": "test/model", "tools": [],
            "behavior": "PRIVATE-BEHAVIOR", "skills": [], "extensions": [],
            "mcpServers": [{"name": "unsupported-by-herdr", "path": "/missing", "configuration": {}, "sha256": "missing"}]
        })).unwrap());
        assert!(HerdrAdapterFactory::validate_profile(&assignment.profile).is_err());
        assert!(herdr::pi_launch_arguments(&assignment.profile, "").is_err());
        let root = env::temp_dir().join(format!(
            "forge-provider-wiring-{}-{}",
            std::process::id(),
            getrandom_id()
        ));
        fs::create_dir_all(&root).unwrap();
        let calls = Calls::default();
        let factory = ServerAgentDispatchAdapterFactory {
            provider: Box::new(AlternateFactory {
                calls: calls.clone(),
                deny_launch,
            }),
            daemon_url: "http://127.0.0.1:1".into(),
            capability_secret_root: root.join("secrets"),
            cli_override: Some(root.join("forge")),
        };
        factory.validate_preflight(&assignment.profile).unwrap();
        let adapter = factory.adapter_for(&assignment.profile).unwrap();
        let context = AgentDispatchContext {
            assignment,
            project_root: root.clone(),
            agent_workspace: Some(root.join("worktree")),
            dispatch_workspace: None,
            rendered_initial_prompt: "PRIVATE-BEHAVIOR".into(),
            bootstrap_prompt: None,
            capability_delivery: None,
            resources: vec![],
        };
        assert_eq!(adapter.provision(&context, &store).is_err(), deny_launch);
        assert_eq!(root.join("secrets/alternate").exists(), !deny_launch);
        // Historical operations do not repeat preflight, issue credentials, or load sources.
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            adapter.observe(&context).unwrap(),
            AgentObservation::Unknown
        );
        assert_eq!(
            adapter.recover(&context).unwrap(),
            AgentObservation::Unknown
        );
        adapter.retire(&context, &store).unwrap();
        assert!(!root.exists());
        let mut expected = vec!["preflight", "select", "launch-check"];
        if !deny_launch {
            expected.push("provision");
        }
        expected.extend(["observe", "recover", "retire"]);
        assert_eq!(*calls.lock().unwrap(), expected);
    }
}

fn getrandom_id() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).unwrap();
    format!("{:032x}", u128::from_le_bytes(bytes))
}

#[test]
fn production_selection_rejects_unknown_backends_without_capability_issuance() {
    let store = SqliteProjectRepository::in_memory().unwrap();
    let mut profile = store.agent_profile(RecordType::Task, 1).unwrap();
    let root = env::temp_dir().join(format!("forge-unsupported-provider-{}", getrandom_id()));
    let factory = ServerAgentDispatchAdapterFactory {
        provider: Box::new(HerdrAdapterFactory::from_configuration_store(
            root.join("missing.db"),
        )),
        daemon_url: "http://127.0.0.1:1".into(),
        capability_secret_root: root.join("secrets"),
        cli_override: None,
    };
    for provider in ["pi-durable", "unknown", ""] {
        profile.provider = provider.into();
        assert!(factory.validate_profile(&profile).is_err());
        assert!(factory.validate_preflight(&profile).is_err());
        assert!(matches!(
            factory.adapter_for(&profile),
            Err(AgentAdapterError::Endpoint(message)) if message == "unsupported execution provider"
        ));
    }
    assert!(
        !root.exists(),
        "unsupported selection must not open storage or issue secrets"
    );
}
