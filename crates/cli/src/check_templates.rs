use clap::{Args, Subcommand};
use forge_rpc::{
    CheckCriterion, CheckTemplateSetRequest, CheckTemplateShowRequest, TemplatePage,
    TemplateReplacement,
};
use serde::Serialize;
use std::{fmt::Write as _, io::Read};

#[derive(Debug, Subcommand)]
pub enum CheckTemplateCommand {
    /// Show current criteria and Stack readiness; add --history for replacements.
    Show(ShowArgs),
    /// Read-only preflight for future Dispatched cumulative Stack Tasks; exits 1 on blockers.
    Preflight(PreflightArgs),
    /// Replace future Task criteria, starting with the mandatory Implementation entry.
    Set(SetArgs),
}
#[derive(Debug, Args)]
pub struct ShowArgs {
    pub project: String,
    /// Include the bounded ascending replacement history page.
    #[arg(long)]
    history: bool,
    /// History cursor: the `next_cursor` of the previous page.
    #[arg(long, requires = "history")]
    after: Option<u64>,
    /// History page size, 1–100.
    #[arg(long, default_value_t = 50, requires = "history")]
    limit: u32,
    #[arg(long)]
    pub server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
pub struct PreflightArgs {
    pub project: String,
    #[arg(long)]
    pub server_url: Option<String>,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("message_source").required(true).args(["message", "message_file"])))]
pub struct SetArgs {
    pub project: String,
    #[arg(long)]
    file: String,
    #[arg(long)]
    expected_version: u64,
    #[arg(long)]
    actor: String,
    #[arg(long)]
    message: Option<String>,
    #[arg(long)]
    message_file: Option<String>,
    #[arg(long)]
    pub server_url: Option<String>,
    #[arg(long)]
    json: bool,
}

/// Machine-readable planning verdict: only what a planner records or reports.
#[derive(Debug, Serialize)]
struct PreflightReport<'a> {
    ok: bool,
    version: u64,
    checks: Vec<CheckSummary<'a>>,
    blockers: &'a [String],
}
#[derive(Debug, Serialize)]
struct CheckSummary<'a> {
    name: &'a str,
    profile_revision: Option<u32>,
}

/// Rendered command result; `ok == false` prints the report and exits 1.
struct Rendered {
    ok: bool,
    value: serde_json::Value,
    text: String,
}

fn read_message(message: Option<String>, file: Option<String>) -> Result<String, String> {
    match (message, file) {
        (Some(message), None) => Ok(message),
        (None, Some(file)) if file == "-" => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|error| error.to_string())?;
            Ok(text)
        }
        (None, Some(file)) => std::fs::read_to_string(file).map_err(|error| error.to_string()),
        _ => Err("exactly one explanation source is required".into()),
    }
}

fn to_value(value: &impl Serialize) -> Result<serde_json::Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

