//! Nonblocking delivery to a directly spawned supervisor's private input pipe.
use forge_app::{
    RegistrationError,
    authorization_lease::{Lease, session::Delivery},
};
use forge_rpc::durable_execution::{SUPERVISOR_CONTROL_VERSION, SupervisorControl};
use std::{
    io::Write,
    process::{Child, ChildStdin},
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
pub struct ControlPipe {
    input: ChildStdin,
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
        Ok(Self { input })
    }
}

fn write_control(writer: &mut impl Write, lease: &Lease) -> Result<(), RegistrationError> {
    let control = SupervisorControl {
        host_control: SUPERVISOR_CONTROL_VERSION,
        lease: lease.clone(),
    };
    let mut bytes = serde_json::to_vec(&control).map_err(|_| uncertain())?;
    bytes.push(b'\n');
    if bytes.len() > 1_048_576 {
        return Err(uncertain());
    }
    // Exactly one nonblocking write. A partial write, EINTR or full pipe is
    // terminal uncertainty, never a locally retried/concatenated JSON control.
    match writer.write(&bytes) {
        Ok(count) if count == bytes.len() => Ok(()),
        _ => Err(uncertain()),
    }
}

impl Delivery for ControlPipe {
    fn deliver(&mut self, lease: &Lease) -> Result<(), RegistrationError> {
        write_control(&mut self.input, lease)
    }
}

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
    fn partial_or_blocked_delivery_never_retries_or_echoes_payload() {
        let lease: Lease = serde_json::from_str(include_str!(
            "../../../runner/test/authorization-lease.json"
        ))
        .unwrap();
        for partial in [true, false] {
            let mut writer = Writer { calls: 0, partial };
            let error = write_control(&mut writer, &lease).unwrap_err();
            assert_eq!(error.code, "authorization-delivery-uncertain");
            assert_eq!(writer.calls, 1);
            assert!(!error.message.contains("epoch"));
        }
        let mut bytes = Vec::new();
        write_control(&mut bytes, &lease).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
        let control: SupervisorControl = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(control.lease, lease);
    }
}
