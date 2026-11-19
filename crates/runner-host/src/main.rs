//! Linux single-owner supervisor executable. Production dispatch remains disabled.
use std::path::Path;

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        eprintln!(
            "usage: forge-runner-host RUNTIME_DIRECTORY WORKTREE ABSOLUTE_NODE ABSOLUTE_RUNNER"
        );
        std::process::exit(1);
    }
    if let Err(error) = forge_runner_host::supervisor::run(
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
    ) {
        eprintln!("runner-host: {error}");
        std::process::exit(1);
    }
}
