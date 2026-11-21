//! Release/rollback control for NEW durable admissions only.
//!
//! The gate wraps a provider factory. It never reroutes: a durable profile that is not
//! admitted is rejected, not sent to Herdr, and `adapter_for` always passes through so an
//! existing Assignment keeps its original backend for inspection, recovery and retirement.
//! Default eligibility is withheld; nothing in production composition can enable it before
//! the retained verification gate (T19) and release handoff (T20).
use forge_app::durable_execution::launch_control::Repository as _;
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentProfile,
};
use forge_store::SqliteProjectRepository;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
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
    /// Store holding the persisted, sticky operator rollback. When set, it is read on every
    /// new-launch validation and an unreadable store refuses (fails closed).
    pub rollback_store: Option<PathBuf>,
    pub inner: Box<dyn AgentDispatchAdapterFactory>,
}

impl ReleaseGate {
    fn admit(&self) -> Result<(), AgentAdapterError> {
        if let Some(path) = &self.rollback_store {
            match SqliteProjectRepository::open(path)
                .and_then(|store| store.durable_launch_rollback())
            {
                Ok(None) => {}
                Ok(Some(_)) => {
                    // Mirror the persisted rollback so no racing enable can undo it here.
                    self.control.disable();
                }
                Err(error) => {
                    return Err(AgentAdapterError::InvalidRequest(format!(
                        "durable launch policy is unreadable ({}); new durable launches are refused",
                        error.code
                    )));
                }
            }
        }
        self.control.admit()
    }
}

fn durable(profile: &AgentProfile) -> bool {
    profile.provider == "pi-durable"
}

impl AgentDispatchAdapterFactory for ReleaseGate {
    fn validate_profile(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        if durable(profile) {
            self.admit()?;
        }
        self.inner.validate_profile(profile)
    }
    fn validate_preflight(&self, profile: &AgentProfile) -> Result<(), AgentAdapterError> {
        if durable(profile) {
            self.admit()?;
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
            rollback_store: None,
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
            rollback_store: None,
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

    #[test]
    fn persisted_rollback_refuses_across_restart_and_unreadable_policy_fails_closed() {
        use forge_app::durable_execution::launch_control::{DisableRequest, Repository};
        let root = std::env::temp_dir().join(format!(
            "forge-release-gate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("forge.sqlite3");
        drop(SqliteProjectRepository::open(&path).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = |control: LaunchControl, store: PathBuf| ReleaseGate {
            control,
            rollback_store: Some(store),
            inner: Box::new(Inner(calls.clone())),
        };
        let first = LaunchControl::default();
        assert!(first.enable_controlled());
        assert!(
            gate(first.clone(), path.clone())
                .validate_preflight(&profile("pi-durable"))
                .is_ok()
        );
        SqliteProjectRepository::open(&path)
            .unwrap()
            .disable_durable_launches(&DisableRequest {
                operation_id: "rollback-1".into(),
                reason: "incident".into(),
            })
            .unwrap();
        // The same process observes the persisted rollback and becomes sticky disabled.
        let live = gate(first.clone(), path.clone());
        assert!(live.validate_preflight(&profile("pi-durable")).is_err());
        assert_eq!(first.state(), LaunchState::Disabled);
        // "Restart": a fresh control that a verification composition tries to enable.
        let restarted = LaunchControl::default();
        assert!(restarted.enable_controlled());
        let after = gate(restarted.clone(), path.clone());
        let refused = after.validate_profile(&profile("pi-durable"));
        assert!(
            matches!(refused, Err(AgentAdapterError::InvalidRequest(m)) if m.contains("disabled"))
        );
        assert_eq!(restarted.state(), LaunchState::Disabled);
        // Original-backend operations and Herdr remain available.
        assert!(after.adapter_for(&profile("pi-durable")).is_ok());
        assert!(after.validate_profile(&profile("herdr")).is_ok());
        // An unreadable policy store refuses new durable launches only.
        let missing = root.join("not-a-dir").join("missing").join("forge.sqlite3");
        std::fs::write(root.join("not-a-dir"), b"file").unwrap();
        let control = LaunchControl::default();
        assert!(control.enable_controlled());
        let broken = gate(control, missing);
        let refused = broken.validate_preflight(&profile("pi-durable"));
        assert!(
            matches!(refused, Err(AgentAdapterError::InvalidRequest(m)) if m.contains("unreadable"))
        );
        assert!(broken.validate_preflight(&profile("herdr")).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }
}
