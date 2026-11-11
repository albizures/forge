use clap::{Args, Subcommand};
use forge_cli::{AssignmentFindingAction, assignment_finding};
use forge_rpc::{
    FindingMutation, FindingMutationInput, FindingRaiseInput, FindingResolveInput, FindingResponse,
    Severity,
};

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List a bounded page of the assigned Task's Findings.
    List(Page),
    /// Inspect an own-Task Finding and bounded activity history.
    Show {
        finding: u64,
        #[command(flatten)]
        page: Page,
    },
    /// Raise an issue with authenticated attribution.
    Raise {
        #[arg(long, value_parser = ["minor", "major", "critical"])]
        severity: String,
        #[arg(long, action = clap::ArgAction::Set)]
        blocking: Option<bool>,
        #[arg(long)]
        check: Option<u64>,
        #[arg(long)]
        assessment: Option<u64>,
        #[command(flatten)]
        explanation: Explanation,
    },
    /// Retain a response without closing or changing blocking.
    Respond {
        #[command(flatten)]
        args: MutationArgs,
        #[arg(long, value_parser = ["fixed", "rejected"])]
        response: String,
    },
    /// Verify and resolve an unresolved Finding.
    Resolve(MutationArgs),
    /// Dismiss an unresolved Finding with a reason.
    Dismiss(MutationArgs),
    /// Reopen a closed Finding with a reason.
    Reopen(MutationArgs),
    /// Explicitly amend severity and/or blocking before Done.
    Amend(AmendArgs),
    /// Append correction/evidence without changing disposition.
    Note(MutationArgs),
}
#[derive(Debug, Args)]
pub struct Page {
    #[arg(long)]
    after: Option<u64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long)]
    compare_revision: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("message_source").required(true).args(["message", "message_file"])))]
pub struct Explanation {
    #[arg(long)]
    message: Option<String>,
    #[arg(long)]
    message_file: Option<String>,
    #[arg(long)]
    revision: Option<String>,
    #[arg(long, action = clap::ArgAction::Set)]
    dirty: Option<bool>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
pub struct MutationArgs {
    finding: u64,
    #[arg(long)]
    expected_version: u64,
    #[command(flatten)]
    explanation: Explanation,
}
#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("amendment").required(true).multiple(true).args(["severity", "blocking"])))]
pub struct AmendArgs {
    #[command(flatten)]
    args: MutationArgs,
    #[arg(long, value_parser = ["minor", "major", "critical"])]
    severity: Option<String>,
    #[arg(long, action = clap::ArgAction::Set)]
    blocking: Option<bool>,
}
fn severity(value: &str) -> Severity {
    match value {
        "minor" => Severity::Minor,
        "major" => Severity::Major,
        _ => Severity::Critical,
    }
}
fn mutation(
    args: MutationArgs,
    change: FindingMutation,
) -> Result<(AssignmentFindingAction, bool), String> {
    let a = args.explanation;
    let message = super::checks::explanation(a.message, a.message_file)?;
    Ok((
        AssignmentFindingAction::Mutate {
            finding: args.finding,
            input: FindingMutationInput {
                change,
                explanation: FindingResolveInput {
                    expected_version: args.expected_version,
                    actor: String::new(),
                    message,
                    revision: a.revision,
                    dirty: a.dirty,
                },
            },
        },
        a.json,
    ))
}
fn action(command: Command) -> Result<(AssignmentFindingAction, bool), String> {
    match command {
        Command::List(p) => Ok((
            AssignmentFindingAction::List {
                after: p.after,
                limit: p.limit,
                compare_revision: p.compare_revision,
            },
            p.json,
        )),
        Command::Show { finding, page: p } => Ok((
            AssignmentFindingAction::Show {
                finding,
                after: p.after,
                limit: p.limit,
                compare_revision: p.compare_revision,
            },
            p.json,
        )),
        Command::Raise {
            severity: s,
            blocking,
            check,
            assessment,
            explanation: a,
        } => {
            let message = super::checks::explanation(a.message, a.message_file)?;
            Ok((
                AssignmentFindingAction::Raise(FindingRaiseInput {
                    severity: severity(&s),
                    blocking,
                    check_id: check,
                    assessment_id: assessment,
                    actor: String::new(),
                    message,
                    revision: a.revision,
                    dirty: a.dirty,
                }),
                a.json,
            ))
        }
        Command::Respond { args, response } => mutation(
            args,
            FindingMutation::Respond {
                response: if response == "fixed" {
                    FindingResponse::Fixed
                } else {
                    FindingResponse::Rejected
                },
            },
        ),
        Command::Resolve(args) => mutation(args, FindingMutation::Resolve),
        Command::Dismiss(args) => mutation(args, FindingMutation::Dismiss),
        Command::Reopen(args) => mutation(args, FindingMutation::Reopen),
        Command::Note(args) => mutation(args, FindingMutation::Note),
        Command::Amend(a) => mutation(
            a.args,
            FindingMutation::Amend {
                severity: a.severity.as_deref().map(severity),
                blocking: a.blocking,
            },
        ),
    }
}
pub async fn run(command: Command) -> i32 {
    let result = match action(command) {
        Ok((action, json)) => assignment_finding(action).await.map(|value| (value, json)),
        Err(error) => Err(error),
    };
    match result {
        Ok((value, json)) => {
            if json {
                println!("{value}");
            } else {
                println!("{}", render(value));
            }
            0
        }
        Err(error) => {
            eprintln!("forge assignment finding: {error}");
            1
        }
    }
}
fn render(value: serde_json::Value) -> String {
    if value.get("findings").is_some() {
        let page: forge_rpc::FindingList =
            serde_json::from_value(value).expect("typed Finding list");
        return super::findings::render_list(&page);
    }
    if value.get("finding").is_some() {
        let page: forge_rpc::FindingPage =
            serde_json::from_value(value).expect("typed Finding page");
        return super::findings::render_page(&page);
    }
    let finding: forge_rpc::Finding = serde_json::from_value(value).expect("typed Finding");
    super::findings::render(&finding)
}
