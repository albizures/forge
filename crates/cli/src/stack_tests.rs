use super::Cli;
use clap::Parser;

#[test]
fn retained_stack_commands_parse_without_execution() {
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
    let legacy = Cli::try_parse_from([
        "forge",
        "task",
        "stack",
        "after",
        "49",
        "--predecessor",
        "41",
    ])
    .unwrap_err()
    .to_string();
    assert!(legacy.contains("record relate <task> --kind builds-on --to <predecessor>"));
    assert!(Cli::try_parse_from(["forge", "assignment", "stack", "--predecessor", "41"]).is_err());
}
