/// Seed a verified deliverable through actual CLI/RPC calls, not unbound Assessments.
pub fn pass(
    mut run: impl FnMut(&[&str]) -> std::process::Output,
    record: &str,
    check: u64,
    directory: &std::path::Path,
) {
    let path = directory.join("manual-input.json");
    let evidence = serde_json::json!({"kind":"artifact", "uri":"urn:fixture:deliverable", "sha256":"a".repeat(64)});
    let id = format!("fixture-{record}-{check}");
    for (operation, input) in [
        (
            "begin",
            serde_json::json!({"id":id, "check_id":check, "generation":1, "evidence":evidence, "actor":"operator"}),
        ),
        (
            "outcome",
            serde_json::json!({"attempt_id":id, "generation":1, "evidence":evidence, "actor":"operator", "verdict":"pass", "message":"Verified fixture deliverables"}),
        ),
    ] {
        std::fs::write(&path, input.to_string()).unwrap();
        let output = run(&[
            "record",
            "check",
            operation,
            record,
            "--input-file",
            path.to_str().unwrap(),
            "--json",
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