pub async fn run(command: CheckTemplateCommand, workspace: Option<String>) -> i32 {
    let (url, json) = match &command {
        CheckTemplateCommand::Show(a) => (a.server_url.clone(), a.json),
        CheckTemplateCommand::Preflight(a) => (a.server_url.clone(), a.json),
        CheckTemplateCommand::Set(a) => (a.server_url.clone(), a.json),
    };
    let result: Result<Rendered, String> = async {
        let url = super::resolve_managed_endpoint(url.as_deref()).await?;
        let endpoint = (url.as_str(), workspace.as_deref());
        match command {
            CheckTemplateCommand::Show(a) => {
                // The server always returns a history page; without --history
                // request the smallest one and omit it from every output.
                let (after, limit) = if a.history {
                    (a.after, a.limit)
                } else {
                    (None, 1)
                };
                let page = forge_cli::request_project_check_template_show(
                    endpoint,
                    CheckTemplateShowRequest {
                        project: a.project,
                        after,
                        limit,
                    },
                )
                .await?;
                let mut value = to_value(&page)?;
                if !a.history
                    && let Some(object) = value.as_object_mut()
                {
                    object.remove("history");
                    object.remove("next_cursor");
                }
                Ok(Rendered {
                    ok: true,
                    value,
                    text: render_show(&page, a.history),
                })
            }
            CheckTemplateCommand::Preflight(a) => {
                let page = forge_cli::request_project_check_template_show(
                    endpoint,
                    CheckTemplateShowRequest {
                        project: a.project,
                        after: None,
                        limit: 1,
                    },
                )
                .await?;
                let report = preflight_report(&page)?;
                Ok(Rendered {
                    ok: report.ok,
                    value: to_value(&report)?,
                    text: render_preflight(&report),
                })
            }
            CheckTemplateCommand::Set(a) => {
                let criteria = serde_json::from_str(
                    &std::fs::read_to_string(a.file).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let message = read_message(a.message, a.message_file)?;
                let template = forge_cli::request_project_check_template_set(
                    endpoint,
                    CheckTemplateSetRequest {
                        project: a.project,
                        replacement: TemplateReplacement {
                            expected_version: a.expected_version,
                            criteria,
                            actor: a.actor,
                            message,
                        },
                    },
                )
                .await?;
                Ok(Rendered {
                    ok: true,
                    value: to_value(&template)?,
                    text: render_template(template.version, &template.criteria),
                })
            }
        }
    }
    .await;
    match result {
        Ok(rendered) => {
            if json {
                println!("{}", rendered.value);
            } else {
                println!("{}", rendered.text);
            }
            i32::from(!rendered.ok)
        }
        Err(error) => {
            eprintln!("forge project check-template: {error}");
            1
        }
    }
}

/// Older servers omit `stack_preflight`; absence is never a passing preflight.
fn preflight_report(page: &TemplatePage) -> Result<PreflightReport<'_>, String> {
    let preflight = page.stack_preflight.as_ref().ok_or_else(|| {
        "server does not expose Stack template preflight; update Forge before planning".to_string()
    })?;
    Ok(PreflightReport {
        ok: preflight.blockers.is_empty(),
        version: page.version,
        checks: page
            .criteria
            .iter()
            .map(|criterion| CheckSummary {
                name: &criterion.name,
                profile_revision: criterion.profile_revision,
            })
            .collect(),
        blockers: &preflight.blockers,
    })
}

fn profile(revision: Option<u32>) -> String {
    revision.map_or_else(
        || "no profile".into(),
        |revision| format!("profile r{revision}"),
    )
}

fn write_blockers(text: &mut String, blockers: &[String]) {
    for blocker in blockers {
        let _ = write!(text, "\n  - {blocker}");
    }
}

fn render_preflight(report: &PreflightReport<'_>) -> String {
    let verdict = if report.ok { "passed" } else { "failed" };
    let mut text = format!(
        "Stack preflight {verdict}: Check Template version {}",
        report.version
    );
    write_blockers(&mut text, report.blockers);
    let chain = report
        .checks
        .iter()
        .map(|check| format!("{} ({})", check.name, profile(check.profile_revision)))
        .collect::<Vec<_>>()
        .join(" → ");
    let _ = write!(text, "\nChecks: {chain}");
    text
}

fn write_criteria(
    text: &mut String,
    criteria: &[CheckCriterion],
    indent: &str,
    descriptions: bool,
) {
    for (index, criterion) in criteria.iter().enumerate() {
        let _ = write!(
            text,
            "\n{indent}{}. {} [{}]",
            index + 1,
            criterion.name,
            profile(criterion.profile_revision)
        );
        if descriptions {
            let _ = write!(text, "\n{indent}   {}", criterion.description);
        }
    }
}

fn render_template(version: u64, criteria: &[CheckCriterion]) -> String {
    let mut text = format!("Check Template version {version}");
    write_criteria(&mut text, criteria, "  ", true);
    text
}

