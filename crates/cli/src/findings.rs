use clap::{Args, Subcommand};
use forge_rpc::{
    Finding, FindingMutation, FindingMutationInput, FindingRaiseInput, FindingResolveInput,
    FindingResponse, RecordFindingListRequest, RecordFindingMutationRequest,
    RecordFindingRaiseRequest, RecordFindingResolveRequest, RecordFindingShowRequest, Severity,
};
use std::fmt::Write as _;

#[derive(Debug, Subcommand)]
pub enum FindingCommand {
    /// List a bounded, ID-ordered page of Findings.
    List(ListArgs),
    /// Inspect a Finding and a bounded activity history page.
    Show(ShowArgs),
    /// Raise a Finding; minor defaults nonblocking, major/critical blocking.
    Raise(RaiseArgs),
    /// Verify and resolve an unresolved Finding at its observed version.
    Resolve(ResolveArgs),
    /// Retain a fixed/rejected response without closing or changing blocking.
    Respond(RespondArgs),
    /// Dismiss an unresolved Finding with a reason.
    Dismiss(ResolveArgs),
    /// Reopen a closed Finding with a reason.
    Reopen(ResolveArgs),
    /// Explicitly amend severity and/or blocking before Done.
    Amend(AmendArgs),
    /// Append a correction or evidence without changing disposition.
    Note(ResolveArgs),
    /// Assign a review Check to verify an unlinked Finding (once only).
    AssignVerification(AssignVerificationArgs),
}
#[derive(Debug, Args)]
pub struct AssignVerificationArgs {
    #[command(flatten)]
    common: ResolveArgs,
    #[arg(long)]
    check: u64,
}
#[derive(Debug, Args)]
pub struct Output {
    #[arg(long)]
    server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
pub struct Page {
    #[arg(long)]
    after: Option<u64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long)]
    compare_revision: Option<String>,
}
#[derive(Debug, Args)]
pub struct ListArgs {
    record: u64,
    #[command(flatten)]
    page: Page,
    #[command(flatten)]
    output: Output,
}
#[derive(Debug, Args)]
pub struct ShowArgs {
    record: u64,
    finding: u64,
    #[command(flatten)]
    page: Page,
    #[command(flatten)]
    output: Output,
}
#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("message_source").required(true).args(["message", "message_file"])))]
pub struct Explanation {
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
}
#[derive(Debug, Args)]
pub struct RaiseArgs {
    record: u64,
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
    #[command(flatten)]
    output: Output,
}
#[derive(Debug, Args)]
pub struct ResolveArgs {
    record: u64,
    finding: u64,
    #[arg(long)]
    expected_version: u64,
    #[command(flatten)]
    explanation: Explanation,
    #[command(flatten)]
    output: Output,
}
#[derive(Debug, Args)]
pub struct RespondArgs {
    #[command(flatten)]
    common: ResolveArgs,
    #[arg(long, value_parser = ["fixed", "rejected"])]
    response: String,
}
#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("amendment").required(true).multiple(true).args(["severity", "blocking"])))]
pub struct AmendArgs {
    #[command(flatten)]
    common: ResolveArgs,
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
async fn mutate(
    endpoint: (&str, Option<&str>),
    args: ResolveArgs,
    change: FindingMutation,
) -> Result<(serde_json::Value, String), String> {
    let message =
        super::checks::explanation(args.explanation.message, args.explanation.message_file)?;
    forge_cli::request_record_finding_mutate(
        endpoint,
        RecordFindingMutationRequest {
            record: args.record,
            finding: args.finding,
            mutation: FindingMutationInput {
                change,
                explanation: FindingResolveInput {
                    expected_version: args.expected_version,
                    actor: args.explanation.actor,
                    message,
                    revision: args.explanation.revision,
                    dirty: args.explanation.dirty,
                },
            },
        },
    )
    .await
    .map(value)
}
pub(super) fn render(f: &Finding) -> String {
    format!(
        "Finding {} [{:?}] {:?}, blocking {}, version {}\nCheck {:?}, Assessment {:?}, Verification Check {:?}\nRaised by {} at {} (Assignment {:?}): {}\nrevision {:?}, dirty {:?}, comparison {:?}\nLatest Activity {} {:?} by {} at {} (Assignment {:?}): {}\nrevision {:?}, dirty {:?}, comparison {:?}\nresponse {:?}, amendment {:?}",
        f.id,
        f.state,
        f.severity,
        f.blocking,
        f.version,
        f.check_id,
        f.assessment_id,
        f.verification_check_id,
        f.raised.actor,
        f.raised.created_at_millis,
        f.raised.assignment_id,
        f.raised.message,
        f.raised.revision,
        f.raised.dirty,
        f.raised.comparison,
        f.latest_activity.id,
        f.latest_activity.action,
        f.latest_activity.actor,
        f.latest_activity.created_at_millis,
        f.latest_activity.assignment_id,
        f.latest_activity.message,
        f.latest_activity.revision,
        f.latest_activity.dirty,
        f.latest_activity.comparison,
        f.latest_activity.response,
        f.latest_activity.amendment
    )
}
pub(super) fn render_list(page: &forge_rpc::FindingList) -> String {
    let mut text = page
        .findings
        .iter()
        .map(render)
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(cursor) = page.next_cursor {
        let _ = write!(text, "\nNext cursor: {cursor}");
    }
    text
}
pub(super) fn render_page(page: &forge_rpc::FindingPage) -> String {
    let mut text = render(&page.finding);
    for activity in &page.activities {
        let _ = write!(
            text,
            "\nActivity {} {:?} by {} at {} (Assignment {:?}): {}\nrevision {:?}, dirty {:?}, comparison {:?}\nresponse {:?}, amendment {:?}",
            activity.id,
            activity.action,
            activity.actor,
            activity.created_at_millis,
            activity.assignment_id,
            activity.message,
            activity.revision,
            activity.dirty,
            activity.comparison,
            activity.response,
            activity.amendment
        );
    }
    if let Some(cursor) = page.next_cursor {
        let _ = write!(text, "\nNext cursor: {cursor}");
    }
    text
}
fn value(f: Finding) -> (serde_json::Value, String) {
    let text = render(&f);
    (serde_json::to_value(f).expect("serializable Finding"), text)
}
#[allow(clippy::too_many_lines)]
pub async fn run(command: FindingCommand, workspace: Option<String>) -> i32 {
    let output = match &command {
        FindingCommand::List(a) => &a.output,
        FindingCommand::Show(a) => &a.output,
        FindingCommand::Raise(a) => &a.output,
        FindingCommand::Resolve(a)
        | FindingCommand::Dismiss(a)
        | FindingCommand::Reopen(a)
        | FindingCommand::Note(a) => &a.output,
        FindingCommand::Respond(a) => &a.common.output,
        FindingCommand::Amend(a) => &a.common.output,
        FindingCommand::AssignVerification(a) => &a.common.output,
    };
    let json = output.json;
    let url = match super::resolve_managed_endpoint(output.server_url.as_deref()).await {
        Ok(url) => url,
        Err(error) => {
            eprintln!("forge record finding: {error}");
            return 1;
        }
    };
    let endpoint = (url.as_str(), workspace.as_deref());
    let result = match command {
        FindingCommand::List(a) => forge_cli::request_record_finding_list(
            endpoint,
            RecordFindingListRequest {
                record: a.record,
                after: a.page.after,
                limit: a.page.limit,
                compare_revision: a.page.compare_revision,
            },
        )
        .await
        .map(|page| {
            let text = render_list(&page);
            (
                serde_json::to_value(page).expect("serializable Findings"),
                text,
            )
        }),
        FindingCommand::Show(a) => forge_cli::request_record_finding_show(
            endpoint,
            RecordFindingShowRequest {
                record: a.record,
                finding: a.finding,
                after: a.page.after,
                limit: a.page.limit,
                compare_revision: a.page.compare_revision,
            },
        )
        .await
        .map(|page| {
            let text = render_page(&page);
            (
                serde_json::to_value(page).expect("serializable Finding page"),
                text,
            )
        }),
        FindingCommand::Raise(a) => {
            match super::checks::explanation(a.explanation.message, a.explanation.message_file) {
                Err(error) => Err(error),
                Ok(message) => forge_cli::request_record_finding_raise(
                    endpoint,
                    RecordFindingRaiseRequest {
                        record: a.record,
                        finding: FindingRaiseInput {
                            severity: match a.severity.as_str() {
                                "minor" => Severity::Minor,
                                "major" => Severity::Major,
                                _ => Severity::Critical,
                            },
                            blocking: a.blocking,
                            check_id: a.check,
                            assessment_id: a.assessment,
                            actor: a.explanation.actor,
                            message,
                            revision: a.explanation.revision,
                            dirty: a.explanation.dirty,
                        },
                    },
                )
                .await
                .map(value),
            }
        }
        FindingCommand::Respond(a) => {
            mutate(
                endpoint,
                a.common,
                FindingMutation::Respond {
                    response: if a.response == "fixed" {
                        FindingResponse::Fixed
                    } else {
                        FindingResponse::Rejected
                    },
                },
            )
            .await
        }
        FindingCommand::AssignVerification(a) => {
            mutate(
                endpoint,
                a.common,
                FindingMutation::AssignVerification { check_id: a.check },
            )
            .await
        }
        FindingCommand::Dismiss(a) => mutate(endpoint, a, FindingMutation::Dismiss).await,
        FindingCommand::Reopen(a) => mutate(endpoint, a, FindingMutation::Reopen).await,
        FindingCommand::Note(a) => mutate(endpoint, a, FindingMutation::Note).await,
        FindingCommand::Amend(a) => {
            mutate(
                endpoint,
                a.common,
                FindingMutation::Amend {
                    severity: a.severity.as_deref().map(severity),
                    blocking: a.blocking,
                },
            )
            .await
        }
        FindingCommand::Resolve(a) => {
            match super::checks::explanation(a.explanation.message, a.explanation.message_file) {
                Err(error) => Err(error),
                Ok(message) => forge_cli::request_record_finding_resolve(
                    endpoint,
                    RecordFindingResolveRequest {
                        record: a.record,
                        finding: a.finding,
                        resolution: FindingResolveInput {
                            expected_version: a.expected_version,
                            actor: a.explanation.actor,
                            message,
                            revision: a.explanation.revision,
                            dirty: a.explanation.dirty,
                        },
                    },
                )
                .await
                .map(value),
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
            eprintln!("forge record finding: {error}");
            1
        }
    }
}
