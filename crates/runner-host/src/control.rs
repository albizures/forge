//! Nonblocking delivery to a directly spawned supervisor's private input pipe.
use forge_app::{
    RegistrationError,
    authorization_lease::{Lease, session::Delivery},
};
use forge_rpc::durable_execution::{SUPERVISOR_CONTROL_VERSION, SupervisorControl};
use std::{
    io::Write,
    process::{Child, ChildStdin},
    sync::{Arc, Mutex},
};

fn uncertain() -> RegistrationError {
    RegistrationError::new(
        "authorization-delivery-uncertain",
        "supervisor control delivery is uncertain; do not replay or release capacity",
    )
}

/// Owns the sole taken stdin writer. It cannot be constructed from a path, PID,
/// retained file or agent payload. The server must still verify live ownership
/// and persist current authority before using the application delivery port.
#[derive(Clone)]
pub struct ControlPipe {
    input: Arc<Mutex<Pipe<ChildStdin>>>,
}

// A partial record corrupts framing for the whole channel, including clones and
// other control kinds. No caller may append bytes after any uncertain send.
struct Pipe<W> {
    writer: W,
    fenced: bool,
}

impl<W: Write> Pipe<W> {
    fn send(
        &mut self,
        control: &impl serde::Serialize,
        limit: usize,
    ) -> Result<(), RegistrationError> {
        if self.fenced {
            return Err(uncertain());
        }
        self.fenced = true;
        let mut bytes = serde_json::to_vec(control).map_err(|_| uncertain())?;
        bytes.push(b'\n');
        if bytes.len() > limit {
            return Err(uncertain());
        }
        // Exactly one nonblocking write. Never retry EINTR, a partial record or
        // a full pipe, and never flush a possibly buffered transport.
        match self.writer.write(&bytes) {
            Ok(count) if count == bytes.len() => {
                self.fenced = false;
                Ok(())
            }
            _ => Err(uncertain()),
        }
    }
}

impl ControlPipe {
    /// Take before binding the borrowing host-evidence adapter to the same Child.
    /// # Errors
    /// Fails closed if the pipe is absent or cannot be made nonblocking.
    pub fn take(supervisor: &mut Child) -> Result<Self, RegistrationError> {
        let input = supervisor.stdin.take().ok_or_else(uncertain)?;
        let flags = rustix::fs::fcntl_getfl(&input).map_err(|_| uncertain())?;
        rustix::fs::fcntl_setfl(&input, flags | rustix::fs::OFlags::NONBLOCK)
            .map_err(|_| uncertain())?;
        Ok(Self {
            input: Arc::new(Mutex::new(Pipe {
                writer: input,
                fenced: false,
            })),
        })
    }

    /// Configure only an unarmed expectation from opaque fresh host verification.
    /// This delivers no authority; the coordinator must separately commit and
    /// deliver the exact recovery grant. Caller retains verified waiting ownership.
    /// Success is transport only: require the matched worker's preparation response
    /// before committing authority; rejection/uncertainty must fence orchestration.
    /// # Errors
    /// Failed delivery terminally fences all controls/clones without byte replay.
    pub fn prepare_recovery(
        &mut self,
        prepared: &forge_app::durable_execution::recovery::authorization::Prepared,
    ) -> Result<(), RegistrationError> {
        let control = forge_rpc::durable_execution::SupervisorRecoveryPreparation {
            host_prepare_recovery:
                forge_rpc::durable_execution::SUPERVISOR_RECOVERY_PREPARATION_VERSION,
            candidate: prepared.candidate().clone(),
            grant: prepared.grant().clone(),
        };
        let mut input = self.input.lock().map_err(|_| uncertain())?;
        input.send(&control, 1_048_576)
    }

    /// Stop an owned predecessor only after persisting the exact recovery claim.
    /// Success is delivery, not descendant proof, revocation or resume authority.
    /// # Errors
    /// Uncertain delivery terminally fences every clone/control on this pipe.
    pub fn quiesce(
        &mut self,
        operation: &forge_app::durable_execution::recovery::quiescence::Operation,
    ) -> Result<(), RegistrationError> {
        let control = forge_rpc::durable_execution::SupervisorQuiescence {
            host_quiesce: forge_rpc::durable_execution::SUPERVISOR_QUIESCENCE_VERSION,
            operation: operation.clone(),
        };
        let mut input = self.input.lock().map_err(|_| uncertain())?;
        input.send(&control, 1_048_576)
    }

    /// Deliver one already recorded follow-up. Success is transport only: the
    /// runner's `followup-accepted` event is the admission evidence, and any
    /// uncertain send fences the pipe with no byte replay.
    /// # Errors
    /// Rejects an invalid request before writing; uncertain delivery fences all controls.
    pub fn deliver_followup(
        &mut self,
        request: &forge_app::durable_execution::followup::Request,
    ) -> Result<(), RegistrationError> {
        request.validate()?;
        let control = forge_rpc::durable_execution::SupervisorFollowUp {
            host_followup: forge_rpc::durable_execution::SUPERVISOR_FOLLOWUP_VERSION,
            followup: forge_rpc::durable_execution::FollowUpPayload {
                operation_id: request.operation_id.clone(),
                content: request.message.clone(),
                content_sha256: request.digest(),
            },
        };
        let mut input = self.input.lock().map_err(|_| uncertain())?;
        input.send(&control, 1_048_576)
    }

