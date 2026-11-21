//! Linux single-owner supervisor executable, spawned by the daemon's durable launcher.

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    std::process::exit(forge_runner_host::entry::run(&args));
}
