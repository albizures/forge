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

/// # Errors
/// Returns routing, validation, transport or daemon errors. Retry with the same
/// operation ID after an uncertain reply; it never creates a second record.
pub async fn request_assignment_follow_up(
    endpoint: impl Into<RecordEndpoint>,
    request: forge_rpc::FollowUpRequest,
) -> Result<forge_rpc::FollowUpReceipt, String> {
    let endpoint = endpoint.into();
    client(&endpoint)?
        .assignment_follow_up(SelectedRecordRequest {
            selection: selection(&endpoint)?,
            record: request,
        })
        .await
        .map_err(|error| format!("could not record follow-up through Forge daemon: {error}"))
}

/// # Errors
/// Returns routing, validation, transport or daemon errors. Retry with the same
/// operation ID after an uncertain reply or a retained diagnostic.
pub async fn request_assignment_purge_runtime(
    endpoint: impl Into<RecordEndpoint>,
    request: forge_rpc::RuntimePurgeRequest,
) -> Result<forge_rpc::RuntimePurgeRecord, String> {
    let endpoint = endpoint.into();
    client(&endpoint)?
        .assignment_purge_runtime(SelectedRecordRequest {
            selection: selection(&endpoint)?,
            record: request,
        })
        .await
        .map_err(|error| format!("could not purge runtime history through Forge daemon: {error}"))
}

fn purge_removal(record: &forge_rpc::RuntimePurgeRecord) -> &'static str {
    use forge_rpc::{RuntimePurgeRemoval as Removal, RuntimePurgeState as State};
    match (record.state, record.removal) {
        (_, Some(Removal::Removed)) => "; this operation removed the runtime directory",
        (_, Some(Removal::AlreadyAbsent)) => {
            "; the mapped runtime directory was already absent, so nothing was removed"
        }
        (State::Purged, None) => "; whether this operation removed it was not recorded",
        _ => "",
    }
}

#[must_use]
pub fn render_purge(record: &forge_rpc::RuntimePurgeRecord) -> String {
    format!(
        "Runtime history for Assignment {} is {} (operation {}{}).\nForge's Assignment, Outcomes, Assessments and audit records are unchanged; the deleted runtime history cannot be recreated or resumed.",
        record.assignment_id,
        record.state.as_str(),
        record.operation_id,
        purge_removal(record),
    )
}

#[must_use]
pub fn render_follow_up(receipt: &forge_rpc::FollowUpReceipt) -> String {
    format!(
        "Follow-up {} for Assignment {} {} (sequence {}, sha256 {}).\nBound to attempt {} generation {} profile revision {} runtime {}; unchanged by this request.\nRecorded input only: this does not prove the runtime received it, grant authority, or complete the Check. Retry with the same operation ID if unsure.",
        receipt.operation_id,
        receipt.assignment_id,
        if receipt.replayed {
            "was already recorded"
        } else {
            "recorded"
        },
        receipt.sequence,
        receipt.message_sha256,
        receipt.binding.attempt_id,
        receipt.binding.generation,
        receipt.binding.profile_revision,
        receipt.binding.runtime_id,
    )
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
    for f in &status.followups {
        writeln!(
            out,
            "Follow-up #{}: {} (sha256 {}, attempt {} generation {}; recorded input, delivery not proven)",
            f.sequence, f.operation_id, f.message_sha256, f.binding.attempt_id, f.binding.generation
        )
        .expect("writing to a string cannot fail");
    }
    if let Some(p) = &status.runtime_purge {
        writeln!(
            out,
            "Runtime history deletion {}: {}{}{}",
            p.operation_id,
            p.state.as_str(),
            purge_removal(p),
            p.diagnostic.as_ref().map_or_else(String::new, |d| format!(
                " ({d}; remaining storage retained)"
            )),
        )
        .expect("writing to a string cannot fail");
    }
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

/// # Errors
/// Returns transport or daemon read errors. Nothing is started or changed.
pub async fn request_durable_launch_status(
    endpoint: impl Into<RecordEndpoint>,
) -> Result<forge_rpc::DurableLaunchStatus, String> {
    let endpoint = endpoint.into();
    client(&endpoint)?
        .durable_launch_status()
        .await
        .map_err(|error| {
            format!("could not read durable launch policy through Forge daemon: {error}")
        })
}

/// # Errors
/// Returns validation, transport or daemon errors; a failed request records nothing.
pub async fn request_durable_launch_disable(
    endpoint: impl Into<RecordEndpoint>,
    request: forge_rpc::DurableLaunchDisableRequest,
) -> Result<forge_rpc::DurableLaunchStatus, String> {
    let endpoint = endpoint.into();
    client(&endpoint)?
        .durable_launch_disable(request)
        .await
        .map_err(|error| {
            format!("could not disable durable launches through Forge daemon: {error}")
        })
}

#[must_use]
pub fn render_durable_launch(status: &forge_rpc::DurableLaunchStatus) -> String {
    use forge_rpc::DurableLaunchState as State;
    let mut out = String::new();
    let state = match status.state {
        State::Withheld => "withheld (pending release verification)",
        State::Controlled => "controlled (verification composition only)",
        State::Disabled => "disabled (rolled back)",
    };
    let _ = writeln!(out, "New durable launches: {state}");
    let _ = writeln!(
        out,
        "Admitted by this daemon: {}",
        if status.new_launches_admitted {
            "yes"
        } else {
            "no"
        }
    );
    if let Some(rollback) = &status.rollback {
        let _ = writeln!(
            out,
            "Rollback: operation {} at {} ms{}\nReason: {}",
            rollback.operation_id,
            rollback.disabled_at_millis,
            if status.replayed {
                " (already recorded; unchanged)"
            } else {
                ""
            },
            rollback.reason
        );
    }
    for note in &status.notes {
        let _ = writeln!(out, "{note}");
    }
    out.trim_end().to_owned()
}

#[cfg(test)]
mod purge_render_tests {
    use super::render_purge;
    use forge_rpc::{RuntimePurgeRecord, RuntimePurgeRemoval, RuntimePurgeState};

    fn record(
        state: RuntimePurgeState,
        removal: Option<RuntimePurgeRemoval>,
    ) -> RuntimePurgeRecord {
        RuntimePurgeRecord {
            assignment_id: "a".into(),
            operation_id: "purge-1".into(),
            state,
            diagnostic: None,
            requested_at_millis: 1,
            completed_at_millis: Some(2),
            removal,
        }
    }

    #[test]
    fn removal_and_already_absent_and_unrecorded_render_distinctly() {
        let removed = render_purge(&record(
            RuntimePurgeState::Purged,
            Some(RuntimePurgeRemoval::Removed),
        ));
        let absent = render_purge(&record(
            RuntimePurgeState::Purged,
            Some(RuntimePurgeRemoval::AlreadyAbsent),
        ));
        let legacy = render_purge(&record(RuntimePurgeState::Purged, None));
        assert!(removed.contains("this operation removed the runtime directory"));
        assert!(absent.contains("already absent, so nothing was removed"));
        assert!(legacy.contains("not recorded"));
        let pending = render_purge(&record(RuntimePurgeState::Pending, None));
        assert!(!pending.contains("removed") && !pending.contains("recorded"));
    }
}
