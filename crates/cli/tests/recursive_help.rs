use std::process::{Command, Output};

fn forge(args: &[&str]) -> Output {
    let directory = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(args)
        .current_dir(directory.path())
        .env("HOME", directory.path())
        .env("XDG_CONFIG_HOME", directory.path().join("config"))
        .env("XDG_DATA_HOME", directory.path().join("data"))
        .env("FORGE_SERVER_URL", "http://127.0.0.1:1")
        .env_remove("FORGE_WORKSPACE")
        .output()
        .unwrap()
}

fn help(args: &[&str]) -> String {
    let output = forge(args);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn full_recursive_help_contains_leaf_details_and_inherited_flags_offline() {
    let output = help(&["help", "--all"]);
    assert!(output.starts_with("=== forge ==="));
    assert!(output.contains("=== forge record list ==="));
    assert!(output.contains("=== forge workspace"));
    assert!(!output.contains("=== forge help"));
    let leaf = help(&["help", "record", "list", "--all"]);
    assert!(leaf.contains("Usage: forge record list"));
    assert!(leaf.contains("--workspace"));
    assert!(leaf.contains("--status"));
    assert!(output.contains(&leaf));
    assert_eq!(output, help(&["help", "--all"]));
}

#[test]
fn subtree_nested_and_leaf_paths_are_bounded() {
    let output = help(&["help", "workspace", "--all"]);
    assert!(output.starts_with("=== forge workspace ==="));
    assert!(output.contains("=== forge workspace list ==="));
    assert!(!output.contains("=== forge record"));
    let nested = help(&["help", "dispatcher", "config", "--all"]);
    assert!(nested.starts_with("=== forge dispatcher config ==="));
    assert!(nested.contains("=== forge dispatcher config get ==="));
    assert!(!nested.contains("=== forge dispatcher status ==="));
    let leaf = help(&["help", "record", "list", "--all"]);
    assert_eq!(leaf.matches("=== forge").count(), 1);
}

#[test]
fn ordinary_help_remains_equivalent() {
    for path in [
        vec![],
        vec!["workspace"],
        vec!["dispatcher", "config", "get"],
    ] {
        let mut ordinary = path.clone();
        ordinary.push("--help");
        let mut command = vec!["help"];
        command.extend(path);
        assert_eq!(help(&ordinary), help(&command));
    }
    help(&["workspace", "help", "list"]);
}

#[test]
fn invalid_requests_fail_without_partial_output() {
    for args in [
        vec!["help", "missing-command", "--all"],
        vec!["help", "workspace", "missing-command", "--all"],
        vec!["help", "record", "list", "--all", "--status", "open"],
        vec!["help", "record", "list", "extra", "--all"],
    ] {
        let output = forge(&args);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("error:"));
    }
}

#[test]
fn workspace_selectors_are_parsed_but_never_resolved() {
    let expected = help(&["help", "workspace", "--all"]);
    for args in [
        vec!["--workspace", "nonexistent", "help", "workspace", "--all"],
        vec!["help", "workspace", "--all", "--workspace", "nonexistent"],
    ] {
        assert_eq!(help(&args), expected);
    }
}
