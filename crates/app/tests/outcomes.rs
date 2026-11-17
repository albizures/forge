use forge_app::{
    checks::Verdict,
    outcomes::{Evidence, OutcomeDisposition, OutcomeInput},
};

#[test]
fn only_interruption_requires_unchanged_evidence_not_prose_or_failure() {
    let bound = Evidence::Repository {
        repository: "/repo".into(),
        commit: "a".repeat(40),
    };
    let mut input = OutcomeInput {
        attempt_id: "attempt".into(),
        generation: 1,
        evidence: bound.clone(),
        verdict: Verdict::Fail,
        actor: "agent".into(),
        message: "Interrupted; please continue".into(),
        disposition: OutcomeDisposition::Interrupted,
        findings: vec![],
        verifications: vec![],
    };
    input.validate_interruption_evidence(&bound).unwrap();
    input.evidence = Evidence::Repository {
        repository: "/repo".into(),
        commit: "b".repeat(40),
    };
    assert_eq!(
        input
            .validate_interruption_evidence(&bound)
            .unwrap_err()
            .code,
        "interrupted-evidence-changed"
    );
    for disposition in [
        OutcomeDisposition::Complete,
        OutcomeDisposition::HumanInput,
        OutcomeDisposition::Handback,
        OutcomeDisposition::Continuation(forge_app::outcomes::ContinuationHandoff {
            stopping_reason: "Concrete execution constraint".into(),
            completed_work: "Checkpoint".into(),
            verification: "Focused tests passed; full gates unrun".into(),
            remaining_work: "Finish".into(),
        }),
    ] {
        input.disposition = disposition;
        input.validate_interruption_evidence(&bound).unwrap();
    }
    input.validate_continuation(true, true).unwrap();
    for (implementation, authenticated) in [(false, true), (true, false), (false, false)] {
        assert_eq!(
            input
                .validate_continuation(implementation, authenticated)
                .unwrap_err()
                .code,
            "invalid-continuation"
        );
    }
    let json = serde_json::to_value(&input).unwrap();
    assert!(json["disposition"]["continuation"].is_object());
    assert_eq!(
        serde_json::from_value::<OutcomeInput>(json.clone()).unwrap(),
        input
    );
    for field in [
        "stopping_reason",
        "completed_work",
        "verification",
        "remaining_work",
    ] {
        let mut missing = json.clone();
        missing["disposition"]["continuation"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(serde_json::from_value::<OutcomeInput>(missing).is_err());
    }
    input.verdict = Verdict::Pass;
    assert!(input.validate_continuation(true, true).is_err());
}
