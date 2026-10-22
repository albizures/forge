use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "forgeup", about = "Manage Forge Revisions")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Register immutable snapshots of a Forge CLI and daemon pair.
    Register {
        #[arg(long)]
        source_label: String,
        #[arg(long)]
        cli: PathBuf,
        #[arg(long)]
        daemon: PathBuf,
        #[arg(long)]
        local_build_alias: Option<String>,
        /// Explicitly replace this Revision Installation (canonical selector or alias).
        #[arg(long)]
        replace: Option<String>,
        /// Reattach the retained state belonging to this canonical selector.
        #[arg(long)]
        reuse_state: Option<String>,
        /// Discard Revision State while replacing the Installation.
        #[arg(long, requires = "replace")]
        purge_state: bool,
    },
    /// Remove a Revision Installation, preserving its state by default.
    Remove {
        selector: String,
        #[arg(long)]
        purge_state: bool,
    },
    /// List registered Revision Installations.
    List,
    /// Show the Revision Installation resolved by a selector or local-build alias.
    Current { selector: String },
    /// Check Forgeup's private locations and registry schema.
    Doctor,
}

fn main() {
    std::process::exit(match Cli::parse().command {
        Command::Register {
            source_label,
            cli,
            daemon,
            local_build_alias,
            replace,
            reuse_state,
            purge_state,
        } => register(
            &source_label,
            &cli,
            &daemon,
            local_build_alias.as_deref(),
            replace.as_deref(),
            reuse_state.as_deref(),
            purge_state,
        ),
        Command::Remove {
            selector,
            purge_state,
        } => remove(&selector, purge_state),
        Command::List => list(),
        Command::Current { selector } => current(&selector),
        Command::Doctor => doctor(),
    });
}
fn register(
    source_label: &str,
    cli: &std::path::Path,
    daemon: &std::path::Path,
    alias: Option<&str>,
    replace: Option<&str>,
    reuse_state: Option<&str>,
    purge_state: bool,
) -> i32 {
    match forgeup::register_with_options(
        source_label,
        cli,
        daemon,
        alias,
        forgeup::RegistrationOptions {
            replace,
            reuse_state,
            purge_state,
        },
    ) {
        Ok(item) => {
            println!("{}", item.selector);
            0
        }
        Err(error) => {
            eprintln!("forgeup register: {error}");
            1
        }
    }
}
fn remove(selector: &str, purge_state: bool) -> i32 {
    match forgeup::remove(selector, purge_state) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("forgeup remove: {error}");
            1
        }
    }
}
fn list() -> i32 {
    match forgeup::list() {
        Ok(items) => {
            for item in items {
                println!(
                    "{}\t{}\t{}",
                    item.selector,
                    item.local_build_alias.unwrap_or_else(|| "-".into()),
                    item.endpoint
                );
            }
            0
        }
        Err(error) => {
            eprintln!("forgeup list: {error}");
            1
        }
    }
}
fn current(selector: &str) -> i32 {
    match forgeup::current(selector) {
        Ok(item) => {
            println!("{}", item.selector);
            0
        }
        Err(error) => {
            eprintln!("forgeup current: {error}");
            1
        }
    }
}
fn doctor() -> i32 {
    match forgeup::doctor() {
        Ok(report) => {
            println!("Forgeup foundation is healthy");
            println!("config\t{}", report.paths.config.display());
            println!("data\t{}", report.paths.data.display());
            println!("runtime\t{}", report.paths.runtime.display());
            println!("bin\t{}", report.paths.bin.display());
            println!("registry-schema\t{}", report.schema_version);
            0
        }
        Err(error) => {
            eprintln!("forgeup doctor: {error}");
            1
        }
    }
}
