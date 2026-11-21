//! Release/rollback control for NEW durable admissions only.
//!
//! The gate wraps a provider factory. It never reroutes: a durable profile that is not
//! admitted is rejected, not sent to Herdr, and `adapter_for` always passes through so an
//! existing Assignment keeps its original backend for inspection, recovery and retirement.
//! Default eligibility is withheld; nothing in production composition can enable it before
//! the retained verification gate (T19) and release handoff (T20).
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentProfile,
};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

const WITHHELD: u8 = 0;
const CONTROLLED: u8 = 1;
const DISABLED: u8 = 2;

/// Shared launch-eligibility state. Disabling is sticky within a process so a rollback
/// cannot be undone by a racing enable.
#[derive(Clone, Debug, Default)]
pub struct LaunchControl(Arc<AtomicU8>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchState {
    /// Default: release evidence is not accepted, so new durable launches are refused.
    Withheld,
    /// Explicit controlled eligibility (tests/release verification only).
    Controlled,
    /// Rollback: new durable launches are refused; existing work is unaffected.
    Disabled,
}

impl LaunchControl {
    #[must_use]
    pub fn state(&self) -> LaunchState {
        match self.0.load(Ordering::SeqCst) {
            CONTROLLED => LaunchState::Controlled,
            DISABLED => LaunchState::Disabled,
            _ => LaunchState::Withheld,
        }
    }

    /// Grants controlled eligibility unless rolled back. Returns whether it is now enabled.
    #[must_use]
    pub fn enable_controlled(&self) -> bool {
        self.0
            .compare_exchange(WITHHELD, CONTROLLED, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
            || self.state() == LaunchState::Controlled
    }

    /// Rollback: refuse new durable launches. Idempotent and irreversible for this control.
    pub fn disable(&self) {
        self.0.store(DISABLED, Ordering::SeqCst);
    }

    fn admit(&self) -> Result<(), AgentAdapterError> {
        match self.state() {
            LaunchState::Controlled => Ok(()),
            LaunchState::Withheld => Err(AgentAdapterError::InvalidRequest(
                "durable launch eligibility is withheld pending release verification".into(),
            )),
            LaunchState::Disabled => Err(AgentAdapterError::InvalidRequest(
                "new durable launches are disabled; existing Assignments are unaffected".into(),
            )),
        }
    }
}

pub struct ReleaseGate {
    pub control: LaunchControl,
    pub inner: Box<dyn AgentDispatchAdapterFactory>,
}

fn durable(profile: &AgentProfile) -> bool {
    profile.provider == "pi-durable"
}

impl AgentDispatchAdapterFactory for ReleaseGate {
    fn validate_profile(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        if durable(profile) {
            self.control.admit()?;
        }
        self.inner.validate_profile(profile)
    }
    fn validate_preflight(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        if durable(profile) {
            self.control.admit()?;
        }
        self.inner.validate_preflight(profile)
    }
    fn adapter_for(
        &self,
        profile: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        // Deliberately ungated: original-backend ownership outlives any launch policy.
        self.inner.adapter_for(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_app::{AgentDispatchContext, AgentDispatchRepository, AgentObservation};
    use forge_store::SqliteProjectRepository;
    use std::sync::atomic::AtomicUsize;

    struct Inner(Arc<AtomicUsize>);
    struct Adapter;
    impl AgentDispatchAdapterFactory for Inner {
        fn validate_profile(&self, _: &AgentProfile) -> Result<(), AgentAdapterError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn adapter_for(
            &self,
            _: &AgentProfile,
        ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
            Ok(Box::new(Adapter))
        }
    }
    impl AgentDispatchAdapter for Adapter {
        fn probe(&self) -> Result<(), AgentAdapterError> {
            Ok(())
        }
        fn provision(
            &self,
            _: &AgentDispatchContext,
            _: &dyn AgentDispatchRepository,
        ) -> Result<(), AgentAdapterError> {
            Ok(())
        }
        fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
            Ok(AgentObservation::Working)
        }
        fn recover(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
            Ok(AgentObservation::Working)
        }
        fn retire(
            &self,
            _: &AgentDispatchContext,
            _: &dyn AgentDispatchRepository,
        ) -> Result<(), AgentAdapterError> {
            Ok(())
        }
    }

    fn profile(provider: &str) -> AgentProfile {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let mut profile = store.agent_profile(forge_app::RecordType::Task, 1).unwrap();
        profile.provider = provider.into();
        profile
    }

    #[test]
    fn durable_eligibility_is_withheld_by_default_without_touching_other_backends() {
        let calls = Arc::new(AtomicUsize::new(0));
        let control = LaunchControl::default();
        let gate = ReleaseGate {
            control: control.clone(),
            inner: Box::new(Inner(calls.clone())),
        };
        assert_eq!(control.state(), LaunchState::Withheld);
        assert!(gate.validate_profile(&profile("pi-durable")).is_err());
        assert!(gate.validate_preflight(&profile("pi-durable")).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0, "inner never consulted");
        assert!(gate.validate_profile(&profile("herdr")).is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // Existing work keeps its adapter even while eligibility is withheld.
        assert!(gate.adapter_for(&profile("pi-durable")).is_ok());
    }

    #[test]
    fn rollback_refuses_new_launches_but_keeps_existing_assignment_operations() {
        let calls = Arc::new(AtomicUsize::new(0));
        let control = LaunchControl::default();
        let gate = ReleaseGate {
            control: control.clone(),
            inner: Box::new(Inner(calls.clone())),
        };
        assert!(control.enable_controlled());
        assert!(gate.validate_profile(&profile("pi-durable")).is_ok());
        control.disable();
        control.disable();
        assert!(!control.enable_controlled(), "rollback is sticky");
        assert_eq!(control.state(), LaunchState::Disabled);
        let rejected = gate.validate_preflight(&profile("pi-durable"));
        assert!(
            matches!(rejected, Err(AgentAdapterError::InvalidRequest(m)) if m.contains("disabled"))
        );
        // The same durable backend still serves recovery/retirement; no Herdr fallback exists.
        let adapter = gate.adapter_for(&profile("pi-durable")).unwrap();
        assert!(adapter.probe().is_ok());
        // Herdr remains available throughout.
        assert!(gate.validate_profile(&profile("herdr")).is_ok());
    }
}
