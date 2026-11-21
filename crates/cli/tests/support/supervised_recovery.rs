//! Full authenticated safe-boundary recovery tracer; fake immediate model only.
use super::*;
use forge_app::durable_execution::recovery::{Claim, Repository as _};
use forge_runner_host::control::ControlPipe;

pub(super) struct Trace {
    pub launch: Launch,
    pub request: RunnerRequest,
    pub directory: PathBuf,
    pub process: Process,
    pub owned: Value,
}
struct Interrupted {
    launch: Launch,
    directory: PathBuf,
    original: Value,
    events: Vec<Value>,
}

impl Trace {
    fn interrupt(
        self,
        adapter: &SupervisedDispatch,
        store: &SqliteProjectRepository,
    ) -> Interrupted {
        let Trace {
            launch,
            request,
            directory,
            mut process,
            owned,
        } = self;
        let id = &launch.assignment_id;
        let runtime = tokio::runtime::Handle::current();
        let generation = store
            .assignment_check_attempt(id)
            .unwrap()
            .unwrap()
            .generation;
        let mut service = runtime
            .block_on(adapter.authority.bind_inspected(
                store,
                "test".into(),
                launch.clone(),
                generation,
                &mut process.child,
            ))
            .unwrap();
        service.tick(store).unwrap();
        let mut pipe = service.retirement_control();
        pipe.request(&request).unwrap();
        let mut events = vec![owned];
        let original = loop {
            let event = receive(&process.output);
            let Some(kind) = event["event"].as_str() else {
                let response: RunnerResponse = serde_json::from_value(event.clone()).unwrap();
                response.validate(&request).unwrap();
                store
                    .record_result("test", &launch, &response.admission)
                    .unwrap();
                events.push(event);
                continue;
            };
            match kind {
                "authorized" => assert_eq!(event["sequence"], 1),
                "bootstrapped" => {
                    let pid = events[0]["identity"]["worker_pid"].as_u64().unwrap();
                    let address =
                        SocketAddr::from_abstract_name(format!("forge-t07-start-{pid}")).unwrap();
                    UnixStream::connect_addr(&address)
                        .unwrap()
                        .write_all(b"start\n")
                        .unwrap();
                }
                "recovery-boundary" => {
                    store
                        .record_result(
                            "test",
                            &launch,
                            &Admission::Accepted {
                                admission_id: event["admission_id"].as_str().unwrap().into(),
                            },
                        )
                        .unwrap();
                    events.push(event.clone());
                    break event;
                }
                _ => panic!("unexpected original recovery-boundary event"),
            }
            events.push(event);
        };
        drop(service); // simulate issuer loss, not a fabricated stop/hold clearance
        let worker = rustix::process::Pid::from_raw(
            i32::try_from(events[0]["identity"]["worker_pid"].as_u64().unwrap()).unwrap(),
        )
        .unwrap();
        rustix::process::kill_process(worker, rustix::process::Signal::KILL).unwrap();
        assert!(process.child.wait().unwrap().success());
        drop(pipe);
        loop {
            let line = process
                .errors
                .recv_timeout(Duration::from_secs(20))
                .unwrap();
            if line.contains("runner-crash-verified") {
                break;
            }
        }
        Interrupted {
            launch,
            directory,
            original,
            events,
        }
    }
}

impl SupervisedDispatch {
    pub(super) fn execute_recovery(
        &self,
        context: &AgentDispatchContext,
        store: &SqliteProjectRepository,
        trace: Trace,
    ) {
        let Interrupted {
            launch,
            directory,
            original,
            mut events,
        } = trace.interrupt(self, store);
        let id = &launch.assignment_id;
        let runtime = tokio::runtime::Handle::current();
        let candidate = store.recovery_candidate("test", &launch).unwrap();
        let claim = Claim::for_candidate(
            &candidate,
            "recover-safe-boundary".into(),
            "controlled-fresh-recovery-epoch".into(),
        )
        .unwrap();
        store.claim_recovery("test", &claim).unwrap();
        // The recovered process reads the SAME protected input file the first
        // admission wrote; nothing is rewritten for recovery.
        assert!(directory.join("check-runtime.json").exists());
        let mut recovered_adapter = self.clone();
        recovered_adapter.runner = self
            .runner
            .with_file_name("recovered-supervised-check-worker.mjs");
        let mut recovered =
            recovered_adapter.spawn(&directory, context.agent_workspace.as_ref().unwrap());
        let owned = recovered.event();
        assert_eq!(owned["event"], "owned");
        events.push(owned);
        let mut pipe = ControlPipe::take(&mut recovered.child).unwrap();
        let inspector = self
            .runner
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("src/inspect-recovery.ts");
        let handoff = runtime.block_on(async {
            forge_server::durable_authorization::waiting_recovery_inspection::inspect(
                store,
                "test",
                &claim,
                &mut recovered.child,
                &self.node,
                &inspector,
            )
            .await
            .unwrap()
            .inspect_progress(store, "test", &self.node, &inspector)
            .await
            .unwrap()
            .prepare(store)
            .await
            .unwrap()
        });
        let mut active = runtime.block_on(handoff.start(store, &mut pipe)).unwrap();
        loop {
            let event = receive(&recovered.output);
            match event["event"].as_str().unwrap() {
                "recovery-prepared" | "bootstrapped" => {}
                "authorized" => assert_eq!(event["sequence"], 2),
                "resumed" => {
                    assert_eq!(event["root"], original["root"]);
                    assert_eq!(event["admission_id"], original["admission_id"]);
                }
                "settled" => {
                    assert_eq!(event["status"], "done");
                    events.push(event);
                    break;
                }
                _ => panic!("unexpected recovered execution event: {event}"),
            }
            events.push(event);
            active.tick(store, &mut pipe).unwrap();
        }
        assert_eq!(
            store.assignment_check_attempt(id).unwrap().unwrap(),
            candidate.attempt
        );
        assert_eq!(
            self.observe(
                &directory,
                &launch,
                original["admission_id"].as_str().unwrap()
            ),
            "answered"
        );
        let answer = store.observe_answer("test", &launch).unwrap();
        assert_eq!(answer, AnswerObservation::OutcomeAccepted);
        active.stop(store).unwrap();
        drop(active);
        let delivery = context.capability_delivery.as_ref().unwrap();
        let secret = fs::read_to_string(&delivery.secret_file).unwrap();
        recovered.finish_redacted(&secret);
        assert!(!serde_json::to_string(&events).unwrap().contains(&secret));
        self.reports.lock().unwrap().push(Report {
            launch,
            directory,
            answer: Some(answer),
            events,
            capability_file: delivery.secret_file.clone(),
            process: None,
        });
    }
}
