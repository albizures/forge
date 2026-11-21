//! T21: actual CLI -> RPC -> daemon -> host runtime purge after the real supervised
//! retirement. Forge eligibility is the real retirement result; nothing is inserted.
use super::{
    AgentDispatchRepository, Command, Path, RecordRepository, SqliteProjectRepository,
    SupervisedDispatch, WorkspaceRepository, fs,
};
use forge_rpc::{RuntimePurgeRecord, RuntimePurgeRemoval, RuntimePurgeState};

fn cli(adapter: &SupervisedDispatch, args: &[&str]) -> std::process::Output {
    Command::new(&adapter.cli)
        .env_clear()
        .arg("dispatcher")
        .args(args)
        .args([
            "--json",
            "--server-url",
            &format!("{}/rpc", adapter.endpoint),
        ])
        .output()
        .unwrap()
}

fn purge(
    adapter: &SupervisedDispatch,
    workspace: &str,
    assignment: &str,
    operation: &str,
) -> Result<RuntimePurgeRecord, String> {
    let output = cli(
        adapter,
        &[
            "assignment-purge-runtime",
            assignment,
            "--workspace",
            workspace,
            "--operation-id",
            operation,
        ],
    );
    if output.status.success() {
        Ok(serde_json::from_slice(&output.stdout).unwrap())
    } else {
        assert!(output.stdout.is_empty());
        Err(String::from_utf8(output.stderr).unwrap())
    }
}

fn names(directory: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn tree(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![];
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() {
                out.push((
                    entry.path().display().to_string(),
                    fs::read(entry.path()).unwrap(),
                ));
            }
        }
    }
    out.sort();
    out
}

/// After `assignment-retire` reconciled the original host's retirement.
#[allow(clippy::too_many_lines)] // One ordered operator journey with its refusals.
pub(super) fn run(
    adapter: &SupervisedDispatch,
    store: &SqliteProjectRepository,
    assignment: &str,
    record: u64,
    directory: &Path,
    worktree: &Path,
) {
    // Scope denial: another Workspace cannot name this Assignment, and nothing is journaled.
    let other = tempfile::tempdir().unwrap();
    store
        .create_workspace("other", "Other", &other.path().canonicalize().unwrap())
        .unwrap();
    let denied = purge(adapter, "other", assignment, "purge-1").unwrap_err();
    assert!(denied.contains("durable-scope-denied"), "{denied}");
    // Ineligible: retirement placed a Dispatch Hold, which still blocks deletion.
    let held = purge(adapter, "test", assignment, "purge-1").unwrap_err();
    assert!(
        held.contains("runtime-purge-uncertainty-retained"),
        "{held}"
    );
    let status = cli(
        adapter,
        &["assignment-status", assignment, "--workspace", "test"],
    );
    assert!(status.status.success());
    assert!(
        serde_json::from_slice::<serde_json::Value>(&status.stdout).unwrap()["runtime_purge"]
            .is_null()
    );
    let lifecycle = names(directory);
    for name in [
        "retirement-intent.json",
        "retirement-receipt.json",
        "owner.json",
        "owner.lock",
    ] {
        assert!(lifecycle.iter().any(|n| n == name), "{name}: {lifecycle:?}");
    }
    // The operator clears the retirement hold through the ordinary command.
    let resume = cli(adapter, &["resume", &record.to_string()]);
    assert!(
        resume.status.success(),
        "{}",
        String::from_utf8_lossy(&resume.stderr)
    );
    // The synthetic test worker writes `controlled-*` fixture files that production
    // never writes. Purge refuses them (retained diagnostic), and the same operation
    // retries after the test removes only them.
    assert!(lifecycle.iter().any(|n| n.starts_with("controlled")));
    let unsafe_content = purge(adapter, "test", assignment, "purge-1").unwrap_err();
    assert!(
        unsafe_content.contains("runtime-purge-storage-unsafe"),
        "{unsafe_content}"
    );
    assert_eq!(names(directory), lifecycle, "nothing removed on refusal");
    let status = cli(
        adapter,
        &["assignment-status", assignment, "--workspace", "test"],
    );
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["runtime_purge"]["state"], "retained");
    assert_eq!(
        status["runtime_purge"]["diagnostic"],
        "runtime-purge-storage-unsafe"
    );
    for name in &lifecycle {
        if name.starts_with("controlled") {
            fs::remove_file(directory.join(name)).unwrap();
        }
    }
    // A conflicting operation cannot replace the journaled one.
    let conflict = purge(adapter, "test", assignment, "purge-2").unwrap_err();
    assert!(conflict.contains("runtime-purge-conflict"), "{conflict}");
    let audit = (
        store.agent_assignment_info(assignment).unwrap(),
        store.list_record_checks(record).unwrap(),
    );
    let work = tree(worktree);
    let done = purge(adapter, "test", assignment, "purge-1").unwrap();
    assert_eq!(done.state, RuntimePurgeState::Purged);
    assert_eq!(done.removal, Some(RuntimePurgeRemoval::Removed));
    assert!(done.diagnostic.is_none());
    assert!(!directory.exists());
    // Exact retry returns the recorded result and never recreates storage.
    assert_eq!(purge(adapter, "test", assignment, "purge-1").unwrap(), done);
    assert!(!directory.exists());
    let conflict = purge(adapter, "test", assignment, "purge-2").unwrap_err();
    assert!(conflict.contains("runtime-purge-conflict"), "{conflict}");
    assert_eq!(
        (
            store.agent_assignment_info(assignment).unwrap(),
            store.list_record_checks(record).unwrap(),
        ),
        audit
    );
    assert_eq!(tree(worktree), work);
    // Human output states this operation removed the directory.
    let human = Command::new(&adapter.cli)
        .env_clear()
        .args([
            "dispatcher",
            "assignment-purge-runtime",
            assignment,
            "--workspace",
            "test",
            "--operation-id",
            "purge-1",
            "--server-url",
            &format!("{}/rpc", adapter.endpoint),
        ])
        .output()
        .unwrap();
    assert!(human.status.success());
    assert!(
        String::from_utf8_lossy(&human.stdout)
            .contains("this operation removed the runtime directory")
    );
    // Status reports the completed purge and how it ended. (Transcript-after-purge
    // is covered by the host composition; this daemon has no transcript reader.)
    let status = cli(
        adapter,
        &["assignment-status", assignment, "--workspace", "test"],
    );
    assert!(status.status.success());
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["runtime_purge"]["state"], "purged");
    assert_eq!(status["runtime_purge"]["removal"], "removed");
}
