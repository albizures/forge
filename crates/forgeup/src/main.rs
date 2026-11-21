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
        /// Packaged matched durable-runner bundle (see runner/package-bundle.mjs).
        #[arg(long)]
        runner_bundle: Option<PathBuf>,
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
    /// Select a Revision Installation without starting its daemon.
    Use {
        selector: String,
        /// Explicit rollback/fork: the source excludes work performed after upgrading.
        #[arg(long)]
        acknowledge_divergence: bool,
    },
    /// Explicitly carry database state into another registered revision.
    Upgrade {
        #[command(subcommand)]
        command: UpgradeCommand,
    },
    /// Clear the active Revision and remove the manager-owned `forge` path.
    Deactivate,
    /// Operate only the active Forge Revision's managed daemon.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Attest or resolve the matched durable-runner components shipped with Revisions.
    Runner {
        #[command(subcommand)]
        command: RunnerCommand,
    },
    /// Print paths to captured managed-daemon output for the active Revision.
    Logs,
    /// Check Forgeup's private locations, registry schema, and managed state.
    Doctor,
}

#[derive(Debug, Subcommand)]
enum UpgradeCommand {
    /// Read-only compatibility, migration chain, preservation evidence, and blockers.
    Plan {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
    },
    /// Snapshot, migrate, validate, and publish without activation or daemon startup.
    Prepare {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        /// Assert operator verification that old agents cannot act.
        #[arg(long)]
        confirm_agents_stopped: bool,
        /// JSON array of `{from_id,to_id}` completion-only resolutions for ambiguous legacy intent.
        #[arg(long)]
        waits_for_decisions: Option<std::path::PathBuf>,
    },
    /// Inspect retained progress and validation evidence.
    Status {
        #[arg(long)]
        to: String,
    },
    /// Validate and select a prepared revision; does not start its daemon.
    Activate {
        #[arg(long)]
        to: String,
        /// Deliberately accept a snapshot missing later source work.
        #[arg(long)]
        accept_stale_source: bool,
        #[arg(long)]
        confirm_agents_stopped: bool,
    },
    /// Recover completed publication or abandon an unpublished attempt, retaining artifacts.
    Recover {
        #[arg(long)]
        to: String,
    },
    /// Archive an unchanged, unactivated prepared copy and reset its destination for fresh preparation.
    Discard {
        #[arg(long)]
        to: String,
    },
    /// Delete backup/staging artifacts after activation; retain receipt and lineage guards.
    Cleanup {
        #[arg(long)]
        to: String,
    },
}

#[derive(Debug, Subcommand)]
enum RunnerCommand {
    /// Fully re-attest a Revision's installed runner (tree hash, lock, Node, host, protocol).
    Verify { selector: String },
    /// Print the verified components of the Revision that shipped this exact bundle tree
    /// hash or runner component hash (the `component_hash` a durable Launch records).
    Resolve {
        #[arg(long)]
        component_hash: String,
    },
}

