use crate::{AgentDispatchCommand, Cli, Command};
use clap::Parser;

fn arguments() -> Vec<&'static str> {
    vec![
        "forge",
        "dispatcher",
        "assignment-resolve-effect",
        "assignment",
        "--operation-id",
        "resolution",
        "--execution-task",
        "tool",
        "--conversation",
        "child",
        "--call",
        "call",
        "--outcome",
        "not-applied",
        "--actor",
        "operator",
        "--evidence",
        "external-reference",
        "--message",
        "investigated",
        "--confirm-old-agent-cannot-act",
        "--confirm-external-work-safe",
        "--workspace",
        "owner",
        "--json",
    ]
}

#[test]
fn resolution_command_has_explicit_scoped_effect_identity_and_no_runtime_paths_or_agent_auth() {
    let parsed = Cli::try_parse_from(arguments()).unwrap();
    assert_eq!(parsed.workspace.as_deref(), Some("owner"));
    let Command::Dispatcher {
        command: AgentDispatchCommand::AssignmentResolveEffect(args),
    } = parsed.command
    else {
        panic!("wrong command");
    };
    assert_eq!(args.assignment_id, "assignment");
    assert_eq!(args.execution_task, "tool");
    assert_eq!(args.conversation, "child");
    assert_eq!(args.call, "call");
    assert!(args.confirm_external_work_safe && args.confirm_old_agent_cannot_act && args.json);
    for flag in [
        "--secret",
        "--capability-id",
        "--node",
        "--inspector",
        "--runtime-directory",
        "--launch",
    ] {
        let mut args = arguments();
        args.extend([flag, "untrusted"]);
        assert!(Cli::try_parse_from(args).is_err());
    }
}

#[test]
fn resolution_parser_does_not_default_missing_identity_evidence_or_unknown_outcome() {
    for flag in [
        "--operation-id",
        "--execution-task",
        "--conversation",
        "--call",
        "--actor",
        "--evidence",
        "--message",
        "--outcome",
    ] {
        let mut args = arguments();
        let position = args.iter().position(|arg| *arg == flag).unwrap();
        args.drain(position..position + 2);
        assert!(Cli::try_parse_from(args).is_err(), "{flag}");
    }
    let mut unknown = arguments();
    let position = unknown
        .iter()
        .position(|arg| *arg == "not-applied")
        .unwrap();
    unknown[position] = "unknown";
    assert!(Cli::try_parse_from(unknown).is_err());
}

#[test]
fn resolution_help_distinguishes_live_fences_from_closed_resource_retirement() {
    let help = Cli::try_parse_from(["forge", "dispatcher", "assignment-resolve-effect", "--help"])
        .unwrap_err();
    assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
    let text = help.to_string();
    assert!(text.contains("preserve live fences"));
    assert!(text.contains(
        "On a live Assignment, the accepted audit preserves its execution fence and attempt"
    ));
    assert!(text.contains("on a closed Assignment, only the targeted effect resource retires"));
    assert!(text.contains("Neither case authorizes recovery"));
    assert!(!text.contains("after revocation"));
}
