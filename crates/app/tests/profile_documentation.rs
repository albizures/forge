use forge_app::agent_profiles::{ProfileDocument, ThinkingLevel};

#[test]
fn documented_profile_explicitly_requests_low_thinking() {
    let documentation = include_str!("../../../docs/agent-profile-policy.md");
    let example = documentation
        .split_once("```markdown\n")
        .unwrap()
        .1
        .split_once("\n```")
        .unwrap()
        .0;
    let profile = ProfileDocument::parse(example).unwrap();
    assert_eq!(profile.thinking_level, ThinkingLevel::Low);
}
