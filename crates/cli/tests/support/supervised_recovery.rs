//! Same-attempt recovery of a production-launched Assignment through the
//! production recovery controller. The only fault is test-only: the worker stops
//! forever before its first generation commit, and the test kills it there.
use super::*;

impl SupervisedDispatch {
    pub(super) fn recover(
        &self,
        store: &SqliteProjectRepository,
        launch: Launch,
        directory: PathBuf,
        capability_file: PathBuf,
    ) -> Report {
        let id = launch.assignment_id.clone();
        let boundary = directory.join("controlled-recovery-boundary.json");
        let original: Value = wait(|| {
            fs::read(&boundary)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        });
        let admission = original["admission_id"].as_str().unwrap().to_owned();
        // The production driver committed the runner's correlated admission reply.
        wait(|| {
            matches!(
                store.runtime("test", &id).unwrap().unwrap().admission,
                forge_app::durable_execution::Admission::Accepted { ref admission_id }
                    if *admission_id == admission
            )
            .then_some(())
        });
        let attempt = store.assignment_check_attempt(&id).unwrap().unwrap();
        // Crash the worker at the injected boundary. The original host verifies
        // the crash and exits; the production driver observes the closed stream.
        let worker = owner(&directory)["worker_pid"].as_u64().unwrap();
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(i32::try_from(worker).unwrap()).unwrap(),
            rustix::process::Signal::KILL,
        )
        .unwrap();
        self.wait_state(&id, |state| state == DriverState::Crashed);
        // The recovered process reads the SAME protected input the first
        // admission wrote; nothing is rewritten for recovery.
        let input = fs::read(directory.join("check-runtime.json")).unwrap();
        // Ordinary daemon reconciliation drives production recovery from the
        // original host's exact crash proof; no test code claims or hands off.
        let app = self.app();
        let now = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        app.reconcile_agent_dispatch("supervised-tracer", now)
            .unwrap();
        let recovery = self.production.recovery.as_ref().unwrap();
        wait(|| {
            (recovery.recover("test", &launch).ok() == Some(AgentObservation::Idle)).then_some(())
        });
        assert_eq!(
            fs::read(directory.join("check-runtime.json")).unwrap(),
            input
        );
        assert_eq!(
            store.assignment_check_attempt(&id).unwrap().unwrap(),
            attempt
        );
        assert_eq!(store.runtime("test", &id).unwrap().unwrap().launch, launch);
        assert_eq!(self.observe(&directory, &launch, &admission), "answered");
        let answer = store.observe_answer("test", &launch).unwrap();
        assert_eq!(answer, AnswerObservation::OutcomeAccepted);
        Report {
            launch,
            directory,
            answer: Some(answer),
            capability_file,
        }
    }
}
