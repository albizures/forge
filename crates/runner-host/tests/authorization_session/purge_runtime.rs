//! Explicit runtime purge against a real supervisor-owned runtime directory and a real
//! Forge store: live ownership and unrecognized content block deletion untouched, an
//! eligible stopped runtime is removed exactly once, and Forge's audit stays intact.
use super::*;
use forge_app::durable_execution::purge::{Repository as _, State};
use forge_rpc::{AssignmentTranscriptRequest, TranscriptHistory};
use forge_server::durable_authorization::purge::run;

fn audit(database: &Path) -> Vec<String> {
    let c = rusqlite::Connection::open(database).unwrap();
    [
        "agent_assignments",
        "durable_assignment_runtimes",
        "durable_assignment_retirements",
        "agent_assignment_resources",
        "record_attempts",
        "dispatch_holds",
    ]
    .iter()
    .map(|table| {
        let mut statement = c
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1, 2"))
            .unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |r| {
                Ok((0..columns)
                    .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                    .collect::<Vec<_>>()
                    .join("|"))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
            .join("\n")
    })
    .collect()
}

fn listing(directory: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                fs::read(e.path()).unwrap_or_default(),
            )
        })
        .collect();
    files.sort();
    files
}

#[test]
#[allow(clippy::too_many_lines)] // One composed live/stopped/unsafe/eligible/purged scenario.
fn purge_requires_exclusive_eligibility_removes_only_bound_history_and_never_resurrects() {
    let mut f = Fixture::new("model");
    // A neighbouring runtime and the Managed Worktree must never be touched.
    let neighbour = f.directory.parent().unwrap().join("0".repeat(64));
    fs::create_dir(&neighbour).unwrap();
    fs::set_permissions(&neighbour, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(neighbour.join("runtime.sqlite"), b"neighbour").unwrap();
    let worktree = f.root.join("worktree");
    fs::write(worktree.join("work.txt"), b"unpublished work").unwrap();

    // Open Assignment: Forge rejects before any filesystem work.
    assert_eq!(
        run(&f.store, "test", &f.launch, "purge").unwrap_err().code,
        "runtime-purge-assignment-open"
    );
    assert!(
        f.store
            .purge_record("test", "assignment")
            .unwrap()
            .is_none()
    );

    // Make Forge-side retirement eligible (test-controlled synthetic closure).
    f.store
        .close_agent_assignment(
            "assignment",
            forge_app::AgentAssignmentClosureReason::Retired,
        )
        .unwrap();
    let c = rusqlite::Connection::open(&f.database).unwrap();
    c.execute(
        "UPDATE durable_assignment_runtimes SET admission_json = '{\"state\":\"accepted\",\"admission_id\":\"a\"}'",
        [],
    )
    .unwrap();
    let operation = serde_json::json!({
        "operation_id": "retire", "launch": f.launch,
        "owner": {"boot_id":"b","time_namespace":"t","host_pid":1,"host_start":"1","worker_pid":2,"worker_start":"2","lock_device":"1","lock_inode":"2"},
    });
    c.execute(
        "INSERT INTO durable_assignment_retirements(assignment_id, operation_json, reconciled_at_millis) VALUES ('assignment', ?1, 1)",
        [operation.to_string()],
    )
    .unwrap();
    c.execute(
        "UPDATE agent_assignment_resources SET retired_at_millis = 1 WHERE retired_at_millis IS NULL",
        [],
    )
    .unwrap();
    drop(c);
    let before_audit = audit(&f.database);
    let neighbour_before = listing(&neighbour);
    let worktree_before = listing(&worktree);

    // Live supervisor owner: rejected, journaled as retained, nothing deleted.
    let live = listing(&f.directory);
    let error = run(&f.store, "test", &f.launch, "purge").unwrap_err();
    assert_eq!(error.code, "runtime-purge-owner-active");
    assert_eq!(listing(&f.directory), live);
    let record = f.store.purge_record("test", "assignment").unwrap().unwrap();
    assert_eq!(record.state, State::Retained);
    assert_eq!(
        record.diagnostic.as_deref(),
        Some("runtime-purge-owner-active")
    );
    // A different operation cannot replace the journaled one.
    assert_eq!(
        run(&f.store, "test", &f.launch, "other").unwrap_err().code,
        "runtime-purge-conflict"
    );

    // Killing only the supervisor leaves the worker holding the shared lock: still
    // an active owner, so deletion stays refused.
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(f.directory.join("owner.json")).unwrap()).unwrap();
    let group = record["process_group"].as_u64().unwrap();
    let _ = f.child.kill();
    let _ = f.child.wait();
    assert_eq!(
        run(&f.store, "test", &f.launch, "purge").unwrap_err().code,
        "runtime-purge-owner-active"
    );
    // Stop the owner's whole process group. Unrecognized content still blocks.
    // The worker may already have exited with its supervisor; either way wait for
    // the shared lock to be free before expecting the next refusal reason.
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{group}")])
        .status();
    for _ in 0..100 {
        let lock = fs::File::open(f.directory.join("owner.lock")).unwrap();
        if fs2::FileExt::try_lock_exclusive(&lock).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let stopped = listing(&f.directory);
    assert!(stopped.iter().any(|(name, _)| name == "controlled-mode"));
    assert_eq!(
        run(&f.store, "test", &f.launch, "purge").unwrap_err().code,
        "runtime-purge-storage-unsafe"
    );
    assert_eq!(listing(&f.directory), stopped);

    // Remove the unrecognized fixture file; the exact retry now succeeds once.
    fs::remove_file(f.directory.join("controlled-mode")).unwrap();
    let done = run(&f.store, "test", &f.launch, "purge").unwrap();
    assert_eq!(done.state, State::Purged);
    assert!(!f.directory.exists());
    assert!(f.directory.parent().unwrap().exists());
    assert_eq!(run(&f.store, "test", &f.launch, "purge").unwrap(), done);
    assert!(
        !f.directory.exists(),
        "an exact retry never recreates storage"
    );

    // Forge audit, the neighbour and the worktree are untouched.
    assert_eq!(audit(&f.database), before_audit);
    assert_eq!(listing(&neighbour), neighbour_before);
    assert_eq!(listing(&worktree), worktree_before);

    // Inspection reports purged history without contacting any reader or recreating it.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let page = runtime
        .block_on(forge_server::durable_authorization::transcript::page(
            &f.store,
            "test",
            &f.launch,
            Path::new("/nonexistent/node"),
            Path::new("/nonexistent/inspect-effects.ts"),
            &AssignmentTranscriptRequest {
                assignment_id: "assignment".into(),
                after: None,
                limit: None,
            },
        ))
        .unwrap();
    assert_eq!(page.history, TranscriptHistory::Unavailable);
    assert_eq!(page.reason.as_deref(), Some("runtime-history-purged"));
    assert!(page.entries.is_empty() && !f.directory.exists());
}
