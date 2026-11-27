//! Local checkout observations for planning Record creation.
//!
//! These observations belong to the initiating client, never to the daemon or
//! the directory in which an Assignment is subsequently dispatched.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use forge_app::planning_context::{PlanningContext, ProjectPlanningContext};

fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(cwd)
        // Do not let a caller's Git overrides redirect inspection to another checkout.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .args(args)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim_end_matches(['\r', '\n']).to_owned())
        .map_err(|_| "git returned non-UTF-8 output".to_owned())
}

fn common_dir(path: &Path) -> Result<PathBuf, String> {
    let value = git(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    PathBuf::from(value)
        .canonicalize()
        .map_err(|error| format!("cannot resolve repository identity: {error}"))
}

/// Observe the branch of `cwd` only when it is in the same Repository as the
/// Project Root. Linked Worktrees share a common Git directory with that root.
/// Failures are data so planning Record creation need not fail.
#[must_use]
pub fn observe(project: &str, project_root: &Path, cwd: &Path) -> ProjectPlanningContext {
    let context = match capture(project_root, cwd) {
        Ok((repository, branch)) => PlanningContext::Available { repository, branch },
        Err(reason) => PlanningContext::Unavailable { reason },
    };
    ProjectPlanningContext {
        project: project.to_owned(),
        context,
    }
}

fn capture(project_root: &Path, cwd: &Path) -> Result<(String, String), String> {
    if git(cwd, &["rev-parse", "--is-inside-work-tree"])? != "true" {
        return Err("initiating directory is not in a checkout".into());
    }
    let initiating_repository = common_dir(cwd)?;
    let project_repository = common_dir(project_root)
        .map_err(|error| format!("cannot inspect Project Root: {error}"))?;
    if initiating_repository != project_repository {
        return Err("initiating checkout does not belong to this Project".into());
    }
    // A Project may occupy only a subdirectory of a repository. Sharing Git's
    // common directory alone would incorrectly accept its Workspace root or a
    // sibling Project. Map the Project's relative path into the initiating
    // worktree instead of substituting the configured checkout.
    let checkout_root = |path: &Path| -> Result<PathBuf, String> {
        PathBuf::from(git(path, &["rev-parse", "--show-toplevel"])?)
            .canonicalize()
            .map_err(|error| format!("cannot resolve checkout root: {error}"))
    };
    let project_root = project_root
        .canonicalize()
        .map_err(|error| format!("cannot resolve Project Root: {error}"))?;
    let relative = project_root
        .strip_prefix(checkout_root(&project_root)?)
        .map_err(|_| "Project Root is outside its checkout".to_owned())?;
    let initiating_root = checkout_root(cwd)?;
    let initiating_directory = cwd
        .canonicalize()
        .map_err(|error| format!("cannot resolve initiating directory: {error}"))?;
    if !initiating_directory.starts_with(initiating_root.join(relative)) {
        return Err("initiating directory is outside this Project's checkout".into());
    }
    // Do not use --short: Git can return `heads/name` when another ref has
    // the same short name, which is not the name of the planning branch.
    let head = git(cwd, &["symbolic-ref", "--quiet", "HEAD"])
        .map_err(|error| format!("branch unavailable (possibly detached HEAD): {error}"))?;
    let branch = head
        .strip_prefix("refs/heads/")
        .filter(|branch| !branch.is_empty())
        .ok_or("branch unavailable: HEAD does not name a local branch")?
        .to_owned();
    // Stack destinations currently use GitHub owner/repository names. Never persist
    // a URL (which may contain credentials), a local path, or an arbitrary choice
    // among remotes. Only a single supported origin is unambiguous.
    let urls = git(cwd, &["remote", "get-url", "--all", "origin"])
        .map_err(|_| "repository unavailable: cannot inspect origin".to_owned())?;
    let path = urls
        .strip_prefix("git@github.com:")
        .or_else(|| urls.strip_prefix("https://github.com/"))
        .or_else(|| urls.strip_prefix("ssh://git@github.com/"))
        .ok_or("repository unavailable: origin is not a supported GitHub URL")?;
    let repository = path.strip_suffix(".git").unwrap_or(path);
    let mut components = repository.split('/');
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if !matches!((components.next(), components.next(), components.next()),
        (Some(owner), Some(name), None) if valid(owner) && valid(name))
    {
        return Err("repository unavailable: invalid or ambiguous origin".into());
    }
    Ok((repository.to_owned(), branch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn run(cwd: &Path, args: &[&str]) {
        let result = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    fn branch(observation: ProjectPlanningContext) -> Option<String> {
        match observation.context {
            PlanningContext::Available { branch, .. } => Some(branch),
            PlanningContext::Unavailable { reason } => {
                assert!(!reason.is_empty());
                None
            }
        }
    }
    #[test]
    fn branch_name_is_not_changed_by_an_ambiguous_tag() {
        let temp = tempdir().unwrap();
        run(temp.path(), &["init", "-b", "feature"]);
        run(
            temp.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        run(temp.path(), &["tag", "feature"]);
        run(
            temp.path(),
            &["remote", "add", "origin", "git@github.com:example/repo.git"],
        );
        assert_eq!(
            branch(observe("w/p", temp.path(), temp.path())),
            Some("feature".into())
        );
    }

    #[test]
    fn repository_identity_is_remote_not_checkout_path() {
        let temp = tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        for path in [&first, &second] {
            std::fs::create_dir(path).unwrap();
            run(path, &["init", "-b", "main"]);
        }
        run(
            &first,
            &["remote", "add", "origin", "git@github.com:example/repo.git"],
        );
        run(
            &second,
            &["remote", "add", "origin", "https://github.com/example/repo"],
        );
        let context = |path: &Path| observe("w/p", path, path).context;
        assert_eq!(context(&first), context(&second));
        run(
            &second,
            &[
                "remote",
                "set-url",
                "origin",
                "https://github.com/example/other.git",
            ],
        );
        assert_ne!(context(&first), context(&second));
        run(
            &second,
            &[
                "remote",
                "set-url",
                "origin",
                "https://secret@github.com/example/repo.git",
            ],
        );
        match context(&second) {
            PlanningContext::Unavailable { reason } => assert!(!reason.contains("secret")),
            PlanningContext::Available { .. } => {
                panic!("credential-bearing URL must not be persisted")
            }
        }
        run(
            &second,
            &[
                "remote",
                "set-url",
                "origin",
                "https://github.com/example/repo",
            ],
        );
        run(
            &second,
            &[
                "remote",
                "set-url",
                "--add",
                "origin",
                "https://github.com/example/other",
            ],
        );
        assert!(matches!(
            context(&second),
            PlanningContext::Unavailable { .. }
        ));
        run(&second, &["remote", "remove", "origin"]);
        assert!(matches!(
            context(&second),
            PlanningContext::Unavailable { .. }
        ));
    }

    #[test]
    fn capture_failures_retain_inspectable_reasons() {
        let temp = tempdir().unwrap();
        let main = temp.path().join("main");
        std::fs::create_dir(&main).unwrap();
        run(&main, &["init", "-b", "main"]);
        run(
            &main,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        run(&main, &["checkout", "--detach"]);
        let reason = |observation: ProjectPlanningContext| match observation.context {
            PlanningContext::Unavailable { reason } => reason,
            PlanningContext::Available { .. } => panic!("expected unavailable context"),
        };
        assert!(reason(observe("w/p", &main, &main)).contains("branch unavailable"));
        assert!(reason(observe("w/p", &main, temp.path())).contains("git"));
        assert!(
            reason(observe("w/p", &temp.path().join("missing"), &main)).contains("Project Root")
        );
    }

    #[test]
    fn only_initiating_matching_worktree_supplies_branch() {
        let temp = tempdir().unwrap();
        let main = temp.path().join("main");
        let linked = temp.path().join("linked");
        let other = temp.path().join("other");
        std::fs::create_dir(&main).unwrap();
        std::fs::create_dir(&other).unwrap();
        run(&main, &["init", "-b", "main"]);
        run(
            &main,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        run(
            &main,
            &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
        );
        run(&other, &["init"]);
        run(
            &main,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/example/repo.git",
            ],
        );
        assert_eq!(branch(observe("w/p", &main, &main)), Some("main".into()));
        assert_eq!(
            branch(observe("w/p", &main, &linked)),
            Some("feature".into())
        );
        let subproject = main.join("project");
        std::fs::create_dir(&subproject).unwrap();
        std::fs::create_dir(linked.join("project")).unwrap();
        assert_eq!(branch(observe("w/p", &subproject, &main)), None);
        assert_eq!(
            branch(observe("w/p", &subproject, &subproject)),
            Some("main".into())
        );
        assert_eq!(branch(observe("w/p", &subproject, &linked)), None);
        assert_eq!(
            branch(observe("w/p", &subproject, &linked.join("project"))),
            Some("feature".into())
        );
        assert_eq!(branch(observe("w/p", &main, &other)), None);
        assert_eq!(branch(observe("w/p", &main, temp.path())), None);
        assert_eq!(
            branch(observe("w/p", &temp.path().join("missing"), &main)),
            None
        );
        run(&linked, &["checkout", "--detach"]);
        assert_eq!(branch(observe("w/p", &main, &linked)), None);
    }
}
