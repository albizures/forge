use forge_app::{
    RecordRepository,
    checks::Verdict,
    outcomes::{AttemptInput, Evidence, OutcomeInput},
};

pub fn pass(repository: &dyn RecordRepository, record: u64, check: u64) {
    let evidence = Evidence::Artifact {
        uri: "urn:fixture:deliverable".into(),
        sha256: "a".repeat(64),
    };
    let id = format!("fixture-{record}-{check}");
    repository
        .begin_record_attempt(
            record,
            &AttemptInput {
                id: id.clone(),
                check_id: check,
                generation: 1,
                evidence: evidence.clone(),
                actor: "test operator".into(),
            },
        )
        .unwrap();
    repository
        .submit_record_outcome(
            record,
            &OutcomeInput {
                attempt_id: id,
                generation: 1,
                evidence,
                verdict: Verdict::Pass,
                actor: "test operator".into(),
                message: "Fixture deliverables verified".into(),
            },
        )
        .unwrap();
}