fn render_show(page: &TemplatePage, history: bool) -> String {
    let mut text = render_template(page.version, &page.criteria);
    match &page.stack_preflight {
        Some(preflight) if preflight.blockers.is_empty() => {
            text.push_str("\nStack preflight: ready");
        }
        Some(preflight) => {
            text.push_str("\nStack preflight: blocked");
            write_blockers(&mut text, &preflight.blockers);
        }
        None => text.push_str("\nStack preflight: not reported by this server"),
    }
    if !history {
        return text;
    }
    text.push_str("\n\nHistory:");
    if page.history.is_empty() {
        text.push_str(" none");
    }
    for change in &page.history {
        let _ = write!(
            text,
            "\n  version {} (change {}) by {} at {}",
            change.version,
            change.id,
            change.actor,
            format_millis(change.created_at_millis)
        );
        for line in change.message.lines() {
            let _ = write!(text, "\n    {line}");
        }
        write_criteria(&mut text, &change.criteria, "    ", false);
    }
    if let Some(cursor) = page.next_cursor {
        let _ = write!(text, "\nMore history: --after {cursor}");
    }
    text
}

fn format_millis(millis: u64) -> String {
    time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000)
        .ok()
        .and_then(|date| {
            date.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        // Out-of-range values are shown truthfully rather than replaced.
        .unwrap_or_else(|| format!("{millis} ms since Unix epoch"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(criteria: &serde_json::Value, blockers: Option<Vec<&str>>) -> TemplatePage {
        let mut page = serde_json::json!({
            "version": 5, "criteria": criteria, "history": [], "next_cursor": null
        });
        if let Some(blockers) = blockers {
            page["stack_preflight"] = serde_json::json!({ "blockers": blockers });
        }
        serde_json::from_value(page).unwrap()
    }

    #[test]
    fn preflight_rejects_older_servers_and_reports_blockers() {
        let criteria = serde_json::json!([
            {"name": "Implementation", "description": "Deliver", "profile_revision": 24},
            {"name": "Review", "description": "Review", "profile_revision": null}
        ]);
        let old = page(&criteria, None);
        assert!(preflight_report(&old).unwrap_err().contains("update Forge"));

        let blocked = page(&criteria, Some(vec!["final Check must be Publication"]));
        let report = preflight_report(&blocked).unwrap();
        assert!(!report.ok);
        let text = render_preflight(&report);
        assert!(text.starts_with("Stack preflight failed: Check Template version 5"));
        assert!(text.contains("  - final Check must be Publication"));
        assert!(text.contains("Implementation (profile r24) → Review (no profile)"));
        assert_eq!(
            to_value(&report).unwrap(),
            serde_json::json!({
                "ok": false, "version": 5,
                "checks": [
                    {"name": "Implementation", "profile_revision": 24},
                    {"name": "Review", "profile_revision": null}
                ],
                "blockers": ["final Check must be Publication"]
            })
        );

        let ready = page(&criteria, Some(vec![]));
        let report = preflight_report(&ready).unwrap();
        assert!(report.ok);
        assert!(render_preflight(&report).starts_with("Stack preflight passed"));
    }

    #[test]
    fn show_hides_history_unless_requested_and_formats_values() {
        let mut page = page(
            &serde_json::json!([{"name": "Implementation", "description": "Deliver", "profile_revision": 2}]),
            Some(vec![]),
        );
        page.history = serde_json::from_value(serde_json::json!([{
            "id": 7, "version": 1, "actor": "operator", "message": "Pin\nprofiles",
            "created_at_millis": 1000,
            "criteria": [{"name": "Implementation", "description": "Old", "profile_revision": null}]
        }]))
        .unwrap();
        page.next_cursor = Some(7);
        let brief = render_show(&page, false);
        assert_eq!(
            brief,
            "Check Template version 5\n  1. Implementation [profile r2]\n     Deliver\nStack preflight: ready"
        );
        let full = render_show(&page, true);
        assert!(full.contains("version 1 (change 7) by operator at 1970-01-01T00:00:01Z"));
        assert!(full.contains("\n    Pin\n    profiles\n    1. Implementation [no profile]"));
        assert!(!full.contains("Old"));
        assert!(full.ends_with("More history: --after 7"));
        assert!(!full.contains("Some("));
    }
}
