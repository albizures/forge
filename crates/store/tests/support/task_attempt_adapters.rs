use forge_app::*;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
pub struct Local;
impl RepositoryInspector for Local {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        unreachable!()
    }
    fn head_commit(&self, _: &Path) -> Result<String, RegistrationError> {
        Ok("a".repeat(40))
    }
}
impl ManagedWorktreeAdapter for Local {
    fn create(&self, _: &Path, _: &Path, _: &str, _: &str) -> Result<(), RegistrationError> {
        unreachable!()
    }
    fn remove(&self, _: &Path, _: bool) -> Result<(), RegistrationError> {
        unreachable!()
    }
    fn assess(&self, _: &Path) -> WorktreeSafetyAssessment {
        WorktreeSafetyAssessment {
            facts: vec![],
            blockers: vec![],
            removable: true,
        }
    }
    fn run_setup_hook(
        &self,
        _: &Path,
        _: &str,
        _: &str,
        _: u64,
        _: &str,
        _: &str,
    ) -> Result<(), RegistrationError> {
        unreachable!()
    }
}
pub struct Provider {
    pub unavailable: Arc<AtomicBool>,
    pub launches: Arc<Mutex<Vec<u32>>>,
}
impl AgentDispatchAdapterFactory for Provider {
    fn validate_profile(&self, _: &AgentProfile) -> Result<(), AgentAdapterError> {
        if self.unavailable.load(Ordering::SeqCst) {
            Err(AgentAdapterError::InvalidRequest(
                "missing pinned prerequisite".into(),
            ))
        } else {
            Ok(())
        }
    }
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        Ok(Box::new(Adapter(self.launches.clone())))
    }
}
struct Adapter(Arc<Mutex<Vec<u32>>>);
impl AgentDispatchAdapter for Adapter {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        Ok(())
    }
    fn verify_workspace(&self, id: &str) -> Result<DispatchWorkspace, AgentAdapterError> {
        Ok(DispatchWorkspace { id: id.into() })
    }
    fn provision(
        &self,
        context: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        self.0
            .lock()
            .unwrap()
            .push(context.assignment.profile.revision);
        Ok(())
    }
    fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        Ok(AgentObservation::Working)
    }
    fn recover(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        unreachable!()
    }
    fn retire(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        Ok(())
    }
}
