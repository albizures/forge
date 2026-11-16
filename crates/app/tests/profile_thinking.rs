use forge_app::agent_profiles::{ProfileDocument, ResolvedLaunchPolicy, ResourceCatalog};

#[test]
fn all_exact_thinking_levels_resolve_and_round_trip() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".agents/profiles")).unwrap();
    let catalog = ResourceCatalog::discover(&root.path().join("operator"), root.path()).unwrap();
    for level in ["off", "minimal", "low", "medium", "high", "xhigh"] {
        let text = format!(
            "---\nversion: 1\nruntime: herdr/pi\nmodel: test/model\nthinkingLevel: {level}\ntools: []\n---\nRole-independent behavior"
        );
        assert_eq!(
            ProfileDocument::parse(&text)
                .unwrap()
                .thinking_level
                .as_str(),
            level
        );
        std::fs::write(root.path().join(".agents/profiles/test.md"), text).unwrap();
        let policy = catalog
            .resolve(root.path(), ".agents/profiles/test.md")
            .unwrap();
        assert_eq!(policy.thinking_level.unwrap().as_str(), level);
        let json = serde_json::to_value(&policy).unwrap();
        assert_eq!(json["thinkingLevel"], level);
        assert_eq!(
            serde_json::from_value::<ResolvedLaunchPolicy>(json).unwrap(),
            policy
        );
    }
}

#[test]
fn tagged_thinking_levels_are_not_exact_string_choices() {
    for level in ["off", "minimal", "low", "medium", "high", "xhigh"] {
        for value in [
            format!("!{level}"),
            format!("!{level} null"),
            format!("!{level} {level}"),
            format!("!!str {level}"),
            format!("!<tag:yaml.org,2002:str> {level}"),
            format!("&choice !!str {level}"),
        ] {
            let text = format!(
                "---\nversion: 1\nruntime: herdr/pi\nmodel: test/model\nthinkingLevel: {value}\ntools: []\n---\nBehavior"
            );
            let error = ProfileDocument::parse(&text).unwrap_err();
            assert!(
                error.contains("thinkingLevel is required and must be one of:"),
                "{value}: {error}"
            );
        }
    }
}

#[test]
fn untagged_yaml_syntax_remains_valid() {
    for value in [
        "low",
        "'low'",
        "\"low\"",
        "&choice low",
        "low # !!str is only a comment",
    ] {
        let text = format!(
            "---\nversion: 1\nruntime: herdr/pi\nmodel: test/model\nthinkingLevel: {value}\ntools: []\n---\nBehavior with !!str low"
        );
        assert_eq!(
            ProfileDocument::parse(&text)
                .unwrap()
                .thinking_level
                .as_str(),
            "low"
        );
    }
    for (anchor, valid) in [("&choice low", true), ("&choice !!str low", false)] {
        let text = format!(
            "---\nversion: 1\nruntime: herdr/pi\nmodel: test/model\ntools: []\nskills: [{{name: {anchor}}}]\nthinkingLevel: *choice\n---\nBehavior"
        );
        assert_eq!(ProfileDocument::parse(&text).is_ok(), valid);
    }
}

#[test]
fn historical_serialized_policy_has_no_invented_thinking_level() {
    let historical = r#"{"version":1,"runtime":"herdr/pi","model":"test/model","tools":[],"behavior":"old","skills":[],"extensions":[],"mcpServers":[]}"#;
    let policy: ResolvedLaunchPolicy = serde_json::from_str(historical).unwrap();
    policy.validate_direct_inputs().unwrap();
    assert_eq!(policy.thinking_level, None);
    assert!(
        serde_json::to_value(&policy)
            .unwrap()
            .get("thinkingLevel")
            .is_none()
    );
    let invalid = historical.replace("\"tools\":[]", "\"thinkingLevel\":\"unknown\",\"tools\":[]");
    assert!(serde_json::from_str::<ResolvedLaunchPolicy>(&invalid).is_err());
}