    /// Send one runner protocol-1 admission or observation request. Success is
    /// transport only: the runner's correlated `RunnerResponse` is the result, and
    /// an admission is idempotent by its operation ID. The caller must persist
    /// send intent first; uncertain delivery fences every control on this pipe.
    /// # Errors
    /// Rejects an invalid request before writing; uncertain delivery fences all controls.
    pub fn request(
        &mut self,
        request: &forge_rpc::durable_execution::RunnerRequest,
    ) -> Result<(), RegistrationError> {
        request.validate(&request.compatibility.component_hash)?;
        let mut input = self.input.lock().map_err(|_| uncertain())?;
        input.send(request, 1_048_576)
    }

    /// Send only an already persisted, revoked retirement operation. Partial or
    /// blocked delivery remains uncertain; this does not acknowledge cancellation.
    /// # Errors
    /// Rejects unavailable serialization, lock or pipe without retrying bytes.
    pub fn retire(
        &mut self,
        operation: &forge_app::durable_execution::retirement::Operation,
    ) -> Result<(), RegistrationError> {
        let mut input = self.input.lock().map_err(|_| uncertain())?;
        write_retirement(&mut *input, operation)
    }
}

fn write_retirement<W: Write>(
    writer: &mut Pipe<W>,
    operation: &forge_app::durable_execution::retirement::Operation,
) -> Result<(), RegistrationError> {
    let control = forge_rpc::durable_execution::SupervisorRetirement {
        host_retire: forge_rpc::durable_execution::SUPERVISOR_RETIREMENT_VERSION,
        operation: operation.clone(),
    };
    writer.send(&control, 4096)
}

fn write_control<W: Write>(writer: &mut Pipe<W>, lease: &Lease) -> Result<(), RegistrationError> {
    let control = SupervisorControl {
        host_control: SUPERVISOR_CONTROL_VERSION,
        lease: lease.clone(),
    };
    writer.send(&control, 1_048_576)
}

impl forge_app::durable_execution::recovery::session::Delivery for ControlPipe {
    fn deliver_recovery(
        &mut self,
        grant: &forge_app::durable_execution::recovery::authorization::Grant,
    ) -> Result<(), RegistrationError> {
        let control = forge_rpc::durable_execution::SupervisorRecovery {
            host_recovery: forge_rpc::durable_execution::SUPERVISOR_RECOVERY_VERSION,
            grant: grant.clone(),
        };
        let mut input = self.input.lock().map_err(|_| uncertain())?;
        input.send(&control, 1_048_576)
    }
}

impl Delivery for ControlPipe {
    fn deliver(&mut self, lease: &Lease) -> Result<(), RegistrationError> {
        let mut input = self.input.lock().map_err(|_| uncertain())?;
        write_control(&mut *input, lease)
    }
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod recovery_tests;

#[cfg(test)]
mod tests {
    use super::*;

    struct Writer {
        calls: usize,
        partial: bool,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.calls += 1;
            if self.partial {
                Ok(bytes.len() / 2)
            } else {
                Err(std::io::ErrorKind::WouldBlock.into())
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            unreachable!("unbuffered control must not flush/retry")
        }
    }

    #[test]
    fn retirement_partial_or_blocked_delivery_never_replays_or_acknowledges_cleanup() {
        let lease: Lease = serde_json::from_str(include_str!(
            "../../../runner/test/authorization-lease.json"
        ))
        .unwrap();
        let operation = forge_app::durable_execution::retirement::Operation {
            operation_id: "retirement".into(),
            launch: lease.launch,
            owner: lease.owner,
        };
        for partial in [true, false] {
            let mut writer = Pipe {
                writer: Writer { calls: 0, partial },
                fenced: false,
            };
            assert_eq!(
                write_retirement(&mut writer, &operation).unwrap_err().code,
                "authorization-delivery-uncertain"
            );
            assert!(write_retirement(&mut writer, &operation).is_err());
            assert_eq!(writer.writer.calls, 1);
        }
        let mut pipe = Pipe {
            writer: Vec::new(),
            fenced: false,
        };
        write_retirement(&mut pipe, &operation).unwrap();
        assert_eq!(pipe.writer.last(), Some(&b'\n'));
        let control: forge_rpc::durable_execution::SupervisorRetirement =
            serde_json::from_slice(&pipe.writer).unwrap();
        assert_eq!(control.operation, operation);
        let mut json = serde_json::to_value(control).unwrap();
        json["unrecognized"] = true.into();
        assert!(
            serde_json::from_value::<forge_rpc::durable_execution::SupervisorRetirement>(json)
                .is_err()
        );
    }

    #[test]
    fn partial_or_blocked_delivery_never_retries_or_echoes_payload() {
        let lease: Lease = serde_json::from_str(include_str!(
            "../../../runner/test/authorization-lease.json"
        ))
        .unwrap();
        for partial in [true, false] {
            let mut writer = Pipe {
                writer: Writer { calls: 0, partial },
                fenced: false,
            };
            let error = write_control(&mut writer, &lease).unwrap_err();
            assert_eq!(error.code, "authorization-delivery-uncertain");
            assert!(write_control(&mut writer, &lease).is_err());
            assert_eq!(writer.writer.calls, 1);
            assert!(!error.message.contains("epoch"));
        }
        let mut pipe = Pipe {
            writer: Vec::new(),
            fenced: false,
        };
        write_control(&mut pipe, &lease).unwrap();
        assert_eq!(pipe.writer.last(), Some(&b'\n'));
        let control: SupervisorControl = serde_json::from_slice(&pipe.writer).unwrap();
        assert_eq!(control.lease, lease);
    }
}
