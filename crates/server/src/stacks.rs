//! GitHub and Git observations for the application-owned stack policy.
use forge_app::{
    RegistrationError,
    stacks::{PullRequestObservation, StackDestination, StackHosting},
};
use std::{
    path::Path,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

pub(super) struct GitHubStackHosting;
static OBSERVATION: AtomicU64 = AtomicU64::new(0);

fn error(message: &str) -> RegistrationError {
    RegistrationError::new("stack-hosting-unavailable", message)
}

fn output(root: &Path, program: &str, args: &[&str]) -> Result<String, RegistrationError> {
    let result = Command::new(program)
        .args(args)
        .current_dir(root)
        .env("GH_HOST", "github.com")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|_| error("could not execute stack hosting command"))?;
    if !result.status.success() {
        return Err(error(
            "stack hosting command failed; verify GitHub authentication, remote identity and branch/PR availability",
        ));
    }
    String::from_utf8(result.stdout)
        .map(|s| s.trim().to_owned())
        .map_err(|_| error("invalid stack hosting response"))
}

fn github_repository(url: &str) -> Option<&str> {
    let path = url
        .strip_prefix("git@github.com:")
        .or_else(|| url.strip_prefix("https://github.com/"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    Some(path.strip_suffix(".git").unwrap_or(path))
}

impl StackHosting for GitHubStackHosting {
    fn fetch_branch(
        &self,
        root: &Path,
        destination: &StackDestination,
        branch: &str,
    ) -> Result<String, RegistrationError> {
        destination.validate()?;
        if !forge_app::stacks::valid_branch(branch) {
            return Err(error("invalid stack branch"));
        }
        // Reject multiple push endpoints and fetch/push identity disagreements. Never follow
        // a configured push URL into a different repository or an unsupported hosting service.
        for args in [
            vec!["remote", "get-url", "--all", destination.remote.as_str()],
            vec![
                "remote",
                "get-url",
                "--push",
                "--all",
                destination.remote.as_str(),
            ],
        ] {
            let url = output(root, "git", &args)?;
            if github_repository(&url) != Some(destination.repository.as_str()) {
                return Err(error(
                    "stack remote must fetch and push the selected GitHub repository",
                ));
            }
        }
        // Each observation has its own ref: concurrent preparation never races on FETCH_HEAD
        // or updates a user's local/remote-tracking branch.
        let reference = format!(
            "refs/forge/stack-observations/{}-{}",
            std::process::id(),
            OBSERVATION.fetch_add(1, Ordering::Relaxed)
        );
        let refspec = format!("refs/heads/{branch}:{reference}");
        output(
            root,
            "git",
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                "--",
                &destination.remote,
                &refspec,
            ],
        )?;
        let commit = output(
            root,
            "git",
            &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        )?;
        output(root, "git", &["update-ref", "-d", &reference, &commit])?;
        Ok(commit)
    }

    fn pull_request(
        &self,
        root: &Path,
        repository: &str,
        url: &str,
    ) -> Result<PullRequestObservation, RegistrationError> {
        let prefix = format!("https://github.com/{repository}/pull/");
        let number = url
            .strip_prefix(&prefix)
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .ok_or_else(|| error("expected a PR URL in the selected GitHub repository"))?;
        let json = output(
            root,
            "gh",
            &[
                "pr",
                "view",
                number,
                "--repo",
                repository,
                "--json",
                "url,headRefName,headRefOid,baseRefName,state,isDraft,headRepository,headRepositoryOwner",
            ],
        )?;
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|_| error("invalid GitHub PR response"))?;
        let text = |key: &str| {
            value[key]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| error("incomplete GitHub PR response"))
        };
        let owner = value["headRepositoryOwner"]["login"]
            .as_str()
            .ok_or_else(|| error("missing PR head owner"))?;
        let name = value["headRepository"]["name"]
            .as_str()
            .ok_or_else(|| error("missing PR head repository"))?;
        let canonical_url = text("url")?;
        if canonical_url != url {
            return Err(error("GitHub returned a different PR identity"));
        }
        Ok(PullRequestObservation {
            repository: repository.into(),
            url: canonical_url,
            head_branch: text("headRefName")?,
            head_commit: text("headRefOid")?,
            base_branch: text("baseRefName")?,
            open: text("state")? == "OPEN",
            draft: value["isDraft"]
                .as_bool()
                .ok_or_else(|| error("missing PR draft status"))?,
            head_repository: format!("{owner}/{name}"),
        })
    }
}
