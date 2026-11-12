use clap::{Args, Subcommand};
use forge_cli::{AssignmentCheckAction, assignment_check};
use forge_rpc::{AssessmentInput, InvalidationInput, Verdict};

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List the assigned Task's completion criteria.
    List {
        #[arg(long)]
        compare_revision: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show an own-Task Check and bounded activity history.
    Show {
        check: u64,
        #[arg(long)]
        after: Option<u64>,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long)]
        compare_revision: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Append an authenticated pass/fail Assessment.
    Assess {
        #[command(flatten)]
        args: MutationArgs,
        #[arg(long, value_parser = ["pass", "fail"])]
        verdict: String,
    },
    /// Invalidate exactly the latest active Assessment.
    Invalidate {
        #[command(flatten)]
        args: MutationArgs,
        #[arg(long)]
        assessment: u64,
    },
}
#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("message_source").required(true).args(["message", "message_file"])))]
pub struct MutationArgs {
    check: u64,
    #[arg(long)]
    expected_version: u64,
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

pub async fn run(command: Command) -> i32 {
    let (action, json) = match action(command) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("forge assignment check: {error}");
            return 1;
        }
    };
    match assignment_check(action).await {
        Ok(value) => {
            if json {
                println!("{value}");
            } else {
                println!("{}", render(value));
            }
            0
        }
        Err(error) => {
            eprintln!("forge assignment check: {error}");
            1
        }
    }
}
fn action(command: Command) -> Result<(AssignmentCheckAction, bool), String> {
    Ok(match command {
        Command::List {
            compare_revision,
            json,
        } => (AssignmentCheckAction::List { compare_revision }, json),
        Command::Show {
            check,
            after,
            limit,
            compare_revision,
            json,
        } => (
            AssignmentCheckAction::Show {
                check,
                after,
                limit,
                compare_revision,
            },
            json,
        ),
        Command::Assess { args: a, verdict } => {
            let message = super::checks::explanation(a.message, a.message_file)?;
            (
                AssignmentCheckAction::Assess {
                    check: a.check,
                    input: AssessmentInput {
                        actor: String::new(),
                        expected_version: a.expected_version,
                        verdict: if verdict == "pass" {
                            Verdict::Pass
                        } else {
                            Verdict::Fail
                        },
                        message,
                        revision: a.revision,
                        dirty: a.dirty,
                    },
                },
                a.json,
            )
        }
        Command::Invalidate {
            args: a,
            assessment,
        } => {
            let message = super::checks::explanation(a.message, a.message_file)?;
            (
                AssignmentCheckAction::Invalidate {
                    check: a.check,
                    input: InvalidationInput {
                        actor: String::new(),
                        expected_version: a.expected_version,
                        assessment_id: assessment,
                        message,
                        revision: a.revision,
                        dirty: a.dirty,
                    },
                },
                a.json,
            )
        }
    })
}
fn render(value: serde_json::Value) -> String {
    if value.is_array() {
        let checks: Vec<forge_rpc::RecordCheck> =
            serde_json::from_value(value).expect("typed Check response");
        return checks
            .iter()
            .map(super::checks::render_check)
            .collect::<Vec<_>>()
            .join("\n");
    }
    if value.get("check").is_some() {
        let page: forge_rpc::CheckPage = serde_json::from_value(value).expect("typed Check page");
        return super::checks::render_page(&page);
    }
    let check: forge_rpc::RecordCheck =
        serde_json::from_value(value).expect("typed Check response");
    super::checks::render_check(&check)
}
