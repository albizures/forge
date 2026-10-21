use std::{env, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=FORGE_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=FORGE_BUILD_DIRTY");
    println!("cargo:rerun-if-env-changed=FORGE_BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=FORGE_MANAGEMENT_PROTOCOL_REVISION");
    println!("cargo:rerun-if-changed=.git/HEAD");

    let commit = env_or_git("FORGE_BUILD_COMMIT", &["rev-parse", "HEAD"]);
    let dirty = env::var("FORGE_BUILD_DIRTY").unwrap_or_else(|_| git_dirty());
    let version = env::var("FORGE_BUILD_VERSION").unwrap_or_else(|_| {
        env::var("CARGO_PKG_VERSION").expect("Cargo supplies CARGO_PKG_VERSION")
    });
    let protocol_revision =
        env::var("FORGE_MANAGEMENT_PROTOCOL_REVISION").unwrap_or_else(|_| "1".to_owned());

    assert!(
        !commit.is_empty(),
        "FORGE_BUILD_COMMIT must be set when Git commit discovery is unavailable"
    );
    assert!(
        matches!(dirty.as_str(), "true" | "false"),
        "FORGE_BUILD_DIRTY must be `true` or `false`"
    );
    assert!(
        protocol_revision.parse::<u32>().is_ok(),
        "FORGE_MANAGEMENT_PROTOCOL_REVISION must be an unsigned integer"
    );

    println!("cargo:rustc-env=FORGE_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=FORGE_BUILD_DIRTY={dirty}");
    println!("cargo:rustc-env=FORGE_BUILD_VERSION={version}");
    println!("cargo:rustc-env=FORGE_MANAGEMENT_PROTOCOL_REVISION={protocol_revision}");
}

fn env_or_git(name: &str, arguments: &[&str]) -> String {
    env::var(name).unwrap_or_else(|_| git(arguments))
}

fn git(arguments: &[&str]) -> String {
    Command::new("git")
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| output.trim().to_owned())
        .unwrap_or_default()
}

fn git_dirty() -> String {
    let status = git(&["status", "--porcelain"]);
    (!status.is_empty()).to_string()
}
