use forge_app::agent_profiles::{ProfileDocument, ResourceCatalog};
use std::fs;

#[test]
fn resolves_workspace_catalog_and_checks_only_selected_direct_inputs() {
    let root = tempfile::tempdir().unwrap();
    let operator = root.path().join("operator");
    let workspace = root.path().join("workspace");
    for base in [&operator, &workspace] {
        fs::create_dir_all(base.join(".agents/skills/entry")).unwrap();
        fs::write(
            base.join(".agents/skills/entry/SKILL.md"),
            "---\nname: entry\ndescription: entry\n---\nSkill",
        )
        .unwrap();
    }
    fs::create_dir_all(workspace.join(".agents/profiles")).unwrap();
    let document = workspace.join(".agents/profiles/implementation.md");
    fs::write(&document, "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: []\nskills: [{name: entry}]\n---\nBehavior").unwrap();
    let catalog = ResourceCatalog::discover(&operator, &workspace).unwrap();
    assert!(
        catalog
            .resolve(&workspace, ".agents/profiles/implementation.md")
            .is_err()
    );
    fs::write(&document, "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: []\nskills: [{path: ../skills/entry/SKILL.md}]\n---\nBehavior").unwrap();
    let policy = catalog
        .resolve(&workspace, ".agents/profiles/implementation.md")
        .unwrap();
    assert_eq!(
        policy.skills[0].path,
        workspace
            .join(".agents/skills/entry/SKILL.md")
            .canonicalize()
            .unwrap()
    );
    fs::write(document.parent().unwrap().join("support.sh"), "changed").unwrap();
    fs::remove_file(&document).unwrap();
    policy.validate_direct_inputs().unwrap();
    assert_eq!(policy.behavior, "Behavior");
    fs::write(workspace.join(".agents/skills/entry/SKILL.md"), "changed").unwrap();
    assert!(policy.validate_direct_inputs().is_err());
    fs::write(
        workspace.join(".agents/skills/entry/SKILL.md"),
        "---\nname: entry\ndescription: entry\n---\nSkill",
    )
    .unwrap();
    policy.validate_direct_inputs().unwrap();
}

#[test]
fn catalog_membership_is_canonical_and_cannot_be_granted_by_a_path() {
    let root = tempfile::tempdir().unwrap();
    let operator = root.path().join("operator");
    let workspace = root.path().join("workspace");
    let outside = root.path().join("outside");
    fs::create_dir_all(operator.join(".agents/skills")).unwrap();
    fs::create_dir_all(workspace.join(".agents/profiles")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(
        outside.join("SKILL.md"),
        "---\nname: external\ndescription: external\n---\nSkill",
    )
    .unwrap();
    fs::write(
        workspace.join("SKILL.md"),
        "---\nname: local\ndescription: local\n---\nSkill",
    )
    .unwrap();
    let document = format!(
        "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: []\nskills: [{{path: '{}'}}]\n---\nBehavior",
        outside.display()
    );
    fs::write(workspace.join(".agents/profiles/external.md"), document).unwrap();
    assert!(
        ResourceCatalog::discover(&operator, &workspace)
            .unwrap()
            .resolve(&workspace, ".agents/profiles/external.md")
            .is_err()
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, operator.join(".agents/skills/alias")).unwrap();
        std::os::unix::fs::symlink(&outside, operator.join(".agents/skills/duplicate")).unwrap();
        let catalog = ResourceCatalog::discover(&operator, &workspace).unwrap();
        let policy = catalog
            .resolve(&workspace, ".agents/profiles/external.md")
            .unwrap();
        assert_eq!(policy.skills.len(), 1);
        assert_eq!(
            policy.skills[0].path,
            outside.join("SKILL.md").canonicalize().unwrap()
        );
        fs::write(workspace.join(".agents/profiles/external.md"), "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: []\nskills: [{name: external}]\n---\nBehavior").unwrap();
        assert_eq!(
            catalog
                .resolve(&workspace, ".agents/profiles/external.md")
                .unwrap(),
            policy
        );
    }
}

