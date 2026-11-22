use forge_app::{
    agent_profiles::{ResourceCatalog, ThinkingLevel},
    outcomes::{OutcomeDisposition, OutcomeInput},
};
use std::fs;

const PROFILE: &str = include_str!("../../../docs/profiles/implementation.md");

#[test]
fn implementation_continuations_profile_resolves_with_stopping_guidance() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let operator = root.path().join("operator");
    fs::create_dir_all(&operator).unwrap();
    fs::create_dir_all(workspace.join(".agents/profiles")).unwrap();
    let path = workspace.join(".agents/profiles/implementation.md");
    fs::write(&path, PROFILE).unwrap();
    let catalog = ResourceCatalog::discover(&operator, &workspace).unwrap();
    let revised = catalog.resolve(&workspace, &path).unwrap();
    assert_eq!(revised.runtime, "herdr/pi");
    assert_eq!(revised.model, "openai-codex/gpt-6.1-terra");
    assert_eq!(revised.thinking_level, Some(ThinkingLevel::Medium));
    for instruction in [
        "keep implementing and testing until the bound Check is met",
        "a small successful commit, or remaining complexity alone is insufficient",
        "Success: `pass` / `complete`",
        "Substantive Implementation failure: `fail` / `complete`",
        "Human decision or missing prerequisite: `fail` / `human-input`",
        "Unfinished work alone is not interruption",
        "including every unrun required gate",
        "Changed evidence is not certified semantic progress",
        "operator-authorized",
        "stop mutating the checkout",
    ] {
        assert!(
            revised
                .behavior
                .to_lowercase()
                .contains(&instruction.to_lowercase()),
            "missing {instruction}"
        );
    }
}

fn json_examples(document: &str) -> Vec<serde_json::Value> {
    document
        .split("```json\n")
        .skip(1)
        .map(|block| serde_json::from_str(block.split_once("\n```").unwrap().0).unwrap())
        .collect()
}

#[test]
fn implementation_continuations_documented_outcomes_validate_and_reject_invalid_handoffs() {
    let examples = json_examples(PROFILE);
    assert_eq!(examples.len(), 2);
    let mut outcome = examples[0].clone();
    outcome["evidence"]["commit"] = "a".repeat(40).into();
    let pass: OutcomeInput = serde_json::from_value(outcome.clone()).unwrap();
    pass.validate_continuation(true, true).unwrap();
    // The second example is explicitly the disposition value, not a whole outcome.
    assert!(serde_json::from_value::<OutcomeInput>(examples[1].clone()).is_err());
    outcome["verdict"] = "fail".into();
    outcome["disposition"] = examples[1].clone();
    let continuation: OutcomeInput = serde_json::from_value(outcome.clone()).unwrap();
    continuation.validate_continuation(true, true).unwrap();
    for (implementation, authenticated) in [(false, true), (true, false)] {
        assert!(
            continuation
                .validate_continuation(implementation, authenticated)
                .is_err()
        );
    }
    for field in [
        "stopping_reason",
        "completed_work",
        "verification",
        "remaining_work",
    ] {
        let mut blank = outcome.clone();
        blank["disposition"]["continuation"][field] = " ".into();
        assert!(
            serde_json::from_value::<OutcomeInput>(blank)
                .unwrap()
                .validate_continuation(true, true)
                .is_err()
        );
        let mut missing = outcome.clone();
        missing["disposition"]["continuation"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(serde_json::from_value::<OutcomeInput>(missing).is_err());
    }
    let mut passing = outcome.clone();
    passing["verdict"] = "pass".into();
    assert!(
        serde_json::from_value::<OutcomeInput>(passing)
            .unwrap()
            .validate_continuation(true, true)
            .is_err()
    );
    let mut with_finding = continuation.clone();
    with_finding
        .findings
        .push(forge_app::outcomes::OutcomeFinding {
            severity: forge_app::findings::Severity::Major,
            blocking: Some(true),
            message: "Not allowed on a Continuation".into(),
        });
    assert!(with_finding.validate_continuation(true, true).is_err());
    for disposition in ["complete", "human-input", "interrupted"] {
        outcome["disposition"] = disposition.into();
        let input: OutcomeInput = serde_json::from_value(outcome.clone()).unwrap();
        input
            .validate_interruption_evidence(&input.evidence)
            .unwrap();
        assert!(!matches!(
            input.disposition,
            OutcomeDisposition::Continuation(_)
        ));
    }
    let dispatch = json_examples(include_str!("../../../docs/dispatch.md"));
    for example in dispatch.iter().filter(|v| v.get("continuation").is_some()) {
        outcome["disposition"] = example.clone();
        serde_json::from_value::<OutcomeInput>(outcome.clone())
            .unwrap()
            .validate_continuation(true, true)
            .unwrap();
    }
}

#[test]
fn implementation_continuations_glossary_matches_approved_block() {
    let block = "**Continuation**:\nA deliberate handoff of unfinished Implementation work for another Agent to continue without passing the bound Check.\n_Avoid_: interruption, implementation failure, Task completion";
    assert!(include_str!("../../GLOSSARY.md").contains(block));
}
