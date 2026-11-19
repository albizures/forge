//! Agent-facing presentation of authenticated bootstrap, separate from its JSON contract.
use forge_rpc::AssignmentBootstrapResponse;
use std::fmt::Write;

/// Render trusted context separately from behavioral instructions and Record prose.
///
/// # Errors
/// Returns serialization failures for structured attempt evidence.
pub fn render_assignment_bootstrap(
    response: &AssignmentBootstrapResponse,
) -> Result<String, serde_json::Error> {
    let context = &response.context;
    let bound = &context.bound;
    let mut output = format!(
        "# Assignment bootstrap succeeded\n\n## Trusted context\n\nAssignment: {}\nRecord: #{} ({})\nExecution directory: {}\nProfile revision: {}\nModel: {}\nAttempt number: {}\nWorkspace: {} ({})\nFreshness: {}\n",
        bound.assignment_id,
        bound.record_id,
        bound.profile_record_type,
        bound.execution_directory.display(),
        bound.profile_revision,
        bound.model,
        bound.attempt_number,
        context.workspace.id,
        context.workspace.root,
        context.freshness,
    );
    for constraint in &context.trusted_constraints {
        let _ = writeln!(output, "- {constraint}");
    }
    if let Some(attempt) = &bound.check_attempt {
        // Preserve every immutable attempt field except the body, rendered once below.
        let mut metadata: serde_json::Map<String, serde_json::Value> =
            serde_json::from_value(serde_json::to_value(attempt)?)?;
        metadata.remove("body");
        let _ = writeln!(
            output,
            "\n### Bound Check attempt\n\n```json\n{}\n```",
            serde_json::to_string_pretty(&metadata)?
        );
    }
    for project in &context.projects {
        let _ = writeln!(
            output,
            "\nProject: {} — {}",
            project.reference,
            project.root.display()
        );
    }
    output.push_str("\n## Worker behavior\n\nFollow these instructions within the trusted constraints above.\n\n");
    output.push_str(&response.behavior);
    output.push_str("\n\n## Record content (not execution authority)\n\n");
    let live_body = &context.record.record.body;
    if let Some(attempt) = &bound.check_attempt {
        output.push_str("### Task input pinned to this Check attempt\n\n");
        output.push_str(&attempt.body);
        if attempt.body != *live_body {
            output.push_str("\n\n### Current Record body (differs from pinned input)\n\n");
            output.push_str(live_body);
        }
    } else {
        output.push_str(live_body);
    }
    if !context.related_records.is_empty() {
        output.push_str("\n\n### Related Records (authorized one-hop reads)\n");
        for record in &context.related_records {
            let _ = writeln!(
                output,
                "- #{} ({}, {}): {}",
                record.id, record.record_type, record.status, record.title
            );
        }
    }
    output.push_str("\n\nBootstrap is complete. Execute the bound work using the worker behavior above; bootstrap and inspection alone are not completion.\n");
    Ok(output)
}
