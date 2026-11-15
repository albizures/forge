use crate::stacks::GitHubStackHosting;
use forge_app::stacks::StackHosting;
use std::{path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn checkout_observation_distinguishes_commits_even_with_equal_trees_and_detects_dirt() {
    let root = std::env::temp_dir().join(format!(
        "forge-unchanged-checkout-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init"]);
    git(
        &root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--allow-empty",
            "--no-gpg-sign",
            "-m",
            "input",
        ],
    );
    let input = GitHubStackHosting.checkout(&root).unwrap();
    assert!(input.clean);
    let tree = git(&root, &["rev-parse", "HEAD^{tree}"]);
    git(
        &root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--allow-empty",
            "--no-gpg-sign",
            "-m",
            "different commit, identical tree",
        ],
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD^{tree}"]), tree);
    let output = GitHubStackHosting.checkout(&root).unwrap();
    assert!(output.clean);
    assert_ne!(input.commit, output.commit);
    std::fs::write(root.join("untracked"), "dirty").unwrap();
    assert!(!GitHubStackHosting.checkout(&root).unwrap().clean);
    // Observation never repairs or removes repository contents.
    assert_eq!(
        std::fs::read_to_string(root.join("untracked")).unwrap(),
        "dirty"
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), output.commit);
    std::fs::remove_dir_all(root).unwrap();
}
