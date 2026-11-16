// Exercise application diagnostics with isolated persistence and no provider processes.
struct DiagnosticLaunchAdapter {
    record_launch: bool,
    error: Option<&'static str>,
}

impl forge_app::AgentDispatchAdapter for DiagnosticLaunchAdapter {
    fn probe(&self) -> Result<(), forge_app::AgentAdapterError> {
        Ok(())
    }

    fn provision(
        &self,
        context: &forge_app::AgentDispatchContext,
        repository: &dyn AgentDispatchRepository,
    ) -> Result<(), forge_app::AgentAdapterError> {
        if self.record_launch {
            assert!(
                repository
                    .begin_agent_launch(&context.assignment.id, 1_000)
                    .unwrap()
            );
        }
        match self.error {
            Some(error) => Err(forge_app::AgentAdapterError::Endpoint(error.into())),
            None => Ok(()),
        }
    }

    fn observe(
        &self,
        _: &forge_app::AgentDispatchContext,
    ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
        unreachable!("provisioning must not observe or retry the launch")
    }

    fn recover(
        &self,
        _: &forge_app::AgentDispatchContext,
    ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
        unreachable!("diagnostic logging must not trigger recovery")
    }

    fn retire(
        &self,
        _: &forge_app::AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), forge_app::AgentAdapterError> {
        unreachable!("diagnostic logging must not release ownership")
    }
}

fn capture_provisioning_diagnostic(record_launch: bool, error: Option<&'static str>) -> String {
    let store = SqliteProjectRepository::in_memory().unwrap();
    super::test_projects::register(&store, "alpha", None);
    let record = store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Grilling,
            body: "Provisioning diagnostic".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let assignment = store
        .create_agent_assignment(&NewAgentAssignment {
            id: "diagnostic-launch".into(),
            record_id: record.id,
            project_slug: "alpha".into(),
            profile_revision: 1,
        })
        .unwrap();
    let logs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer = AssignmentLogWriter(logs.clone());
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(move || writer.clone())
        .finish();
    let outcome = tracing::subscriber::with_default(subscriber, || {
        forge_app::reconcile_agent_assignment(
            &store,
            &DiagnosticLaunchAdapter {
                record_launch,
                error,
            },
            &assignment,
        )
        .unwrap()
    });
    let expected = if record_launch || error.is_none() {
        forge_app::AssignmentReconciliation::AwaitingAcknowledgement
    } else {
        forge_app::AssignmentReconciliation::Retained(forge_app::AdapterRetention::Endpoint)
    };
    assert_eq!(outcome, expected);
    assert_eq!(store.agent_assignment(&assignment.id).unwrap(), assignment);
    assert_eq!(store.list_open_agent_assignments().unwrap().len(), 1);
    assert_eq!(
        store.agent_launch_intent(&assignment.id).unwrap().is_some(),
        record_launch
    );
    assert!(store.dispatch_hold(record.id).unwrap().is_none());
    if record_launch {
        assert!(!store.begin_agent_launch(&assignment.id, 2_000).unwrap());
    }
    String::from_utf8(logs.lock().unwrap().clone()).unwrap()
}

#[test]
fn provisioning_failures_are_logged_once_without_releasing_launch_ownership() {
    for record_launch in [false, true] {
        let logs = capture_provisioning_diagnostic(
            record_launch,
            Some("Herdr agent.start: agent_pane_unavailable"),
        );
        assert_eq!(
            logs.matches("agent dispatch adapter failed").count(),
            1,
            "{logs}"
        );
        assert!(logs.contains("operation=\"provision\""), "{logs}");
        assert!(
            logs.contains("Herdr agent.start: agent_pane_unavailable"),
            "{logs}"
        );
    }
    assert!(capture_provisioning_diagnostic(true, None).is_empty());
}

#[test]
fn provisioning_failures_with_launch_intent_redact_unreviewed_details() {
    for message in [
        "Herdr agent.start: agent_pane_unavailable PRIVATE-CAPABILITY PRIVATE-PATH PROFILE-BEHAVIOR",
        "PRIVATE-CAPABILITY PRIVATE-PATH PROFILE-BEHAVIOR",
    ] {
        let logs = capture_provisioning_diagnostic(true, Some(message));
        assert_eq!(
            logs.matches("agent dispatch adapter failed").count(),
            1,
            "{logs}"
        );
        assert!(
            logs.contains("details withheld; adapter message is not allowlisted"),
            "{logs}"
        );
        for secret in ["PRIVATE-CAPABILITY", "PRIVATE-PATH", "PROFILE-BEHAVIOR"] {
            assert!(!logs.contains(secret), "{logs}");
        }
    }
}
