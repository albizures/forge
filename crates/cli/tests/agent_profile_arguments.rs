use std::process::Command;

#[test]
fn profile_commands_require_explicit_authority_free_operator_selectors() {
    for (arguments, expected) in [
        (
            vec!["agent-profile", "preview", "forge-agent.md"],
            "requires explicit --workspace",
        ),
        (
            vec![
                "agent-profile",
                "register",
                "forge-agent.md",
                "--workspace",
                "one",
            ],
            "--record-type",
        ),
        (
            vec![
                "agent-profile",
                "register",
                "forge-agent.md",
                "--workspace",
                "one",
                "--record-type",
                "spec",
            ],
            "invalid value",
        ),
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(arguments)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains(expected));
    }
}
