//! Trusted composition seam for future durable dispatch. No operator RPC,
//! Assignment capability, renewal transport or launch path exposes this service.
//! One `DaemonAuthority` belongs to one daemon lifetime, never retained on disk.
use forge_app::{
    RegistrationError,
    authorization_lease::{Issuer, Lease},
    durable_execution::Launch,
};
use forge_runner_host::SpawnedOwner;
use forge_store::SqliteProjectRepository;
use std::{path::Path, process::Child};

/// A fresh cryptographic epoch fences previous daemon lifetimes. This is not a
/// credential, heartbeat or proof that old supervised processes have exited.
pub struct DaemonAuthority {
    epoch: String,
}

impl DaemonAuthority {
    /// Create once in the daemon's trusted composition root when durable launch
    /// is wired. Never accept a persisted or caller-supplied epoch as a shortcut.
    /// # Errors
    /// Fails closed if the operating system cannot generate fresh identity.
    pub fn new() -> Result<Self, RegistrationError> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| {
            RegistrationError::new(
                "assignment-epoch-unavailable",
                "fresh daemon authorization identity is unavailable",
            )
        })?;
        Ok(Self {
            epoch: bytes
                .iter()
                .fold(String::with_capacity(64), |mut epoch, byte| {
                    use std::fmt::Write;
                    write!(epoch, "{byte:02x}").expect("writing to a String cannot fail");
                    epoch
                }),
        })
    }

    /// Bind immutable Forge state to a directly spawned, ready supervisor. The
    /// directory must come from Forge's canonical runtime mapping, not an RPC
    /// path. A retained owner file or lease alone cannot construct this service.
    /// # Errors
    /// Rejects malformed bindings, unsafe storage or unverified live ownership.
    pub fn bind<'a>(
        &self,
        workspace: String,
        launch: Launch,
        generation: u64,
        supervisor: &'a mut Child,
        directory: &Path,
    ) -> Result<Authorization<'a>, RegistrationError> {
        let issuer = Issuer::new(workspace, launch, generation, self.epoch.clone())?;
        let host = SpawnedOwner::capture(supervisor, directory)?;
        Ok(Authorization { issuer, host })
    }
}

/// The server selects the repository; there are no grant/owner/clock parameters
/// supplied by the agent. Call at the application-defined renewal cadence and
/// deliver only success. Dropping this handle does not stop work or release any
/// capacity: the independent host watchdog and retained blockers remain required.
pub struct Authorization<'a> {
    issuer: Issuer,
    host: SpawnedOwner<'a>,
}

impl Authorization<'_> {
    /// Reobserve the kernel owner/clock, then let the application/store validate
    /// and append current bounded authority. No transport or heartbeat implies
    /// successful authorization. Consumers must check the immutable deadline.
    /// # Errors
    /// Any failed observation or uncertain persistence permanently fences this
    /// handle. It cannot renew locally or revive after the old deadline expires.
    pub fn issue(
        &mut self,
        repository: &SqliteProjectRepository,
    ) -> Result<Lease, RegistrationError> {
        self.issuer.issue(repository, &mut self.host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_lifetimes_have_distinct_non_retained_epochs() {
        let first = DaemonAuthority::new().unwrap();
        let second = DaemonAuthority::new().unwrap();
        assert_eq!(first.epoch.len(), 64);
        assert!(first.epoch.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first.epoch, second.epoch);
    }
}
