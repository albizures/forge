//! Read-only status/transcript beside an actual lock-owning supervisor, after it
//! stops, with retained closed state and corrupt storage. Runtime entries are
//! storage fixtures; no Harness, model, tool, lease or scheduler is started.
use super::effect_admission::{component_hash, components};
use super::*;
use forge_rpc::{AssignmentDurableStatusResponse, AssignmentTranscriptRequest, TranscriptHistory};
use forge_server::durable_authorization::transcript::page;
use std::io::Write;

fn fixture_run(f: &Fixture, mode: &str, count: u32) {
    let components = components();
    let script = components
        .inspector
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test/transcript-fixture.mjs");
    let mut child = Command::new(&components.node)
        .env_clear()
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            &serde_json::to_vec(&serde_json::json!({
                "directory": f.directory, "launch": f.launch, "mode": mode, "count": count,
            }))
            .unwrap(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn request(after: Option<&str>, limit: u32) -> AssignmentTranscriptRequest {
    AssignmentTranscriptRequest {
        assignment_id: "assignment".into(),
        after: after.map(Into::into),
        limit: Some(limit),
    }
}

fn read(
    runtime: &tokio::runtime::Runtime,
    f: &Fixture,
    request: &AssignmentTranscriptRequest,
) -> forge_rpc::AssignmentTranscriptResponse {
    let components = components();
    runtime
        .block_on(page(
            &f.store,
            "test",
            &f.launch,
            &components.node,
            &components.inspector,
            request,
        ))
        .unwrap()
}

fn all(runtime: &tokio::runtime::Runtime, f: &Fixture, mut after: Option<String>) -> Vec<String> {
    let mut ids = Vec::new();
    loop {
        let result = read(runtime, f, &request(after.as_deref(), 2));
        assert_eq!(result.history, TranscriptHistory::Available);
        ids.extend(result.entries.iter().map(|e| e.id.clone()));
        after = result.next_after;
        if !result.more {
            return ids;
        }
    }
}

fn state(f: &Fixture) -> (Vec<u8>, i64, usize, bool, bool) {
    let info = f.store.agent_assignment_info("assignment").unwrap();
    let leases: i64 = rusqlite::Connection::open(&f.database)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM assignment_authorization_leases",
            [],
            |r| r.get(0),
        )
        .unwrap();
    (
        fs::read(f.directory.join("runtime.sqlite")).unwrap(),
        leases,
        info.resources.len(),
        info.capacity_reserved(),
        info.dispatch_hold.is_some(),
    )
}

#[test]
#[allow(clippy::too_many_lines)] // One composed live/stopped/closed/corrupt scenario.
fn status_and_transcript_are_read_only_beside_live_stopped_closed_and_corrupt_runtimes() {
    let components = components();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut f = Fixture::with_component("model", component_hash(&components));
    fixture_run(&f, "seed", 5);
    let before = state(&f);

    // 1. Active supervisor/lock owner: paginate with live appends and no side effects.
    let first = read(&runtime, &f, &request(None, 2));
    assert_eq!((first.entries.len(), first.more), (2, true));
    fixture_run(&f, "append", 3);
    let ids = all(&runtime, &f, first.next_after.clone());
    let numbers: Vec<u64> = first
        .entries
        .iter()
        .map(|e| e.id.parse().unwrap())
        .chain(ids.iter().map(|id| id.parse().unwrap()))
        .collect();
    assert_eq!(numbers.len(), 8);
    assert!(
        numbers.windows(2).all(|w| w[0] < w[1]),
        "stable ascending identity: {numbers:?}"
    );
    let serialized = serde_json::to_string(&first).unwrap();
    for hidden in [
        f.directory.to_str().unwrap(),
        "runtime.sqlite",
        "owner.lock",
        "capability",
    ] {
        assert!(!serialized.contains(hidden), "leaked {hidden}");
    }
    let after = state(&f);
    // The fixture appends committed rows; the reader itself must not change retained
    // Forge state. Compare everything except runtime bytes against the live baseline.
    assert_eq!(&after.1.to_string(), &before.1.to_string());
    assert_eq!((after.2, after.3, after.4), (before.2, before.3, before.4));
    assert_eq!(
        rusqlite::Connection::open(&f.database)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM assignment_authorization_leases",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    let live_runtime_bytes = after.0;
    for _ in 0..2 {
        read(&runtime, &f, &request(None, 100));
    }
    assert_eq!(
        state(&f).0,
        live_runtime_bytes,
        "reader left runtime bytes unchanged"
    );
    f.child
        .try_wait()
        .unwrap()
        .is_none()
        .then_some(())
        .expect("owner still alive and unaffected");

    // 2. Retained uncertainty is visible and never cleared by reading.
    rusqlite::Connection::open(&f.database).unwrap().execute(
        "INSERT INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) VALUES ('assignment', 'missing-outcome', 'answer', 1)",
        [],
    ).unwrap();
    let reopened = open(&f.database);
    let status = |repository: &SqliteProjectRepository| {
        use forge_app::durable_execution::status::Repository as _;
        AssignmentDurableStatusResponse::compose(
            repository.agent_assignment_info("assignment").unwrap(),
            repository.durable_status("test", "assignment").unwrap(),
        )
    };
    let running = status(&reopened);
    assert!(running.missing_outcome && running.assignment.capacity_reserved);
    assert!(
        running
            .retirement_blockers
            .contains(&"missing-outcome".to_string())
    );
    let runtime_status = running.runtime.as_ref().unwrap();
    assert_eq!(
        (
            runtime_status.backend.as_str(),
            runtime_status.lease_state.as_str(),
            runtime_status.lease_count
        ),
        ("pi-durable", "never-granted", 0)
    );
    assert_eq!(runtime_status.admission, "pending");
    let json = serde_json::to_string(&running).unwrap();
    assert!(!json.contains("\"store\"") && !json.contains(f.directory.to_str().unwrap()));
    assert!(
        status(&reopened) == running,
        "status read is repeatable and writes nothing"
    );

    // 3. Stopped runner (crash): transcript and status remain readable, nothing resurrected.
    let _ = f.child.kill();
    let _ = f.child.wait();
    reopened
        .close_agent_assignment(
            "assignment",
            forge_app::AgentAssignmentClosureReason::Crashed,
        )
        .unwrap();
    let stopped = status(&reopened);
    assert!(
        stopped.assignment.closed_at_millis.is_some()
            && stopped.assignment.capacity_reserved
            && stopped.missing_outcome
    );
    assert_eq!(all(&runtime, &f, None).len(), 8);

    // 4. Corrupt runtime storage is reported unavailable, unchanged and not empty.
    let bytes = b"PRIVATE-CANARY not sqlite".to_vec();
    fs::write(f.directory.join("runtime.sqlite"), &bytes).unwrap();
    for name in ["runtime.sqlite-wal", "runtime.sqlite-shm"] {
        let _ = fs::remove_file(f.directory.join(name));
    }
    let corrupt = read(&runtime, &f, &request(None, 10));
    assert_eq!(corrupt.history, TranscriptHistory::Unavailable);
    assert_eq!(
        corrupt.reason.as_deref(),
        Some("runner-history-unavailable")
    );
    assert!(
        corrupt.entries.is_empty() && !serde_json::to_string(&corrupt).unwrap().contains("CANARY")
    );
    assert_eq!(fs::read(f.directory.join("runtime.sqlite")).unwrap(), bytes);
    let after_corrupt = state(&f);
    assert_eq!(after_corrupt.1, 0, "no lease was granted by inspection");

    // 5. Scope and request validation fail before reading.
    let denied = runtime.block_on(page(
        &f.store,
        "other",
        &f.launch,
        &components.node,
        &components.inspector,
        &request(None, 1),
    ));
    assert!(denied.is_err());
    for bad in [
        request(Some("0"), 1),
        request(Some("x"), 1),
        request(None, 0),
        request(None, 101),
    ] {
        assert!(
            runtime
                .block_on(page(
                    &f.store,
                    "test",
                    &f.launch,
                    &components.node,
                    &components.inspector,
                    &bad
                ))
                .is_err()
        );
    }
}
