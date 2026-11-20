//! Reinspect predecessor/runtime proof under a directly spawned waiting owner.
//! No PID adoption, old-lease delivery, writable open or waiting-consumer setup.
use super::{recovery_inspection, runtime_directory};
use forge_app::{
    RegistrationError,
    authorization_lease::{AuthorityHost, Observation},
    durable_execution::recovery::{Candidate, Claim, Repository as _, quiescence::Operation},
};
use forge_rpc::durable_execution::recovery_inspection::PendingInput;
use forge_runner_host::SpawnedOwner;
use forge_store::SqliteProjectRepository;
use std::{
    path::{Path, PathBuf},
    process::Child,
};

fn denied() -> RegistrationError {
    RegistrationError::new(
        "durable-waiting-inspection-unavailable",
        "waiting-owner recovery proof unavailable; preserve evidence and capacity; no recovery is authorized",
    )
}

struct Ownership<'a> {
    host: SpawnedOwner<'a>,
    directory: PathBuf,
    candidate: Candidate,
    operation: Operation,
    fenced: bool,
}

impl Ownership<'_> {
    fn observe(
        &mut self,
        repository: &SqliteProjectRepository,
        workspace: &str,
    ) -> Result<Observation, RegistrationError> {
        if self.fenced {
            return Err(denied());
        }
        self.fenced = true;
        let before = self.observe_owner_inner()?;
        if runtime_directory::mapped(repository, workspace, &self.candidate.launch, false)?
            != self.directory
        {
            return Err(denied());
        }
        if repository.claimed_recovery_candidate(workspace, &self.operation.claim)?
            != self.candidate
        {
            return Err(denied());
        }
        let after = self.observe_owner_inner()?;
        if after.owner != before.owner || after.boottime_millis < before.boottime_millis {
            return Err(denied());
        }
        self.fenced = false;
        Ok(after)
    }

    fn observe_owner_inner(&mut self) -> Result<Observation, RegistrationError> {
        let before = self.host.observe()?;
        forge_runner_host::quiescence::waiting_receipt(&self.directory, &self.operation)
            .map_err(|_| denied())?;
        let after = self.host.observe()?;
        if after.owner != before.owner || after.boottime_millis < before.boottime_millis {
            return Err(denied());
        }
        Ok(after)
    }
}

/// Retains the direct child borrow and verifies its shared kernel ownership.
/// The supervisor/worker, not a daemon inspection descriptor, hold this lock.
/// A preceding daemon inspection must NOT be reused across spawning: this adapter
/// freshly reinspects under the new owner and revalidates the active claim.
#[must_use = "retain verified waiting ownership through dependent operations"]
pub struct Inspection<'a> {
    ownership: Ownership<'a>,
    input: PendingInput,
}

impl Inspection<'_> {
    #[must_use]
    pub fn candidate(&self) -> &Candidate {
        &self.ownership.candidate
    }

    #[must_use]
    pub fn input(&self) -> &PendingInput {
        &self.input
    }

    /// Reobserve the direct waiting owner, protected predecessor receipt and live
    /// original claim. Any failure terminally fences this verifier, including
    /// temporary receipt loss or state drift later restored by another writer.
    /// # Errors
    /// Rejects owner/process/clock drift, missing proof and changed live policy.
    pub fn observe(
        &mut self,
        repository: &SqliteProjectRepository,
        workspace: &str,
    ) -> Result<Observation, RegistrationError> {
        self.ownership.observe(repository, workspace)
    }
}

/// Physical owner/proof observations remain available across claim consumption.
/// The recovery coordinator revalidates live authority in its grant transaction;
/// using the pre-commit claimed read after commit would incorrectly reject the
/// just-consumed claim. This port never adopts historical authority or grants it.
impl AuthorityHost for Inspection<'_> {
    fn observe(&mut self) -> Result<Observation, RegistrationError> {
        if self.ownership.fenced {
            return Err(denied());
        }
        self.ownership.fenced = true;
        let observed = self.ownership.observe_owner_inner()?;
        self.ownership.fenced = false;
        Ok(observed)
    }
}

/// Inspect an explicitly spawned ready but ungranted supervisor. Its kernel lock
/// is independently verified BEFORE and AFTER bounded original-input/effect I/O.
/// The original receipt remains bound to its old owner and unchanged lock inode;
/// the new owner is separately captured from the actual Child, never old metadata.
/// Releasing an earlier read-only guard to spawn cannot carry its observations
/// forward: all runtime/process/live-claim gates here are fresh under the new lock.
///
/// This is not a `RecoveryHost`: Git policy, protected proof persistence, exact
/// consumer expectation, fresh transactional grant and writable reopen are absent.
/// Neither this call nor dropping its result signals/cleans/releases the child.
/// # Errors
/// Rejects live competition, unsafe mapping, missing proof, host faults, stale
/// candidates, settled original input, interrupted effects and unavailable I/O.
pub async fn inspect<'a>(
    repository: &SqliteProjectRepository,
    workspace: &str,
    claim: &Claim,
    supervisor: &'a mut Child,
    node: &Path,
    inspector: &Path,
) -> Result<Inspection<'a>, RegistrationError> {
    let candidate = repository.claimed_recovery_candidate(workspace, claim)?;
    let operation = Operation::for_candidate(&candidate, claim.clone())?;
    let directory = runtime_directory::mapped(repository, workspace, &candidate.launch, false)?;
    let host = SpawnedOwner::capture(supervisor, &directory)?;
    let mut ownership = Ownership {
        host,
        directory,
        candidate,
        operation,
        fenced: false,
    };
    ownership.observe(repository, workspace)?;
    let input = recovery_inspection::read_input(
        &ownership.directory,
        &ownership.candidate.launch,
        &ownership.candidate.admission_id,
        node,
        inspector,
    )
    .await?;
    ownership.observe(repository, workspace)?;
    Ok(Inspection { ownership, input })
}
