//! Snapshot reads only; no Git/process proof is supplied by these store tests.
use super::*;
use forge_app::{
    ManagedWorktreeRepository as _, durable_execution::recovery::worktree::Repository as _,
};

fn bound() -> (Fixture, Claim) {
    let f = prepared();
    let id = candidate(&f).unwrap().attempt.record_id;
    let path = f.store.worktree_path(id).unwrap();
    let mut attempt = candidate(&f).unwrap().attempt;
    attempt.evidence = forge_app::outcomes::Evidence::Repository {
        repository: f.root.display().to_string(),
        commit: "a".repeat(40),
    };
    {
        let c = f.store.connection.lock().unwrap();
        c.execute(
            "UPDATE assignment_execution_bindings SET directory = ?1",
            [path.to_str().unwrap()],
        )
        .unwrap();
        c.execute(
            "UPDATE worktree_claims SET path = ?1",
            [path.to_str().unwrap()],
        )
        .unwrap();
        c.execute(
            "UPDATE record_attempts SET attempt_json = ?1 WHERE id = 'attempt'",
            [serde_json::to_string(&attempt).unwrap()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO task_base_commits VALUES (?1, ?2)",
            params![id.cast_signed(), "a".repeat(40)],
        )
        .unwrap();
    }
    let claim = claim(&f);
    f.store.claim_recovery("test", &claim).unwrap();
    (f, claim)
}

#[test]
fn recovery_worktree_snapshot_is_read_only_and_revalidates_claim_after_restart() {
    let (f, claim) = bound();
    let before = snapshot(&f);
    let binding = f.reopen().claimed_worktree_binding("test", &claim).unwrap();
    assert_eq!(
        binding.candidate,
        f.store.claimed_recovery_candidate("test", &claim).unwrap()
    );
    assert_eq!(binding.project_root, f.root);
    assert_eq!(binding.check_name, "Implementation");
    assert_eq!(binding.base_commit, "a".repeat(40));
    assert_eq!(
        binding.worktree_path,
        f.store
            .worktree_path(binding.candidate.attempt.record_id)
            .unwrap()
    );
    assert!(
        f.reopen()
            .claimed_worktree_binding("foreign", &claim)
            .is_err()
    );
    let mut wrong = claim.clone();
    wrong.operation_id.push('x');
    assert!(f.reopen().claimed_worktree_binding("test", &wrong).is_err());
    assert_eq!(snapshot(&f), before);
    f.store
        .close_agent_assignment(
            "assignment",
            forge_app::AgentAssignmentClosureReason::Retired,
        )
        .unwrap();
    let closed = snapshot(&f);
    assert!(f.reopen().claimed_worktree_binding("test", &claim).is_err());
    assert_eq!(snapshot(&f), closed);
}

#[test]
fn recovery_worktree_snapshot_rejects_missing_base_repository_and_fixed_path_drift() {
    for fault in 0..4 {
        let (f, claim) = bound();
        {
            let c = f.store.connection.lock().unwrap();
            match fault {
                0 => c.execute("DELETE FROM task_base_commits", []).unwrap(),
                1 => c
                    .execute(
                        "UPDATE record_attempts SET attempt_json = json_set(attempt_json, '$.evidence.repository', '/foreign/repository')",
                        [],
                    )
                    .unwrap(),
                2 => c
                    .execute("UPDATE worktree_claims SET path = '/foreign/worktree'", [])
                    .unwrap(),
                3 => c
                    .execute(
                        "UPDATE task_base_commits SET base_commit = 'not-a-commit'",
                        [],
                    )
                    .unwrap(),
                _ => unreachable!(),
            };
        }
        let before = snapshot(&f);
        assert!(f.reopen().claimed_worktree_binding("test", &claim).is_err());
        assert_eq!(snapshot(&f), before);
    }
}
