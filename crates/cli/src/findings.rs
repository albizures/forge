use clap::{Args, Subcommand};
use forge_rpc::{
    Finding, FindingRaiseInput, FindingResolveInput, Severity, TaskFindingListRequest,
    TaskFindingRaiseRequest, TaskFindingResolveRequest, TaskFindingShowRequest,
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
    task: u64,
    #[command(flatten)]
    page: Page,
    #[command(flatten)]
    output: Output,
}
#[derive(Debug, Args)]
pub struct ShowArgs {
    task: u64,
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
    task: u64,
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
    task: u64,
    finding: u64,
    #[arg(long)]
    expected_version: u64,
    #[command(flatten)]
    explanation: Explanation,
    #[command(flatten)]
    output: Output,
}
fn render(f: &Finding) -> String {
    format!(
        "Finding {} [{:?}] {:?}, blocking {}, version {}\nCheck {:?}, Assessment {:?}\nRaised by {} at {} (Assignment {:?}): {}\nrevision {:?}, dirty {:?}, comparison {:?}\nLatest Activity {} {:?} by {} at {} (Assignment {:?}): {}\nrevision {:?}, dirty {:?}, comparison {:?}",
        f.id,
        f.state,
        f.severity,
        f.blocking,
        f.version,
        f.check_id,
        f.assessment_id,
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
        f.latest_activity.comparison
    )
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
        FindingCommand::Resolve(a) => &a.output,
    };
    let json = output.json;
    let url = match super::resolve_managed_endpoint(output.server_url.as_deref()).await {
        Ok(url) => url,
        Err(error) => {
            eprintln!("forge task finding: {error}");
            return 1;
        }
    };
    let endpoint = (url.as_str(), workspace.as_deref());
    let result = match command {
        FindingCommand::List(a) => forge_cli::request_task_finding_list(endpoint, TaskFindingListRequest { task: a.task, after: a.page.after, limit: a.page.limit, compare_revision: a.page.compare_revision }).await.map(|page| {
            let mut text = page.findings.iter().map(render).collect::<Vec<_>>().join("\n");
            if let Some(cursor) = page.next_cursor { let _ = write!(text, "\nNext cursor: {cursor}"); }
            (serde_json::to_value(page).expect("serializable Findings"), text)
        }),
        FindingCommand::Show(a) => forge_cli::request_task_finding_show(endpoint, TaskFindingShowRequest { task: a.task, finding: a.finding, after: a.page.after, limit: a.page.limit, compare_revision: a.page.compare_revision }).await.map(|page| {
            let mut text = render(&page.finding);
            for activity in &page.activities { let _ = write!(text, "\nActivity {} {:?} by {} at {} (Assignment {:?}): {}\nrevision {:?}, dirty {:?}, comparison {:?}", activity.id, activity.action, activity.actor, activity.created_at_millis, activity.assignment_id, activity.message, activity.revision, activity.dirty, activity.comparison); }
            if let Some(cursor) = page.next_cursor { let _ = write!(text, "\nNext cursor: {cursor}"); }
            (serde_json::to_value(page).expect("serializable Finding page"), text)
        }),
        FindingCommand::Raise(a) => match super::checks::explanation(a.explanation.message, a.explanation.message_file) {
            Err(error) => Err(error),
            Ok(message) => forge_cli::request_task_finding_raise(endpoint, TaskFindingRaiseRequest { task: a.task, finding: FindingRaiseInput { severity: match a.severity.as_str() { "minor" => Severity::Minor, "major" => Severity::Major, _ => Severity::Critical }, blocking: a.blocking, check_id: a.check, assessment_id: a.assessment, actor: a.explanation.actor, message, revision: a.explanation.revision, dirty: a.explanation.dirty } }).await.map(value),
        },
        FindingCommand::Resolve(a) => match super::checks::explanation(a.explanation.message, a.explanation.message_file) {
            Err(error) => Err(error),
            Ok(message) => forge_cli::request_task_finding_resolve(endpoint, TaskFindingResolveRequest { task: a.task, finding: a.finding, resolution: FindingResolveInput { expected_version: a.expected_version, actor: a.explanation.actor, message, revision: a.explanation.revision, dirty: a.explanation.dirty } }).await.map(value),
        },
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
            eprintln!("forge task finding: {error}");
            1
        }
    }
}
