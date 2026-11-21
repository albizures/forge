//! Read-only operator transport and presentation for durable status and transcripts.
use crate::RecordEndpoint;
use forge_rpc::{
    AgentDispatchApiClient, AssignmentDurableStatusResponse, AssignmentInfoRequest,
    AssignmentTranscriptRequest, AssignmentTranscriptResponse, RecordSelection,
    SelectedRecordRequest, TranscriptHistory,
};
use jsonrpsee::http_client::HttpClientBuilder;
use std::{fmt::Write, time::Duration};

fn selection(endpoint: &RecordEndpoint) -> Result<RecordSelection, String> {
    let cwd = std::env::current_dir().map_err(|_| "could not resolve client cwd")?;
    Ok(RecordSelection {
        workspace: endpoint.workspace.clone(),
        cwd: Some(cwd.to_string_lossy().into_owned()),
    })
}

fn client(endpoint: &RecordEndpoint) -> Result<jsonrpsee::http_client::HttpClient, String> {
    // The daemon's reader deadline is 30 seconds; leave time for its reply.
    HttpClientBuilder::default()
        .request_timeout(Duration::from_secs(40))
        .build(&endpoint.url)
        .map_err(|_| "invalid Forge daemon endpoint".to_string())
}

/// # Errors
/// Returns routing, transport or daemon read errors. Nothing is started or repaired.
pub async fn request_assignment_durable_status(
    endpoint: impl Into<RecordEndpoint>,
    assignment_id: String,
) -> Result<AssignmentDurableStatusResponse, String> {
    let endpoint = endpoint.into();
    client(&endpoint)?
        .assignment_durable_status(SelectedRecordRequest {
            selection: selection(&endpoint)?,
            record: AssignmentInfoRequest { assignment_id },
        })
        .await
        .map_err(|error| {
            format!("could not inspect durable Assignment status through Forge daemon: {error}")
        })
}

/// # Errors
/// Returns routing, validation, transport or daemon read errors. Nothing is started or repaired.
pub async fn request_assignment_transcript(
    endpoint: impl Into<RecordEndpoint>,
    request: AssignmentTranscriptRequest,
) -> Result<AssignmentTranscriptResponse, String> {
    let endpoint = endpoint.into();
    client(&endpoint)?
        .assignment_transcript(SelectedRecordRequest {
            selection: selection(&endpoint)?,
            record: request,
        })
        .await
        .map_err(|error| {
            format!("could not read committed transcript through Forge daemon: {error}")
        })
}

fn list(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(", ")
    }
}

/// Persisted facts only; never claims a process is running, absent or safe to clean.
#[must_use]
pub fn render_durable_status(status: &AssignmentDurableStatusResponse) -> String {
    let a = &status.assignment;
    let mut out = format!(
        "Assignment {} ({}) — Task {} ({})\nClosed: {} — Closure reason: {}\nCapacity reserved: {} — Dispatch Hold: {}\n",
        a.id,
        a.state,
        a.record_id,
        a.task_status,
        a.closed_at_millis
            .map_or_else(|| "no".into(), |t| t.to_string()),
        a.closure_reason.as_deref().unwrap_or("none"),
        a.capacity_reserved,
        a.dispatch_hold.as_deref().unwrap_or("none"),
    );
    match &status.runtime {
        None => out.push_str("Durable runtime: none retained (not proof that no agent exists)\n"),
        Some(r) => {
            let lease = r.latest_lease_sequence.map_or_else(
                || "no lease".into(),
                |s| {
                    format!(
                        "latest sequence {s}, generation {}, boot-clock issued {} expires {} (informational, not liveness)",
                        r.latest_lease_generation.unwrap_or_default(),
                        r.latest_lease_issued_boottime_millis.unwrap_or_default(),
                        r.latest_lease_expires_boottime_millis.unwrap_or_default()
                    )
                },
            );
            write!(
                out,
                "Durable runtime: {} runtime {} operation {} attempt {} incarnation {}\nProfile revision {} — Forge {} — component {}\nAdmission: {}{}\nAuthorization: {} ({} retained; {lease})\n",
                r.backend, r.runtime_id, r.operation_id, r.attempt_id, r.incarnation_id,
                r.profile_revision, r.forge_revision, r.component_hash, r.admission,
                r.rejection_code.as_ref().map_or_else(String::new, |c| format!(" ({c})")),
                r.lease_state, r.lease_count,
            )
            .expect("writing to a string cannot fail");
        }
    }
    writeln!(
        out,
        "Missing Outcome: {}\nUncertainty: {}\nRetirement/recovery blockers: {}",
        if status.missing_outcome {
            "yes (unresolved; capacity retained)"
        } else {
            "no retained marker"
        },
        list(&status.uncertainty),
        list(&status.retirement_blockers),
    )
    .expect("writing to a string cannot fail");
    for note in &status.notes {
        writeln!(out, "Note: {note}").expect("writing to a string cannot fail");
    }
    out.push_str("Transcript: forge dispatcher assignment-transcript ");
    out.push_str(&a.id);
    out
}

/// Entry content is bounded runtime data and is printed verbatim on its own lines.
#[must_use]
pub fn render_transcript(page: &AssignmentTranscriptResponse) -> String {
    let mut out = String::new();
    match page.history {
        TranscriptHistory::Unavailable => {
            let _ = writeln!(
                out,
                "History unavailable ({}). No runtime was started, repaired or modified; do not treat this as an empty transcript.",
                page.reason.as_deref().unwrap_or("unknown")
            );
        }
        TranscriptHistory::NotStarted => {
            out.push_str("History not started: no committed conversation exists yet (not proof that no agent exists).\n");
        }
        TranscriptHistory::Available => {
            for entry in &page.entries {
                let _ = writeln!(
                    out,
                    "#{} {}{}{}\n{}",
                    entry.id,
                    entry.kind,
                    entry
                        .by_task_id
                        .as_ref()
                        .map_or_else(String::new, |t| format!(" (task {t})")),
                    if entry.truncated {
                        " [content truncated]"
                    } else {
                        ""
                    },
                    entry.content
                );
            }
            if page.entries.is_empty() {
                out.push_str("No committed entries after this cursor.\n");
            }
        }
    }
    let next = page.next_after.as_deref().map_or_else(
        || "none".into(),
        |after| {
            format!(
                "forge dispatcher assignment-transcript {} --after {after}",
                page.assignment_id
            )
        },
    );
    let _ = write!(
        out,
        "More committed entries: {}\nContinue: {next}\nA live Assignment can append later; an empty page is not end of history.",
        if page.more {
            "yes"
        } else {
            "none at read time"
        }
    );
    out
}