#[test]
fn freezes_mcp_configuration_but_detects_extension_entry_drift() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".pi/extensions")).unwrap();
    fs::write(
        root.path().join("SKILL.md"),
        "---\nname: entry\ndescription: entry\n---\nSkill",
    )
    .unwrap();
    fs::write(
        root.path().join(".pi/extensions/entry.ts"),
        "throw new Error('never execute');",
    )
    .unwrap();
    fs::write(
        root.path().join(".pi/mcp.json"),
        r#"{"mcpServers":{"local":{"command":"never-connect","args":[]}}}"#,
    )
    .unwrap();
    fs::create_dir_all(root.path().join(".agents/profiles")).unwrap();
    fs::write(root.path().join(".agents/profiles/mcp.md"), "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: []\nextensions: [{name: entry}]\nmcpServers: [{name: local}]\n---\nBehavior").unwrap();
    let policy = ResourceCatalog::discover(root.path(), root.path())
        .unwrap()
        .resolve(root.path(), ".agents/profiles/mcp.md")
        .unwrap();
    assert_eq!(
        policy.mcp_servers[0].configuration["command"],
        "never-connect"
    );
    let debug = format!("{policy:?}");
    assert!(!debug.contains("never-connect"));
    assert!(!debug.contains("Behavior"));
    fs::remove_file(root.path().join(".pi/mcp.json")).unwrap();
    fs::write(root.path().join(".pi/extensions/support.ts"), "changed").unwrap();
    policy.validate_direct_inputs().unwrap();
    fs::remove_file(root.path().join(".pi/extensions/entry.ts")).unwrap();
    assert!(policy.validate_direct_inputs().is_err());
}

#[test]
fn extension_directories_select_the_entry_not_supporting_modules() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".pi/extensions/plugin")).unwrap();
    fs::write(
        root.path().join("SKILL.md"),
        "---\nname: entry\ndescription: entry\n---\nSkill",
    )
    .unwrap();
    fs::write(root.path().join(".pi/extensions/plugin/index.ts"), "entry").unwrap();
    fs::write(
        root.path().join(".pi/extensions/plugin/support.ts"),
        "support",
    )
    .unwrap();
    let document = "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: []\nextensions: [{name: plugin}]\n---\nBehavior";
    fs::create_dir_all(root.path().join(".agents/profiles")).unwrap();
    let source = root.path().join(".agents/profiles/extensions.md");
    fs::write(&source, document).unwrap();
    let catalog = ResourceCatalog::discover(root.path(), root.path()).unwrap();
    let policy = catalog.resolve(root.path(), &source).unwrap();
    assert_eq!(
        policy.extensions[0].path,
        root.path()
            .join(".pi/extensions/plugin/index.ts")
            .canonicalize()
            .unwrap()
    );
    fs::write(
        &source,
        document.replace("name: plugin", "path: ../../.pi/extensions/plugin"),
    )
    .unwrap();
    assert_eq!(catalog.resolve(root.path(), &source).unwrap(), policy);
    fs::write(
        &source,
        document.replace(
            "name: plugin",
            "path: ../../.pi/extensions/plugin/support.ts",
        ),
    )
    .unwrap();
    assert!(catalog.resolve(root.path(), &source).is_err());
}

#[test]
fn parses_closed_policy_and_rejects_ambiguous_authoring() {
    let valid = "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: []\n---\nUse the entry skill.\n";
    let parsed = ProfileDocument::parse(valid).unwrap();
    assert!(parsed.tools.is_empty());
    assert!(parsed.skills.is_empty());
    assert_eq!(parsed.behavior, "Use the entry skill.\n");
    for invalid in [
        valid.replace("version: 1", "version: 2"),
        valid.replace("version: 1", "version: 1\nversion: 1"),
        valid.replace("runtime: herdr/pi", "runtime: other"),
        valid.replace("openai/gpt-5", "gpt-5"),
        valid.replace("tools: []", "tools: ['*']"),
        valid.replace("tools: []", "tools: ['+read']"),
        valid.replace("tools: []", "tools: [unknown]"),
        valid.replace("tools: []", "tools: []\nretry_limit: 2"),
        valid.replace("tools: []", "tools: []\nskills: [{name: x, path: x}]"),
        valid.replace("tools: []", "tools: []\nskills: [{name: ' '}]"),
        valid.replace("tools: []", "tools: null"),
        valid.replace("tools: []", "tools: [read, read]"),
        valid.replace("tools: []", "tools: []\nskills: null"),
        valid.replace("tools: []", "tools: []\nskills: [{name: x, name: y}]"),
        valid.replace("tools: []", "tools: []\nextensions: [entry]"),
        valid.replace("tools: []", "tools: []\nmcpServers: [{}]"),
        valid.replace("version: 1", "version: 1.5"),
        valid.replace("model: openai/gpt-5\n", ""),
        valid.replace("runtime: herdr/pi\n", ""),
        valid.replace("tools: []\n", ""),
    ] {
        assert!(ProfileDocument::parse(&invalid).is_err(), "{invalid}");
    }
}
