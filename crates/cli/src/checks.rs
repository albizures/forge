use clap::{Args, Subcommand};
use forge_rpc::{
    AssessmentInput, TaskCheck, TaskCheckAssessRequest, TaskCheckShowRequest, Verdict,
};
use std::{fmt::Write as _, io::Read};

#[derive(Debug, Subcommand)]
pub enum CheckCommand {
    /// List immutable Task completion criteria and current verdicts.
    List(ListArgs),
    /// Show one Check and a bounded ascending Assessment history page.
    Show(ShowArgs),
    /// Append a pass/fail Assessment using the observed Check version.
    Assess(AssessArgs),
    /// Invalidate the latest active Assessment, leaving the Check pending.
    Invalidate(InvalidateArgs),
}
#[derive(Debug, Args)]
pub struct ListArgs {
    task: u64,
    #[arg(long)]
    compare_revision: Option<String>,
    #[arg(long)]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
pub struct ShowArgs {
    task: u64,
    #[arg(long)]
    compare_revision: Option<String>,
    check: u64,
    #[arg(long)]
    after: Option<u64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long)]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("message_source").required(true).args(["message", "message_file"])))]
pub struct AssessArgs {
    task: u64,
    check: u64,
    #[arg(long, value_parser = ["pass", "fail"])]
    verdict: String,
    #[arg(long)]
    expected_version: u64,
    #[arg(long)]
    actor: String,
    #[arg(long)]
    message: Option<String>,
    #[arg(long)]
    message_file: Option<String>,
    #[arg(long)]
    revision: Option<String>,
    #[arg(long, action = clap::ArgAction::Set)]
    dirty: Option<bool>,
    #[arg(long)]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("message_source").required(true).args(["message", "message_file"])))]
pub struct InvalidateArgs {
    task: u64,
    check: u64,
    #[arg(long)]
    assessment: u64,
    #[arg(long)]
    expected_version: u64,
    #[arg(long)]
    actor: String,
    #[arg(long)]
    message: Option<String>,
    #[arg(long)]
    message_file: Option<String>,
    #[arg(long)]
    revision: Option<String>,
    #[arg(long, action = clap::ArgAction::Set)]
    dirty: Option<bool>,
    #[arg(long)]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}

pub(super) fn explanation(message: Option<String>, file: Option<String>) -> Result<String, String> {
    match (message, file) {
        (Some(message), None) => Ok(message),
        (None, Some(file)) if file == "-" => {
            let mut message = String::new();
            std::io::stdin()
                .read_to_string(&mut message)
                .map(|_| message)
                .map_err(|e| e.to_string())
        }
        (None, Some(file)) => std::fs::read_to_string(file).map_err(|e| e.to_string()),
        _ => Err("exactly one explanation source is required".into()),
    }
}

#[allow(clippy::too_many_lines)]
pub async fn run(command: CheckCommand, workspace: Option<String>) -> i32 {
    let (url, json) = match &command {
        CheckCommand::List(a) => (&a.server_url, a.json),
        CheckCommand::Show(a) => (&a.server_url, a.json),
        CheckCommand::Assess(a) => (&a.server_url, a.json),
        CheckCommand::Invalidate(a) => (&a.server_url, a.json),
    };
    let url = match super::resolve_managed_endpoint(url.as_deref()).await {
        Ok(url) => url,
        Err(error) => {
            eprintln!("forge task check: {error}");
            return 1;
        }
    };
    let endpoint = (url.as_str(), workspace.as_deref());
    let result = match command {
        CheckCommand::Invalidate(a) => match explanation(a.message, a.message_file) {
            Err(error) => Err(error),
            Ok(message) => forge_cli::request_task_check_invalidate(
                endpoint,
                forge_rpc::TaskCheckInvalidateRequest {
                    task: a.task,
                    check: a.check,
                    invalidation: forge_rpc::InvalidationInput {
                        expected_version: a.expected_version,
                        assessment_id: a.assessment,
                        actor: a.actor,
                        message,
                        revision: a.revision,
                        dirty: a.dirty,
                    },
                },
            )
            .await
            .map(|check| {
                let text = render_check(&check);
                (
                    serde_json::to_value(check).expect("serializable Check"),
                    text,
                )
            }),
        },
        CheckCommand::List(a) => {
            forge_cli::request_task_check_list_compared(endpoint, a.task, a.compare_revision)
                .await
                .map(|checks| {
                    let text = checks
                        .iter()
                        .map(render_check)
                        .collect::<Vec<_>>()
                        .join("\n");
                    (
                        serde_json::to_value(checks).expect("serializable Checks"),
                        text,
                    )
                })
        }
        CheckCommand::Show(a) => forge_cli::request_task_check_show(
            endpoint,
            TaskCheckShowRequest {
                compare_revision: a.compare_revision,
                task: a.task,
                check: a.check,
                after: a.after,
                limit: a.limit,
            },
        )
        .await
        .map(|page| {
            let text = render_page(&page);
            (
                serde_json::to_value(page).expect("serializable Check page"),
                text,
            )
        }),
        CheckCommand::Assess(a) => {
            let message = explanation(a.message, a.message_file);
            match message {
                Err(error) => Err(error),
                Ok(message) => forge_cli::request_task_check_assess(
                    endpoint,
                    TaskCheckAssessRequest {
                        task: a.task,
                        check: a.check,
                        assessment: AssessmentInput {
                            expected_version: a.expected_version,
                            verdict: if a.verdict == "pass" {
                                Verdict::Pass
                            } else {
                                Verdict::Fail
                            },
                            actor: a.actor,
                            message,
                            revision: a.revision,
                            dirty: a.dirty,
                        },
                    },
                )
                .await
                .map(|check| {
                    let text = render_check(&check);
                    (
                        serde_json::to_value(check).expect("serializable Check"),
                        text,
                    )
                }),
            }
        }
    };
    match result {
        Ok((value, text)) => {
            if json {
                println!("{value}");
            } else {
                println!("{text}");
            }
            0
        }
        Err(error) => {
            eprintln!("forge task check: {error}");
            1
        }
    }
}
pub(super) fn render_page(page: &forge_rpc::CheckPage) -> String {
    let mut text = render_check(&page.check);
    for activity in &page.activities {
        if let Some(i) = &activity.invalidation {
            let _ = write!(
                text,
                "\nActivity {}: Invalidation {} of Assessment {} by {} (Assignment {:?}) at {}\n{}\nrevision {:?}, dirty {:?}, comparison {:?}",
                activity.id,
                i.id,
                i.assessment_id,
                i.actor,
                i.assignment_id,
                i.created_at_millis,
                i.message,
                i.revision,
                i.dirty,
                i.comparison
            );
        } else if let Some(a) = page
            .assessments
            .iter()
            .find(|a| a.id == activity.assessment_id)
        {
            let _ = write!(
                text,
                "\nActivity {}: Assessment {} {:?} by {} (Assignment {:?}) at {}\n{}\nrevision {:?}, dirty {:?}, comparison {:?}",
                activity.id,
                a.id,
                a.verdict,
                a.actor,
                a.assignment_id,
                a.created_at_millis,
                a.message,
                a.revision,
                a.dirty,
                a.comparison
            );
        }
    }
    if let Some(cursor) = page.next_cursor {
        let _ = write!(text, "\nNext cursor: {cursor}");
    }
    text
}

pub(super) fn render_check(check: &TaskCheck) -> String {
    let verdict = match check.verdict() {
        None => "pending",
        Some(Verdict::Pass) => "pass",
        Some(Verdict::Fail) => "fail",
    };
    let mut text = format!(
        "Check {} {} [{}] version {}\n{}",
        check.id, check.name, verdict, check.version, check.description
    );
    if let Some(a) = &check.latest_assessment {
        let _ = write!(
            text,
            "\nLatest Assessment {} by {} (Assignment {:?}) at {}: {}\nrevision {:?}, dirty {:?}, comparison {:?}, invalidation {:?}",
            a.id,
            a.actor,
            a.assignment_id,
            a.created_at_millis,
            a.message,
            a.revision,
            a.dirty,
            a.comparison,
            a.invalidation
        );
    }
    text
}
