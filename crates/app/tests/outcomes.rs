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
    ] {
        input.disposition = disposition;
        input.validate_interruption_evidence(&bound).unwrap();
    }
    let retired = serde_json::json!({
        "attempt_id": "attempt", "generation": 1, "verdict": "fail",
        "evidence": {"kind": "repository", "repository": "/repo", "commit": "a".repeat(40)},
        "actor": "agent", "message": "m",
        "disposition": {"continuation": {
            "stopping_reason": "r", "completed_work": "c", "verification": "v", "remaining_work": "w"
        }}
    });
    assert!(serde_json::from_value::<OutcomeInput>(retired).is_err());
}
