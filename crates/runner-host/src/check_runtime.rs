//! Writer of the runner's protected Check input (`check-runtime.json`).
//!
//! The trusted daemon calls [`write`] once, after reserving the canonical runtime
//! directory and before spawning `forge-runner-host` for the first admission. The
//! same immutable file is the input to every later same-attempt recovery of that
//! runtime; it is never rewritten, so a recovery cannot be rebound. The shipped
//! runner (`runner/src/check-runner.ts::readCheckRuntime`) re-validates it
//! strictly. Writing it grants nothing: execution still needs a current lease and
//! successful authenticated bootstrap.
use forge_app::RegistrationError;
use forge_rpc::durable_execution::{CHECK_RUNTIME_FILE, CheckRuntimeInput};
use std::{
    fs,
    io::{self, Write},
    path::Path,
};

/// Upper bound shared with the runner reader.
pub const MAX_BYTES: usize = 1024 * 1024;

fn failed() -> RegistrationError {
    RegistrationError::new(
        "runner-check-input-unavailable",
        "could not write the protected durable Check input; nothing was launched",
    )
}

/// Exclusively create the owner-only input in a verified runtime directory.
/// A partial write is retained and refused by the runner, never repaired here.
/// # Errors
/// Rejects inconsistent input, an unprotected directory, an existing input file
/// or any I/O failure.
pub fn write(directory: &Path, input: &CheckRuntimeInput) -> Result<(), RegistrationError> {
    input.validate()?;
    let bytes = serde_json::to_vec(input).map_err(|_| failed())?;
    if bytes.len() > MAX_BYTES {
        return Err(failed());
    }
    let result = (|| -> io::Result<()> {
        crate::directory(directory)?;
        let fd = rustix::fs::open(
            directory.join(CHECK_RUNTIME_FILE),
            rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )?;
        let mut file = fs::File::from(fd);
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::File::open(directory)?.sync_all()
    })();
    result.map_err(|_| failed())
}

#[cfg(test)]
#[path = "check_runtime_tests.rs"]
mod tests;
