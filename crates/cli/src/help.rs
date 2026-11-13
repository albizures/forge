use clap::{Args, Command, CommandFactory, FromArgMatches, error::ErrorKind};
use std::fmt::Write;

#[derive(Debug, Args)]
struct HelpArgs {
    /// Include detailed help for every discoverable descendant.
    #[arg(long)]
    all: bool,
    /// Command path to describe.
    #[arg(value_name = "COMMAND")]
    path: Vec<String>,
}

// Extend only the root help entry in a parsing-only tree. Ordinary invocations
// still use Clap's original parser, including its nested synthetic help commands.
pub(super) fn recursive(arguments: &[String]) -> Option<Result<String, clap::Error>> {
    if !arguments.iter().any(|argument| argument == "--all") {
        return None;
    }
    let parser = super::Cli::command()
        .disable_help_subcommand(true)
        .subcommand(HelpArgs::augment_args(Command::new("help")));
    let matches = parser.try_get_matches_from(arguments).ok()?;
    let ("help", matches) = matches.subcommand()? else {
        return None;
    };
    let args = HelpArgs::from_arg_matches(matches).expect("help arguments match their definition");
    args.all.then(|| render(super::Cli::command(), &args))
}

fn render(mut root: Command, args: &HelpArgs) -> Result<String, clap::Error> {
    root.build();
    let mut selected = &mut root;
    let mut path = String::from("forge");
    for name in &args.path {
        // Resolve before borrowing mutably so errors retain the current usage.
        let canonical = selected
            .find_subcommand(name)
            .map(|command| command.get_name().to_owned())
            .ok_or_else(|| {
                selected.error(
                    ErrorKind::InvalidSubcommand,
                    format!("unrecognized command '{name}' in '{path}'"),
                )
            })?;
        path.push(' ');
        path.push_str(&canonical);
        selected = selected.find_subcommand_mut(&canonical).unwrap();
    }
    let mut output = String::new();
    render_tree(selected, &path, &mut output);
    Ok(output)
}

fn render_tree(command: &mut Command, path: &str, output: &mut String) {
    writeln!(output, "=== {path} ===\n").unwrap();
    writeln!(output, "{}", command.render_long_help()).unwrap();
    for child in command.get_subcommands_mut() {
        if child.is_hide_set() || child.get_name() == "help" {
            continue;
        }
        let child_path = format!("{path} {}", child.get_name());
        render_tree(child, &child_path, output);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_skips_hidden_commands_and_help_and_uses_canonical_paths() {
        let mut command = Command::new("forge")
            .disable_help_subcommand(true)
            .subcommand(Command::new("visible").alias("v"))
            .subcommand(Command::new("secret").hide(true))
            .subcommand(Command::new("help"));
        command.build();
        let aliased = render(
            command.clone(),
            &HelpArgs {
                all: true,
                path: vec!["v".into()],
            },
        )
        .unwrap();
        assert!(aliased.starts_with("=== forge visible ==="));
        assert!(aliased.contains("Usage: forge visible"));
        let mut output = String::new();
        render_tree(&mut command, "forge", &mut output);
        assert!(output.contains("=== forge visible ==="));
        for excluded in [
            "=== forge secret ===",
            "=== forge help ===",
            "=== forge v ===",
        ] {
            assert!(!output.contains(excluded));
        }
    }
}
