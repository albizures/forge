//! Real application -> store retirement, with a deliberately non-verifying adapter.
use super::*;
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentObservation, AgentProfile, Application, RepositoryInspector,
};
use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

struct NoopFactory(Arc<AtomicUsize>);
struct NoopAdapter(Arc<AtomicUsize>);
impl AgentDispatchAdapterFactory for NoopFactory {
    fn validate_preflight(&self, _: &AgentProfile) -> Result<(), AgentAdapterError> {
        panic!("retirement must not load original profile sources or issue credentials")
    }
    fn adapter_for(
        &self,
        profile: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        assert_eq!(profile.provider, "pi-durable", "retain original backend");
        Ok(Box::new(NoopAdapter(self.0.clone())))
    }
}
impl AgentDispatchAdapter for NoopAdapter {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        unreachable!()
    }
    fn provision(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        unreachable!()
    }
    fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        unreachable!()
    }
    fn recover(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        unreachable!()
    }
    fn retire(
        &self,
        context: &AgentDispatchContext,
        repository: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        assert!(context.assignment.closed_at_millis.is_some());
        assert!(
            repository
                .agent_assignment(&context.assignment.id)
                .unwrap()
                .closed_at_millis
                .is_some(),
            "durable revocation precedes cleanup"
        );
        assert!(context.capability_delivery.is_none());
        self.0.fetch_add(1, Ordering::SeqCst);
        // Cancellation/abort admission without verified cleanup is not absence.
        Ok(())
    }
}
struct UnusedInspector;
impl RepositoryInspector for UnusedInspector {
    fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
        unreachable!("retirement cannot inspect or repair Git")
    }
}

#[test]
fn successful_adapter_reply_cannot_finalize_unreconciled_durable_retirement() {
    let f = Fixture::new();
    accept_runtime(&f);
    let calls = Arc::new(AtomicUsize::new(0));
    let app = || {
        Application::with_agent_dispatch(
            Box::new(f.reopen()),
            Box::new(f.reopen()),
            Box::new(NoopFactory(calls.clone())),
            Box::new(UnusedInspector),
        )
    };
    for attempt in 1..=2 {
        assert_eq!(
            app()
                .retire_agent_assignment("assignment")
                .unwrap_err()
                .code,
            "assignment-retirement-unreconciled"
        );
        let info = f.reopen().agent_assignment_info("assignment").unwrap();
        assert!(info.assignment.closed_at_millis.is_some());
        assert!(info.capacity_reserved());
        assert!(info.dispatch_hold.is_some());
        assert!(
            info.resources
                .iter()
                .any(|r| r.resource_kind == "retirement-pending" && r.retired_at_millis.is_none())
        );
        assert_eq!(calls.load(Ordering::SeqCst), attempt);
    }
}
