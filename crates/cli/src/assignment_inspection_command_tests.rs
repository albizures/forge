use crate::{AgentDispatchCommand, Cli, Command};
use clap::{CommandFactory, Parser};

#[test]
fn status_and_transcript_commands_are_scoped_read_only_and_reject_runtime_inputs() {
    let parsed = Cli::try_parse_from([
        "forge",
        "dispatcher",
        "assignment-transcript",
        "assignment",
        "--after",
        "42",
        "--limit",
        "100",
        "--workspace",
        "owner",
        "--json",
    ])
    .unwrap();
    assert_eq!(parsed.workspace.as_deref(), Some("owner"));
    let Command::Dispatcher {
        command: AgentDispatchCommand::AssignmentTranscript(args),
    } = parsed.command
    else {
        panic!("wrong command");
    };
    assert_eq!(
        (args.after.as_deref(), args.limit, args.json),
        (Some("42"), Some(100), true)
    );
    assert!(matches!(
        Cli::try_parse_from(["forge", "dispatcher", "assignment-status", "a"])
            .unwrap()
            .command,
        Command::Dispatcher {
            command: AgentDispatchCommand::AssignmentStatus(_)
        }
    ));
    for command in ["assignment-status", "assignment-transcript"] {
        for flag in [
            "--node",
            "--inspector",
            "--runtime-directory",
            "--launch",
            "--secret",
            "--repair",
            "--resume",
        ] {
            assert!(Cli::try_parse_from(["forge", "dispatcher", command, "a", flag, "x"]).is_err());
        }
    }
    for limit in ["0", "101", "-1", "x"] {
        assert!(
            Cli::try_parse_from([
                "forge",
                "dispatcher",
                "assignment-transcript",
                "a",
                "--limit",
                limit
            ])
            .is_err()
        );
    }
}

#[test]
fn help_has_examples_continuation_and_non_execution_guidance() {
    let mut command = Cli::command();
    let dispatcher = command.find_subcommand_mut("dispatcher").unwrap();
    for (name, expected) in [
        (
            "assignment-status",
            ["Examples:", "starts no runner", "not prove a process"].as_slice(),
        ),
        (
            "assignment-transcript",
            [
                "Examples:",
                "next_after",
                "not end of history",
                "never repaired",
            ]
            .as_slice(),
        ),
    ] {
        let help = dispatcher
            .find_subcommand_mut(name)
            .unwrap()
            .render_long_help()
            .to_string();
        for text in expected {
            assert!(help.contains(text), "{name} help missing {text}: {help}");
        }
    }
}

#[test]
fn rendering_is_truthful_for_unavailable_unstarted_empty_and_paged_history() {
    use forge_rpc::{AssignmentTranscriptResponse, TranscriptEntry, TranscriptHistory};
    let page = |history, reason: Option<&str>, entries: Vec<TranscriptEntry>, more| {
        AssignmentTranscriptResponse {
            assignment_id: "a1".into(),
            history,
            reason: reason.map(Into::into),
            next_after: entries.last().map(|e| e.id.clone()).or(Some("7".into())),
            entries,
            more,
        }
    };
    let unavailable = forge_cli::render_transcript(&page(
        TranscriptHistory::Unavailable,
        Some("runner-history-unavailable"),
        vec![],
        false,
    ));
    assert!(
        unavailable.contains("History unavailable (runner-history-unavailable)")
            && unavailable.contains("not treat this as an empty")
    );
    assert!(
        forge_cli::render_transcript(&page(TranscriptHistory::NotStarted, None, vec![], false))
            .contains("not started")
    );
    let empty =
        forge_cli::render_transcript(&page(TranscriptHistory::Available, None, vec![], false));
    assert!(
        empty.contains("No committed entries after this cursor")
            && empty.contains("--after 7")
            && empty.contains("not end of history")
    );
    let entry = TranscriptEntry {
        id: "9".into(),
        kind: "pi.message".into(),
        by_task_id: Some("3".into()),
        content: "{}".into(),
        truncated: true,
    };
    let paged =
        forge_cli::render_transcript(&page(TranscriptHistory::Available, None, vec![entry], true));
    assert!(
        paged.contains("#9 pi.message (task 3) [content truncated]")
            && paged.contains("More committed entries: yes")
            && paged.contains("--after 9")
    );
}

#[test]
fn followup_requires_one_message_source_and_rejects_rebinding_flags() {
    let base = [
        "forge",
        "dispatcher",
        "assignment-followup",
        "a",
        "--operation-id",
        "op",
    ];
    let parse = |extra: &[&str]| Cli::try_parse_from(base.iter().chain(extra)).is_ok();
    assert!(parse(&["--message", "m"]));
    assert!(parse(&[
        "--message-file",
        "note.txt",
        "--workspace",
        "w",
        "--json"
    ]));
    assert!(!parse(&[]), "a message is required");
    assert!(!parse(&["--message", "m", "--message-file", "f"]));
    for flag in [
        "--profile",
        "--check",
        "--commit",
        "--evidence",
        "--generation",
        "--attempt",
    ] {
        assert!(!parse(&["--message", "m", flag, "x"]), "{flag}");
    }
}
