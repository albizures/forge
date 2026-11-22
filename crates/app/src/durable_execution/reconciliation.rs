//! Same-Assignment recovery is separate from accepted Check handoff/successors.
use crate::{Application, RegistrationError};

impl Application {
    pub(crate) fn reconcile_durable_check_execution(&self) -> Result<(), RegistrationError> {
        let repository = self.dispatch()?;
        for assignment in repository.list_open_agent_assignments()? {
            if assignment.profile.provider != "pi-durable"
                || repository
                    .assignment_check_attempt(&assignment.id)?
                    .is_none()
            {
                continue;
            }
            let context = crate::agent_dispatch_context(repository, &assignment)?;
            let result = self
                .adapter_factory()?
                .adapter_for(&assignment.profile)
                .and_then(|adapter| adapter.recover(&context));
            // The specialized repository/adapter owns current authorization,
            // host proof, retained failures and grant issuance. No observation
            // creates an Assessment, changes attempt/context or starts a successor.
            // Existing acceptance/retirement policy remains the only handoff path.
            if result.is_err() {
                tracing::warn!(assignment_id = %assignment.id, code = "durable-recovery-blocked", "same-Assignment recovery unavailable; original resources retained");
            }
        }
        Ok(())
    }
}
