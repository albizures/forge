//! The `forge-runner-host` executable entry, shared by the packaged binary and
//! test compositions that re-execute the same library.
use serde::{Deserialize, Serialize};
use std::{ffi::OsString, path::Path};

/// Asks a host for its compatibility instead of supervising a runner.
pub const COMPATIBILITY_FLAG: &str = "--compatibility";

/// What this host build implements. Printed as one JSON line by
/// `forge-runner-host --compatibility`, and checked by Forgeup attestation and
/// daemon launch preflight before any durable allocation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCompatibility {
    /// Forge-owned runner wire protocol.
    pub protocol: u32,
    /// Durable runtime store schema.
    pub runtime_schema: u32,
    /// Forge Revision of this build: the commit, with `-dirty` for a modified checkout.
    pub forge_revision: String,
}

/// This build's compatibility.
#[must_use]
pub fn compatibility() -> HostCompatibility {
    let build = forge_build_info::current();
    HostCompatibility {
        protocol: forge_rpc::durable_execution::RUNNER_PROTOCOL_VERSION,
        runtime_schema: forge_rpc::durable_execution::DURABLE_RUNTIME_SCHEMA,
        forge_revision: if build.dirty {
            format!("{}-dirty", build.commit)
        } else {
            build.commit.to_owned()
        },
    }
}

/// Run the host with its arguments (without the program name); returns the exit code.
#[must_use]
pub fn run(args: &[OsString]) -> i32 {
    if args.len() == 1 && args[0] == COMPATIBILITY_FLAG {
        return match serde_json::to_string(&compatibility()) {
            Ok(line) => {
                println!("{line}");
                0
            }
            Err(_) => 1,
        };
    }
    if args.len() != 4 {
        eprintln!(
            "usage: forge-runner-host RUNTIME_DIRECTORY WORKTREE ABSOLUTE_NODE ABSOLUTE_RUNNER\n       forge-runner-host {COMPATIBILITY_FLAG}"
        );
        return 1;
    }
    if let Err(error) = crate::supervisor::run(
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
    ) {
        eprintln!("runner-host: {error}");
        return 1;
    }
    0
}
