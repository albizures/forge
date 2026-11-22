//! Combined read-only proof under the directly spawned waiting owner's lock.
//! Protected proof and grant handoff are provided by the child handoff module.
use super::{Inspection as Waiting, denied, recovery_inspection};
use crate::durable_authorization::recovery_worktree::GitInspector;
use forge_app::{
    RegistrationError,
    authorization_lease::Observation,
    durable_execution::recovery::{Candidate, worktree},
};
use forge_rpc::durable_execution::recovery_inspection::PendingInput;
use forge_store::SqliteProjectRepository;
use std::path::{Path, PathBuf};

pub mod handoff;

/// Holds the direct Child borrow, not an independently acquired lock descriptor.
/// This type does not implement `AuthorityHost`: claimed reads cease to be valid
/// after a grant consumes the claim. The protected handoff module explicitly
/// bridges that transition rather than reusing this pre-grant verifier for renewal.
#[must_use = "retain waiting ownership through all dependent verification"]
pub struct Inspection<'a> {
    waiting: Waiting<'a>,
    progress: worktree::Inspection,
    workspace: String,
    node: PathBuf,
    inspector: PathBuf,
    fenced: bool,
}

impl Inspection<'_> {
    #[must_use]
    pub fn candidate(&self) -> &Candidate {
        self.waiting.candidate()
    }

    #[must_use]
    pub fn input(&self) -> &PendingInput {
        self.waiting.input()
    }

    /// Repository progress remains an observation, never adopted Check evidence.
    #[must_use]
    pub fn progress(&self) -> &worktree::Inspection {
        &self.progress
    }

    /// Recheck the owner/receipt and active claim around fresh bounded committed
    /// runtime/effect inspection and canonical Git/content/index observations.
    /// Any failed or cancelled verification permanently fences this object, even
    /// if another writer subsequently restores the original bytes or authority.
    /// # Errors
    /// Unknown, changed or unavailable proof never authorizes recovery.
    pub async fn verify(
        &mut self,
        repository: &SqliteProjectRepository,
    ) -> Result<Observation, RegistrationError> {
        if self.fenced {
            return Err(denied());
        }
        self.fenced = true;
        let before = self.waiting.observe(repository, &self.workspace)?;
        self.progress.verify(repository, &GitInspector)?;
        let input = recovery_inspection::read_input(
            &self.waiting.ownership.directory,
            &self.waiting.candidate().launch,
            &self.waiting.candidate().admission_id,
            &self.node,
            &self.inspector,
        )
        .await?;
        if input != *self.waiting.input() {
            return Err(denied());
        }
        self.progress.verify(repository, &GitInspector)?;
        let after = self.waiting.observe(repository, &self.workspace)?;
        if before.owner != after.owner || after.boottime_millis < before.boottime_millis {
            return Err(denied());
        }
        self.fenced = false;
        Ok(after)
    }
}

pub(super) async fn inspect<'a>(
    mut waiting: Waiting<'a>,
    repository: &SqliteProjectRepository,
    workspace: &str,
    node: &Path,
    inspector: &Path,
) -> Result<Inspection<'a>, RegistrationError> {
    waiting.observe(repository, workspace)?;
    let progress = worktree::inspect(
        repository,
        &GitInspector,
        workspace,
        &waiting.ownership.operation.claim,
    )?;
    if progress.binding().candidate != *waiting.candidate() {
        return Err(denied());
    }
    let mut inspection = Inspection {
        waiting,
        progress,
        workspace: workspace.into(),
        node: node.into(),
        inspector: inspector.into(),
        fenced: false,
    };
    inspection.verify(repository).await?;
    Ok(inspection)
}
