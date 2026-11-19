//! Test-only durable transport boundary. Application dispatch and Git preparation
//! are real; the caller drives the captured launch through the component runner.
//! This deliberately supplies neither a supervisor nor an authorization grant.
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, DispatchWorkspace,
};
use forge_server::durable_profiles::{PreparedProfile, prepare};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub struct CapturedLaunch {
    pub context: AgentDispatchContext,
    pub profile: PreparedProfile,
}

#[derive(Clone)]
pub struct DurableDispatch {
    pub credential: PathBuf,
    pub launches: Arc<Mutex<Vec<CapturedLaunch>>>,
}

impl AgentDispatchAdapterFactory for DurableDispatch {
    fn validate_profile(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        assert_eq!(profile.provider, "pi-durable");
        assert_eq!(profile.record_type, forge_app::RecordType::Task);
        forge_server::durable_profiles::prepare(
            profile,
            self.credential.parent().unwrap(),
            Some(&self.credential),
        )
        .map(|_| ())
        .map_err(|error| AgentAdapterError::InvalidRequest(error.code.into()))
    }

    fn adapter_for(
        &self,
        profile: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        self.validate_profile(profile)?;
        Ok(Box::new(self.clone()))
    }
}

impl AgentDispatchAdapter for DurableDispatch {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        Ok(())
    }

    fn verify_workspace(&self, _: &str) -> Result<DispatchWorkspace, AgentAdapterError> {
        panic!("durable dispatch must not invoke Herdr")
    }

    fn validate_provision(&self, context: &AgentDispatchContext) -> Result<(), AgentAdapterError> {
        let cwd = context
            .agent_workspace
            .as_ref()
            .expect("bound Managed Worktree");
        prepare(&context.assignment.profile, cwd, Some(&self.credential))
            .map(|_| ())
            .map_err(|error| AgentAdapterError::InvalidRequest(error.code.into()))
    }

    fn provision(
        &self,
        context: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        assert!(context.dispatch_workspace.is_none());
        let cwd = context
            .agent_workspace
            .as_ref()
            .expect("bound Managed Worktree");
        let profile = prepare(&context.assignment.profile, cwd, Some(&self.credential))
            .map_err(|error| AgentAdapterError::InvalidRequest(error.code.into()))?;
        self.launches.lock().unwrap().push(CapturedLaunch {
            context: context.clone(),
            profile,
        });
        Ok(())
    }

    fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        Ok(AgentObservation::Working)
    }

    fn recover(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        panic!("automatic recovery remains disabled")
    }

    fn retire(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        panic!("component tracer does not prove retirement safety")
    }
}
