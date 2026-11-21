//! Fresh protected proof -> exact unarmed preparation -> committed grant delivery.
use super::{GitInspector, Inspection, denied};
use forge_app::{
    RegistrationError,
    authorization_lease::{
        AuthorityHost, Observation,
        session::{Failure, SessionRepository, Tick},
    },
    durable_execution::recovery::{
        Candidate, Claim,
        authorization::{Prepared, RecoveryHost, Verification},
        session::Session,
        worktree::Inspector as _,
    },
};
use forge_runner_host::{control::ControlPipe, recovery_proof};
use forge_store::SqliteProjectRepository;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

// A one-use trusted boundary result, never public/deserializable authority.
struct Verified(Option<Verification>);
impl RecoveryHost for Verified {
    fn verify(&mut self, _: &Candidate, _: &Claim) -> Result<Verification, RegistrationError> {
        self.0.take().ok_or_else(denied)
    }
}

struct Host<'a> {
    inspection: Inspection<'a>,
    key: String,
    proof: Value,
    active: bool,
    fenced: bool,
}
impl AuthorityHost for Host<'_> {
    fn observe(&mut self) -> Result<Observation, RegistrationError> {
        if self.fenced {
            return Err(denied());
        }
        self.fenced = true;
        let directory = &self.inspection.waiting.ownership.directory;
        recovery_proof::verify(directory, &self.key, &self.proof).map_err(|_| denied())?;
        // During commit/delivery, keep the exact original repository observation.
        // Once active, legitimate new Implementation progress must not be mistaken
        // for a changed pre-grant snapshot. Ordinary lease transactions own policy.
        if !self.active
            && GitInspector.observe(self.inspection.progress.binding())?
                != *self.inspection.progress.observation()
        {
            return Err(denied());
        }
        let observation = AuthorityHost::observe(&mut self.inspection.waiting)?;
        self.fenced = false;
        Ok(observation)
    }
}

/// Owns the original waiting Child borrow through proof, grant and delivery.
/// Dropping this neither cleans descendants nor releases the retained claim.
#[must_use]
pub struct Handoff<'a> {
    host: Host<'a>,
    prepared: Prepared,
}

/// Active same-Assignment authorization driver. No PID adoption or grant replay.
#[must_use]
pub struct Active<'a> {
    host: Host<'a>,
    session: Session,
}
impl Active<'_> {
    /// # Errors
    /// Renewal/owner/clock/transport failures terminally fence and retain capacity.
    pub fn tick(
        &mut self,
        repository: &SqliteProjectRepository,
        pipe: &mut ControlPipe,
    ) -> Result<Tick, RegistrationError> {
        self.session.tick(repository, &mut self.host, pipe)
    }
    /// # Errors
    /// Diagnostic persistence failure does not restore execution permission.
    pub fn stop(&mut self, repository: &SqliteProjectRepository) -> Result<(), RegistrationError> {
        self.session.stop(repository)
    }
}

impl<'a> Inspection<'a> {
    /// Persist bounded protected evidence only after all combined checks; recheck
    /// afterward. A retained file/digest cannot reconstruct this in-memory value.
    /// # Errors
    /// Unknown/conflicting proof retains original ownership, claim and capacity.
    pub async fn prepare(
        mut self,
        repository: &SqliteProjectRepository,
    ) -> Result<Handoff<'a>, RegistrationError> {
        let observed = self.verify(repository).await?;
        let operation = &self.waiting.ownership.operation;
        let predecessor_proof = forge_runner_host::quiescence::waiting_receipt(
            &self.waiting.ownership.directory,
            operation,
        )
        .map_err(|_| denied())?;
        let proof = json!({"version": 1, "predecessor_proof": predecessor_proof, "claim": operation.claim, "predecessor": operation.predecessor,
            "waiting": {"owner": observed.owner, "boottime_millis": observed.boottime_millis}, "input": self.input(), "execution_directory": self.candidate().execution_directory,
            "progress": self.progress.observation(), "base_commit": self.progress.binding().base_commit,
            "check_name": self.progress.binding().check_name});
        let key = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&operation.claim).map_err(|_| denied())?)
        );
        let directory = &self.waiting.ownership.directory;
        recovery_proof::persist(directory, &key, &proof).map_err(|_| denied())?;
        let observation = self.verify(repository).await?;
        recovery_proof::verify(&self.waiting.ownership.directory, &key, &proof)
            .map_err(|_| denied())?;
        let digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&proof).map_err(|_| denied())?)
        );
        let prepared = Prepared::verify(
            self.candidate().clone(),
            self.waiting.ownership.operation.claim.clone(),
            &mut Verified(Some(Verification {
                observation,
                evidence_sha256: digest,
            })),
        )?;
        Ok(Handoff {
            host: Host {
                inspection: self,
                key,
                proof,
                active: false,
                fenced: false,
            },
            prepared,
        })
    }
}
impl<'a> Handoff<'a> {
    #[must_use]
    pub fn prepared(&self) -> &Prepared {
        &self.prepared
    }

    /// Ordered private preparation grants no authority. Recheck runtime/effects
    /// afterward, then atomically commit before delivering the exact envelope.
    /// Both executable consumers independently validate preparation and delivery.
    /// # Errors
    /// No uncertain preparation/commit/delivery is replayed or adopted.
    pub async fn start(
        mut self,
        repository: &SqliteProjectRepository,
        pipe: &mut ControlPipe,
    ) -> Result<Active<'a>, RegistrationError> {
        let workspace = self.host.inspection.workspace.clone();
        let launch = self.prepared.candidate().launch.clone();
        if pipe.prepare_recovery(&self.prepared).is_err()
            || self.host.inspection.verify(repository).await.is_err()
        {
            repository.retain_failure(&workspace, &launch, Failure::AuthorityUnavailable)?;
            return Err(denied());
        }
        let mut session = Session::new(workspace, self.prepared)?;
        session.tick(repository, &mut self.host, pipe)?;
        self.host.active = true;
        Ok(Active {
            host: self.host,
            session,
        })
    }
}
