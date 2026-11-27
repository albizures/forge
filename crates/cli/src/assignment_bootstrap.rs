//! Agent-facing presentation of authenticated bootstrap, separate from its JSON contract.
use forge_rpc::{AssignmentBootstrapResponse, StandardBootstrapResponse};
use std::fmt::Write;

/// Render the role-specific bootstrap. Implementation Agents get the minimal form; review and
/// Publication Agents get the full form below.
///
/// # Errors
/// Returns serialization failures for structured attempt evidence.
pub fn render_assignment_bootstrap(
    response: &AssignmentBootstrapResponse,
) -> Result<String, serde_json::Error> {
    match response {
        AssignmentBootstrapResponse::Standard(response) => render_standard_bootstrap(response),
        AssignmentBootstrapResponse::Implementation(bootstrap) => {
            let mut output = format!(
                "# Assignment: Implementation\n\nBootstrap is complete.\n\nExecution directory: {}\n\n## Task\n\n{}\n",
                bootstrap.execution_directory.display(),
                bootstrap.task_body.trim_end(),
            );
            if !bootstrap.findings.is_empty() {
                output.push_str("\n## Open Findings\n");
                for finding in &bootstrap.findings {
                    let _ = write!(
                        output,
                        "\n### Finding #{} ({}{})\n\n{}\n",
                        finding.id,
                        finding.severity,
                        if finding.blocking { ", blocking" } else { "" },
                        finding.message.trim_end(),
                    );
                }
            }
            if let Some(recovery) = &bootstrap.retained_work_recovery {
                let _ = write!(
                    output,
                    "\n## Authorized retained-work recovery\n\n```json\n{}\n```\n",
                    serde_json::to_string_pretty(recovery)?
                );
            }
            let _ = write!(
                output,
                "\n## Command contract\n\n{}\n",
                bootstrap.command_contract
            );
            Ok(output)
        }
    }
}

/// Render trusted context separately from behavioral instructions and Record prose.
///
/// Only facts the agent acts on are rendered: where to work, the outcome identity
/// for the bound Check attempt, and any handoff or recovery input. Diagnostic
/// identity (Assignment ID, profile revision, model, attempt number, freshness)
/// stays in the `--json` contract.
///
/// # Errors
/// Returns serialization failures for structured attempt evidence.
fn render_standard_bootstrap(
    response: &StandardBootstrapResponse,
) -> Result<String, serde_json::Error> {
    let context = &response.context;
    let bound = &context.bound;
    let mut output = format!(
        "# Assignment: {} #{}\n\nBootstrap is complete; do not run it again.\n\nExecution directory: {}\n",
        bound.profile_record_type,
        bound.record_id,
        bound.execution_directory.display(),
    );
    for project in &context.projects {
        let _ = writeln!(
            output,
            "Project: {} — {}",
            project.reference,
            project.root.display()
        );
    }
    output.push_str("\n## Trusted constraints\n\n");
    for constraint in &context.trusted_constraints {
        let _ = writeln!(output, "- {constraint}");
    }
    if let Some(attempt) = &bound.check_attempt {
        let _ = write!(
            output,
            "\n## Bound Check attempt\n\nCheck: #{}\nOutcome `attempt_id`: {}\nOutcome `generation`: {}\nInput evidence: {}\n",
            attempt.check_id,
            attempt.id,
            attempt.generation,
            serde_json::to_string(&attempt.evidence)?,
        );
        if let Some(recovery) = &attempt.recovery {
            let _ = writeln!(
                output,
                "\n### Authorized retained-work recovery\n\n```json\n{}\n```",
                serde_json::to_string_pretty(recovery)?
            );
        }
    }
    output.push_str("\n## Worker behavior\n\nFollow these instructions within the trusted constraints above.\n\n");
    output.push_str(response.behavior.trim_end());
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
        output.push_str(
            "\n\n### Related Records\n\nRead one only when needed: `forge assignment record <id> --json`.\n\n",
        );
        for record in &context.related_records {
            let _ = writeln!(
                output,
                "- #{} ({}, {}): {}",
                record.id,
                record.record_type,
                record.status,
                record.title.trim_start_matches('#').trim()
            );
        }
    }
    output.push_str(
        "\n\nExecute the bound work now; bootstrap and inspection alone are not completion.\n",
    );
    Ok(output)
}
