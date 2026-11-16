use super::Cli;
use clap::Parser;

#[test]
fn stack_commands_parse_without_execution_and_require_explicit_predecessors() {
    for arguments in [
        vec!["forge", "task", "stack", "show", "49", "--json"],
        vec![
            "forge",
            "task",
            "stack",
            "root",
            "41",
            "--repository",
            "acme/widgets",
            "--remote",
            "origin",
            "--base",
            "feature/payments",
        ],
        vec![
            "forge",
            "task",
            "stack",
            "after",
            "49",
            "--predecessor",
            "41",
        ],
        vec!["forge", "task", "stack", "prepare", "49"],
        vec![
            "forge",
            "task",
            "stack",
            "publish",
            "49",
            "--url",
            "https://github.com/acme/widgets/pull/123",
        ],
        vec!["forge", "assignment", "stack", "--json"],
        vec![
            "forge",
            "assignment",
            "stack",
            "--url",
            "https://github.com/acme/widgets/pull/123",
            "--json",
        ],
    ] {
        Cli::try_parse_from(arguments).unwrap();
    }
    assert!(Cli::try_parse_from(["forge", "task", "stack", "after", "49"]).is_err());
    assert!(Cli::try_parse_from(["forge", "assignment", "stack", "--predecessor", "41"]).is_err());
}
