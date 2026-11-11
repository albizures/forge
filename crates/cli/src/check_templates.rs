use clap::{Args, Subcommand};
use forge_rpc::{CheckTemplateSetRequest, CheckTemplateShowRequest, TemplateReplacement};
use std::{fmt::Write as _, io::Read};

#[derive(Debug, Subcommand)]
pub enum CheckTemplateCommand {
    /// Show current criteria and a bounded ascending replacement history page.
    Show(ShowArgs),
    /// Replace the ordered criteria for future Tasks (an empty array clears extras).
    Set(SetArgs),
}
#[derive(Debug, Args)]
pub struct ShowArgs {
    pub project: String,
    #[arg(long)]
    after: Option<u64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
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

pub async fn run(command: CheckTemplateCommand, workspace: Option<String>) -> i32 {
    let (url, json) = match &command {
        CheckTemplateCommand::Show(a) => (a.server_url.clone(), a.json),
        CheckTemplateCommand::Set(a) => (a.server_url.clone(), a.json),
    };
    let result: Result<(serde_json::Value, String), String> = async {
        let url = super::resolve_managed_endpoint(url.as_deref()).await?;
        let endpoint = (url.as_str(), workspace.as_deref());
        match command {
            CheckTemplateCommand::Show(a) => {
                let page = forge_cli::request_project_check_template_show(
                    endpoint,
                    CheckTemplateShowRequest {
                        project: a.project,
                        after: a.after,
                        limit: a.limit,
                    },
                )
                .await?;
                let mut text = render_template(page.version, &page.criteria);
                for change in &page.history {
                    let _ = write!(
                        text,
                        "\nChange {} version {} by {} at {}\n{}",
                        change.id,
                        change.version,
                        change.actor,
                        change.created_at_millis,
                        change.message
                    );
                    for criterion in &change.criteria {
                        let _ = write!(text, "\n  {}: {}", criterion.name, criterion.description);
                    }
                }
                if let Some(cursor) = page.next_cursor {
                    let _ = write!(text, "\nNext cursor: {cursor}");
                }
                Ok((
                    serde_json::to_value(page).expect("serializable template page"),
                    text,
                ))
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
                let text = render_template(template.version, &template.criteria);
                Ok((
                    serde_json::to_value(template).expect("serializable template"),
                    text,
                ))
            }
        }
    }
    .await;
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
            eprintln!("forge project check-template: {error}");
            1
        }
    }
}
fn render_template(version: u64, criteria: &[forge_rpc::CheckCriterion]) -> String {
    let mut text = format!("Check Template version {version} (Implementation is always required)");
    for criterion in criteria {
        let _ = write!(text, "\n{}: {}", criterion.name, criterion.description);
    }
    text
}
