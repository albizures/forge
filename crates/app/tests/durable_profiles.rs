use forge_app::agent_profiles::{ProfileDocument, ResourceCatalog};
use forge_app::durable_profiles::MAX_CONTEXT_BYTES;
use std::fs;

fn document() -> String {
    "---\nversion: 1\nruntime: pi-durable\nmodel: openai/gpt-4.1\nthinkingLevel: off\ntools: [read, write, edit, bash]\nskills: [{name: approved}]\n---\nExplicit behavior".into()
}

#[test]
fn explicit_durable_matrix_rejects_unsupported_policy_without_echoing_values() {
    ProfileDocument::parse(&document()).unwrap();
    for text in [
        document().replace("openai/gpt-4.1", "secret-provider/key-canary"),
        document().replace("thinkingLevel: off", "thinkingLevel: low"),
        document().replace("read, write, edit, bash", "read, grep"),
        document().replace("read, write, edit, bash", "bash, bash"),
        document().replace("skills:", "extensions:"),
        document().replace("skills:", "mcpServers:"),
    ] {
        let error = ProfileDocument::parse(&text).unwrap_err();
        assert!(!error.contains("key-canary"));
    }
}

#[test]
fn context_contains_only_bounded_hash_verified_approved_markdown() {
    let root = tempfile::tempdir().unwrap();
    let operator = root.path().join("operator");
    fs::create_dir_all(root.path().join(".agents/profiles")).unwrap();
    fs::create_dir_all(root.path().join(".agents/skills/approved")).unwrap();
    let resource = root.path().join(".agents/skills/approved/SKILL.md");
    let approved =
        "---\nname: approved\ndescription: Approved Markdown\n---\nApproved instructions";
    fs::write(&resource, approved).unwrap();
    fs::write(root.path().join("AGENTS.md"), "ambient-canary").unwrap();
    fs::create_dir_all(root.path().join(".pi")).unwrap();
    fs::write(root.path().join(".pi/settings.json"), "ambient-canary").unwrap();
    let source = root.path().join(".agents/profiles/test.md");
    fs::write(&source, document()).unwrap();
    let policy = ResourceCatalog::discover(&operator, root.path())
        .unwrap()
        .resolve(root.path(), &source)
        .unwrap();
    let context = policy.durable_context().unwrap();
    assert_eq!(
        context.instructions,
        format!("Explicit behavior\n\n{approved}")
    );
    assert_eq!(context.resources, policy.skills);
    fs::remove_file(&source).unwrap();
    fs::write(root.path().join("AGENTS.md"), "changed-ambient-canary").unwrap();
    assert_eq!(
        policy.durable_context().unwrap().instructions,
        context.instructions
    );
    fs::write(&resource, "drift-canary").unwrap();
    assert!(policy.durable_context().unwrap_err().contains("drifted"));
    fs::write(&resource, vec![b'x'; MAX_CONTEXT_BYTES + 1]).unwrap();
    assert!(policy.durable_context().unwrap_err().contains("256 KiB"));
    fs::remove_file(&resource).unwrap();
    assert!(policy.validate_direct_inputs().is_err());
    // Historical compatibility checks never reload sources or grant execution.
    policy.validate_durable_configuration().unwrap();
    assert!(!context.instructions.contains("canary"));
}

#[test]
fn resolved_execution_configuration_matches_the_runner_contract() {
    use forge_app::durable_profiles::ExecutionConfiguration;
    let wire: serde_json::Value =
        serde_json::from_str(include_str!("../../../runner/test/profile-v1.json")).unwrap();
    let decoded: ExecutionConfiguration = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    let mut policy: forge_app::agent_profiles::ResolvedLaunchPolicy = serde_json::from_value(serde_json::json!({
        "version": 1, "runtime": "pi-durable", "model": "openai/gpt-4.1", "thinkingLevel": "off",
        "tools": ["read", "write", "edit", "bash"], "behavior": "Explicit profile behavior",
        "skills": [], "extensions": [], "mcpServers": []
    })).unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = policy.durable_execution_configuration(root.path()).unwrap();
    let mut expected = wire;
    expected["cwd"] = root.path().to_str().unwrap().into();
    assert_eq!(serde_json::to_value(config).unwrap(), expected);
    assert!(
        policy
            .durable_execution_configuration(std::path::Path::new("relative"))
            .is_err()
    );
    policy.thinking_level = None;
    assert!(policy.durable_execution_configuration(root.path()).is_err());
}

#[test]
fn persisted_policy_cannot_bypass_durable_checks() {
    let policy = serde_json::json!({
        "version": 1, "runtime": "pi-durable", "model": "openai/gpt-4.1",
        "tools": [], "behavior": "Explicit", "skills": [], "extensions": [], "mcpServers": []
    });
    let decoded: forge_app::agent_profiles::ResolvedLaunchPolicy =
        serde_json::from_value(policy.clone()).unwrap();
    assert!(decoded.validate_direct_inputs().is_err()); // No invented thinking default.
    let mut policy = policy;
    policy["thinkingLevel"] = "off".into();
    let mut decoded: forge_app::agent_profiles::ResolvedLaunchPolicy =
        serde_json::from_value(policy).unwrap();
    decoded.validate_direct_inputs().unwrap();
    decoded.behavior = "x".repeat(MAX_CONTEXT_BYTES + 1);
    assert!(decoded.validate_direct_inputs().is_err());
}
