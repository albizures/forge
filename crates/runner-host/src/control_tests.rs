use super::*;
use forge_rpc::durable_execution::{SUPERVISOR_RECOVERY_VERSION, SupervisorRecovery};

fn control() -> SupervisorRecovery {
    serde_json::from_str(include_str!("../../../runner/test/recovery-control.json")).unwrap()
}

#[test]
fn recovery_control_is_strict_exact_and_separate_from_ordinary_control() {
    let control = control();
    assert_eq!(control.host_recovery, SUPERVISOR_RECOVERY_VERSION);
    control.grant.claim.validate().unwrap();
    let mut policy = forge_app::authorization_lease::LeaseWatchdog::for_recovery(
        control.grant.lease.owner.clone(),
        control.grant.clone(),
    )
    .unwrap();
    let mut pipe = Pipe {
        writer: Vec::new(),
        fenced: false,
    };
    pipe.send(&control, 1_048_576).unwrap();
    let decoded: SupervisorRecovery = serde_json::from_slice(&pipe.writer).unwrap();
    assert_eq!(decoded, control);
    policy.accept_recovery(&decoded.grant, 1000).unwrap();
    assert_eq!(policy.last_lease(), Some(&control.grant.lease));
    assert!(serde_json::from_slice::<SupervisorControl>(&pipe.writer).is_err());
    let mut value = serde_json::to_value(&control).unwrap();
    for path in [
        "/",
        "/grant",
        "/grant/claim",
        "/grant/lease",
        "/grant/lease/owner",
    ] {
        let target = if path == "/" {
            &mut value
        } else {
            value.pointer_mut(path).unwrap()
        };
        target["unknown"] = true.into();
        assert!(serde_json::from_value::<SupervisorRecovery>(value.clone()).is_err());
        value = serde_json::to_value(&control).unwrap();
    }
}

struct FaultWriter {
    calls: usize,
    fault: u8,
}
impl Write for FaultWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.calls += 1;
        match self.fault {
            0 => Ok(bytes.len() / 2),
            1 => Ok(0),
            2 => Err(std::io::ErrorKind::Interrupted.into()),
            3 => Err(std::io::ErrorKind::WouldBlock.into()),
            _ => Err(std::io::ErrorKind::BrokenPipe.into()),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        panic!("must not flush/retry")
    }
}

#[test]
fn every_uncertain_recovery_send_fences_all_later_control_kinds() {
    for fault in 0..5 {
        let control = control();
        let mut pipe = Pipe {
            writer: FaultWriter { calls: 0, fault },
            fenced: false,
        };
        let error = pipe.send(&control, 1_048_576).unwrap_err();
        assert_eq!(error.code, "authorization-delivery-uncertain");
        assert!(!error.message.contains(&control.grant.evidence_sha256));
        assert!(pipe.send(&control, 1_048_576).is_err());
        assert!(write_control(&mut pipe, &control.grant.lease).is_err());
        let op = forge_app::durable_execution::retirement::Operation {
            operation_id: "retire".into(),
            launch: control.grant.lease.launch,
            owner: control.grant.lease.owner,
        };
        assert!(write_retirement(&mut pipe, &op).is_err());
        assert_eq!(pipe.writer.calls, 1);
    }
}

#[test]
fn oversized_recovery_is_rejected_before_writing_and_terminally_fences() {
    let mut control = control();
    control.grant.evidence_sha256 = "x".repeat(1_048_576);
    let mut pipe = Pipe {
        writer: Vec::new(),
        fenced: false,
    };
    assert!(pipe.send(&control, 1_048_576).is_err());
    assert!(pipe.writer.is_empty());
    assert!(write_control(&mut pipe, &control.grant.lease).is_err());
    assert!(pipe.writer.is_empty());
}

#[test]
fn complete_recovery_and_renewal_are_separate_newline_records() {
    let control = control();
    let mut pipe = Pipe {
        writer: Vec::new(),
        fenced: false,
    };
    pipe.send(&control, 1_048_576).unwrap();
    let mut renewal = control.grant.lease.clone();
    renewal.sequence += 1;
    renewal.issued_boottime_millis += 10_000;
    renewal.expires_boottime_millis += 10_000;
    write_control(&mut pipe, &renewal).unwrap();
    let records: Vec<_> = pipe.writer.split_inclusive(|b| *b == b'\n').collect();
    assert_eq!(records.len(), 2);
    assert_eq!(
        serde_json::from_slice::<SupervisorRecovery>(records[0]).unwrap(),
        control
    );
    assert_eq!(
        serde_json::from_slice::<SupervisorControl>(records[1])
            .unwrap()
            .lease,
        renewal
    );
}
