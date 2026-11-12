use std::process::{Command, Output};

fn forge(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(args)
        .env_remove("FORGE_SERVER_URL")
        .output()
        .unwrap()
}

#[test]
fn removed_classification_flags_are_rejected_before_contacting_a_daemon() {
    for command in [
        vec!["task", "open", "--body", "Research an API"],
        vec![
            "task",
            "under-spec",
            "--spec",
            "1",
            "--body",
            "Review a change",
        ],
        vec![
            "task",
            "under-wayfinder",
            "--wayfinder",
            "1",
            "--body",
            "Build a prototype",
        ],
        vec!["record", "list"],
    ] {
        for flag in ["--kind", "--task-kind"] {
            let mut args = command.clone();
            args.extend([flag, "research"]);
            let output = forge(&args);
            assert_eq!(output.status.code(), Some(2), "{args:?}");
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(error.contains("unexpected argument"), "{error}");
            assert!(error.contains(flag), "{error}");
        }
    }
}

#[test]
fn check_and_finding_commands_are_record_oriented_without_old_aliases() {
    for surface in ["check", "finding"] {
        let output = forge(&["record", surface, "list", "--help"]);
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(
            help.contains(&format!("forge record {surface} list")),
            "{help}"
        );
        assert!(help.contains("<RECORD>"), "{help}");
        assert!(!help.contains("<TASK>"), "{help}");

        let output = forge(&["task", surface, "list", "1"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("unrecognized subcommand")
        );
    }
}
