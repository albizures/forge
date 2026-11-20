//! Claim-bound predecessor/runtime proof under one retained exclusive lock.
//! A read-only recovery-host component, not waiting-owner handoff or authority.
use super::{recovery_inspection, runtime_directory};
use forge_app::{
    RegistrationError,
    durable_execution::recovery::{Candidate, Claim, Repository as _, quiescence::Operation},
};
use forge_rpc::durable_execution::recovery_inspection::PendingInput;
use forge_store::SqliteProjectRepository;
use std::{path::Path, path::PathBuf};

fn denied() -> RegistrationError {
    RegistrationError::new(
        "durable-quiesced-inspection-unavailable",
        "claim-bound predecessor/runtime proof unavailable; preserve evidence and capacity; no recovery is authorized",
    )
}

/// Holds canonical exclusive ownership from committed runtime inspection through
/// dependent verification. No external proof is accepted by digest/PID alone.
/// This does not validate Git freshness, create a waiting owner or grant execution.
#[must_use = "retain exclusive inspection through all dependent operations"]
pub struct Inspection {
    runtime: recovery_inspection::Inspection,
    directory: PathBuf,
    candidate: Candidate,
    operation: Operation,
}

impl Inspection {
    #[must_use]
    pub fn candidate(&self) -> &Candidate {
        &self.candidate
    }

    #[must_use]
    pub fn input(&self) -> &PendingInput {
        self.runtime.input()
    }

    /// Recheck protected canonical identity/receipt and the current claimed Forge
    /// binding. This never replays claim acceptance or adopts historical grants.
    /// External writers must still be excluded by the trusted local host; this
    /// is not a substitute for transactional grant checks or process containment.
    /// # Errors
    /// Rejects changed/closed/blocked state, cancelled claims and lost proof.
    pub fn verify(
        &self,
        repository: &SqliteProjectRepository,
        workspace: &str,
    ) -> Result<(), RegistrationError> {
        self.runtime.verify()?;
        if runtime_directory::mapped(repository, workspace, &self.candidate.launch, false)?
            != self.directory
        {
            return Err(denied());
        }
        forge_runner_host::quiescence::waiting_receipt(&self.directory, &self.operation)
            .map_err(|_| denied())?;
        if repository.claimed_recovery_candidate(workspace, &self.operation.claim)?
            != self.candidate
        {
            return Err(denied());
        }
        self.runtime.verify()
    }
}

/// Compose exact active-claim inspection, original accepted pending input and the
/// original subreaper's protected descendant-absence receipt without a lock gap.
/// Source profiles/provider credentials are not historical proof prerequisites.
/// No host is launched, receipt repaired, claim consumed or grant issued. Failures
/// preserve all bytes and Forge state; callers retain any additional diagnostics.
/// # Errors
/// Rejects unavailable/unsafe ownership, foreign/stale claims, missing/conflicting
/// predecessor proof, settled input, interrupted effects and changed live binding.
pub async fn inspect(
    repository: &SqliteProjectRepository,
    workspace: &str,
    claim: &Claim,
    node: &Path,
    inspector: &Path,
) -> Result<Inspection, RegistrationError> {
    let candidate = repository.claimed_recovery_candidate(workspace, claim)?;
    let operation = Operation::for_candidate(&candidate, claim.clone())?;
    let directory = runtime_directory::mapped(repository, workspace, &candidate.launch, false)?;
    // This read-only component cannot admit work even when process safety is
    // unavailable. Proof is accepted only after BOTH inspections succeed under
    // the same lock; the guard is never dropped/reacquired between them.
    let runtime = recovery_inspection::inspect_exclusive(
        repository,
        workspace,
        &candidate.launch,
        &candidate.admission_id,
        node,
        inspector,
    )
    .await?;
    let inspection = Inspection {
        runtime,
        directory,
        candidate,
        operation,
    };
    inspection.verify(repository, workspace)?;
    Ok(inspection)
}