#[derive(Debug, Subcommand)]
enum DaemonCommand {
    /// Report managed ownership for the active Revision, or inspect an explicit manual endpoint.
    Status {
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Start the active Revision's managed daemon.
    Start,
    /// Gracefully stop the active Revision's proved-owned managed daemon.
    Stop,
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
            runner_bundle,
        } => register(
            &source_label,
            &cli,
            &daemon,
            local_build_alias.as_deref(),
            forgeup::RegistrationOptions {
                replace: replace.as_deref(),
                reuse_state: reuse_state.as_deref(),
                purge_state,
                runner_bundle: runner_bundle.as_deref(),
            },
        ),
        Command::Remove {
            selector,
            purge_state,
        } => remove(&selector, purge_state),
        Command::List => list(),
        Command::Current { selector } => current(&selector),
        Command::Use {
            selector,
            acknowledge_divergence,
        } => use_revision(&selector, acknowledge_divergence),
        Command::Upgrade { command } => upgrade(command),
        Command::Deactivate => deactivate(),
        Command::Daemon { command } => daemon(command),
        Command::Runner { command } => runner(command),
        Command::Logs => logs(),
        Command::Doctor => doctor(),
    });
}
fn register(
    source_label: &str,
    cli: &std::path::Path,
    daemon: &std::path::Path,
    alias: Option<&str>,
    options: forgeup::RegistrationOptions<'_>,
) -> i32 {
    match forgeup::register_with_options(source_label, cli, daemon, alias, options) {
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
fn use_revision(selector: &str, acknowledge: bool) -> i32 {
    let result = if acknowledge {
        forgeup::use_revision_acknowledging_divergence(selector)
    } else {
        forgeup::use_revision(selector)
    };
    match result {
        Ok(item) => {
            println!("{}", item.selector);
            0
        }
        Err(error) => {
            eprintln!("forgeup use: {error}");
            1
        }
    }
}
fn deactivate() -> i32 {
    match forgeup::deactivate() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("forgeup deactivate: {error}");
            1
        }
    }
}
fn daemon(command: DaemonCommand) -> i32 {
    let result = match command {
        DaemonCommand::Status { endpoint } => forgeup::daemon_status(endpoint.as_deref()),
        DaemonCommand::Start => forgeup::daemon_start(),
        DaemonCommand::Stop => forgeup::daemon_stop(),
    };
    match result {
        Ok(status) => {
            println!("{status}");
            0
        }
        Err(error) => {
            eprintln!("forgeup daemon: {error}");
            1
        }
    }
}
fn runner(command: RunnerCommand) -> i32 {
    let result = match command {
        RunnerCommand::Verify { selector } => forgeup::current(&selector),
        RunnerCommand::Resolve { component_hash } => forgeup::runner_for_component(&component_hash),
    }
    .and_then(|item| {
        forgeup::verify_runner(&item).map(|runner| {
            format!(
                "{}\nnode\t{}\nhost\t{}\nentry\t{}\ninspector\t{}\nprotocol\t{}\nnode-version\t{}\nlock-sha256\t{}\ntree-sha256\t{}\ncomponent-sha256\t{}",
                item.selector,
                runner.node_path.display(),
                runner
                    .host_path
                    .as_ref()
                    .map_or_else(|| "(schema 1: none; inspection only)".into(), |p| p.display().to_string()),
                runner.entry_path.display(),
                runner.inspector_path.display(),
                runner.protocol,
                runner.node_version,
                runner.package_lock_sha256,
                runner.tree_sha256,
                runner.component_sha256.as_deref().unwrap_or("(schema 1: not recorded)"),
            )
        })
    });
    match result {
        Ok(output) => {
            println!("{output}");
            0
        }
        Err(error) => {
            eprintln!("forgeup runner: {error}");
            1
        }
    }
}
fn logs() -> i32 {
    match forgeup::logs() {
        Ok(paths) => {
            println!("stdout\t{}", paths.stdout.display());
            println!("stderr\t{}", paths.stderr.display());
            0
        }
        Err(error) => {
            eprintln!("forgeup logs: {error}");
            1
        }
    }
}
fn upgrade(command: UpgradeCommand) -> i32 {
    use forgeup::upgrade;
    let result = match command {
        UpgradeCommand::Plan { from, to } => upgrade::plan(&from, &to).and_then(json_output),
        UpgradeCommand::Prepare {
            from,
            to,
            confirm_agents_stopped,
            waits_for_decisions,
        } => {
            let decisions = waits_for_decisions.map_or_else(
                || Ok(vec![]),
                |path| {
                    std::fs::read(&path)
                        .map_err(|error| forgeup::ForgeupError::UpgradeBlocked(error.to_string()))
                        .and_then(|bytes| {
                            serde_json::from_slice(&bytes).map_err(|error| {
                                forgeup::ForgeupError::UpgradeBlocked(error.to_string())
                            })
                        })
                },
            );
            decisions
                .and_then(|decisions| {
                    upgrade::prepare_with_decisions(&from, &to, confirm_agents_stopped, decisions)
                })
                .and_then(json_output)
        }
        UpgradeCommand::Status { to } => upgrade::status(&to).and_then(json_output),
        UpgradeCommand::Activate {
            to,
            accept_stale_source,
            confirm_agents_stopped,
        } => upgrade::activate(&to, accept_stale_source, confirm_agents_stopped)
            .and_then(json_output),
        UpgradeCommand::Recover { to } => upgrade::recover(&to).and_then(json_output),
        UpgradeCommand::Discard { to } => upgrade::discard(&to).and_then(json_output),
        UpgradeCommand::Cleanup { to } => upgrade::cleanup(&to).and_then(json_output),
    };
    match result {
        Ok(output) => {
            println!("{output}");
            0
        }
        Err(error) => {
            eprintln!("forgeup upgrade: {error}");
            1
        }
    }
}
fn json_output(value: impl serde::Serialize) -> Result<String, forgeup::ForgeupError> {
    serde_json::to_string_pretty(&value)
        .map_err(|error| forgeup::ForgeupError::UpgradeBlocked(error.to_string()))
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
