//! Operator transcript pages from committed runtime history. Read-only: no host
//! ownership lock, retention write, Harness, profile source, credential or Git access.
use super::{inspection_process, runtime_directory};
use forge_app::{
    RegistrationError,
    durable_execution::{Launch, Repository},
};
use forge_rpc::{
    AssignmentTranscriptRequest, AssignmentTranscriptResponse, DEFAULT_TRANSCRIPT_LIMIT,
    durable_execution::transcript_inspection::{
        MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Request, Response,
    },
};
use forge_store::SqliteProjectRepository;
use std::{path::Path, time::Duration};

const TRANSCRIPT_INSPECTOR: &str = "inspect-transcript.ts";

fn unavailable() -> RegistrationError {
    RegistrationError::new(
        "durable-transcript-unavailable",
        "committed transcript inspection failed; no runtime was started, repaired or modified",
    )
}

/// The reader is the sibling of the explicitly configured matched effect inspector.
/// Both belong to one hash-pinned runner component; this is never RPC input or PATH
/// discovery. The reader independently verifies the hash recorded in the Launch.
fn reader(inspector: &Path) -> Result<std::path::PathBuf, RegistrationError> {
    Ok(inspector
        .parent()
        .ok_or_else(unavailable)?
        .join(TRANSCRIPT_INSPECTOR))
}

/// Read one committed page through the explicitly selected matched components. No
/// host lock is taken, so this works beside a live owner and for stopped runners.
/// # Errors
/// Rejects invalid requests, foreign scope/binding, unavailable or malformed
/// reader output and a replaced binding. Unavailable *history* is a successful
/// response with an explicit history state, not an error or an empty page.
pub async fn page(
    repository: &SqliteProjectRepository,
    workspace: &str,
    launch: &Launch,
    node: &Path,
    inspector: &Path,
    request: &AssignmentTranscriptRequest,
) -> Result<AssignmentTranscriptResponse, RegistrationError> {
    request.validate()?;
    let directory = runtime_directory::mapped(repository, workspace, launch, false)?;
    let wire = Request {
        protocol: forge_rpc::durable_execution::RUNNER_PROTOCOL_VERSION,
        launch: launch.clone(),
        after: request.after.clone(),
        limit: request.limit.unwrap_or(DEFAULT_TRANSCRIPT_LIMIT),
    };
    let input = serde_json::to_vec(&wire).map_err(|_| unavailable())?;
    if input.len() > MAX_REQUEST_BYTES || !node.is_absolute() || !inspector.is_absolute() {
        return Err(unavailable());
    }
    let bytes = inspection_process::read(
        node,
        &reader(inspector)?,
        &directory,
        &input,
        Duration::from_secs(30),
        MAX_RESPONSE_BYTES,
    )
    .await
    .map_err(|_| unavailable())?;
    let response: Response = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
    response.validate(&wire)?;
    // Re-authorize the immutable binding after external I/O; a closed Assignment
    // remains readable, but a foreign/replaced binding must not return a page.
    if repository
        .runtime(workspace, &launch.assignment_id)?
        .as_ref()
        .map(|r| &r.launch)
        != Some(launch)
    {
        return Err(unavailable());
    }
    Ok(AssignmentTranscriptResponse {
        assignment_id: launch.assignment_id.clone(),
        history: response.history,
        reason: response.code,
        entries: response.entries,
        next_after: response.next_after,
        more: response.more,
    })
}
