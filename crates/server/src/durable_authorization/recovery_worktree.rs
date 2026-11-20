//! Read-only Git adapter for same-attempt recovery progress verification.
use forge_app::{
    ManagedWorktreeAdapter, RegistrationError, RepositoryInspector,
    durable_execution::recovery::worktree::{Binding, Inspector, Observation},
    outcomes::Evidence,
};
use std::{fs, path::Path, process::Command};

#[cfg(test)]
#[path = "recovery_worktree_tests.rs"]
mod tests;

/// No profile files, credentials, network calls or repository repair.
pub struct GitInspector;

fn denied() -> RegistrationError {
    RegistrationError::new(
        "durable-recovery-worktree-unavailable",
        "cannot safely inspect original repository progress; preserve worktree and runtime evidence",
    )
}

fn git(path: &Path, args: &[&str]) -> Result<Vec<u8>, RegistrationError> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .args(["-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|_| denied())?;
    if !output.status.success() {
        return Err(denied());
    }
    Ok(output.stdout)
}

// Even status can run configured clean/process filters. This read-only
// verifier does not support those execution-bearing configurations. Exit one
// with no output means no matching drivers, not an unavailable Git invocation.
fn require_no_filters(path: &Path) -> Result<(), RegistrationError> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(path)
        .args([
            "config",
            "--null",
            "--get-regexp",
            r"^filter\..*\.(clean|process)$",
        ])
        .output()
        .map_err(|_| denied())?;
    if output.status.code() != Some(1) || !output.stdout.is_empty() {
        return Err(denied());
    }
    Ok(())
}

impl Inspector for GitInspector {
    fn observe(&self, binding: &Binding) -> Result<Observation, RegistrationError> {
        binding.validate()?;
        let path = &binding.worktree_path;
        forge_app::validate_managed_worktree_placement(path)?;
        if fs::canonicalize(path).map_err(|_| denied())? != *path
            || fs::canonicalize(&binding.project_root).map_err(|_| denied())?
                != binding.project_root
        {
            return Err(denied());
        }
        // Existing worktree identity policy, without invoking removal assessment
        // (upstream/removability are not execution freshness or liveness facts).
        crate::GitWorktreeAdapter.validate_recovery(
            &binding.project_root,
            path,
            &forge_app::stacks::task_branch(binding.candidate.attempt.record_id),
            &binding.base_commit,
        )?;
        let Evidence::Repository { commit, .. } = &binding.candidate.attempt.evidence else {
            return Err(denied());
        };
        git(path, &["merge-base", "--is-ancestor", commit, "HEAD"])?;
        let head = crate::GitInspector.head_commit(path)?;
        require_no_filters(path)?;
        // Reject links/submodule directories before status can descend into
        // nested repositories with their own execution-bearing configuration.
        let retained = crate::GitWorktreeAdapter.retained_state(path)?;
        let status = git(
            path,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )?;
        // Status may hide working bytes under assume-unchanged/skip-worktree.
        // Treat such index entries conservatively as dirty for read-only Checks;
        // Implementation still gets a content-bound progress observation.
        let entries = git(path, &["ls-files", "-v", "-z"])?;
        let hidden = entries
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .any(|entry| entry[0].is_ascii_lowercase() || entry[0] == b'S');
        let dirty = !status.is_empty() || hidden;
        Ok(Observation {
            head: Evidence::Repository {
                repository: binding.project_root.display().to_string(),
                commit: head,
            },
            dirty,
            retained,
        })
    }
}
