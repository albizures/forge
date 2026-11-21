//! Internal runner protocol 1. Separate from operator JSON-RPC and management protocol.
//! Defining operations does not enable their execution or issue authorization leases.
//!
//! One versioned contract between the daemon, `forge-runner-host` and the shipped
//! Check runner (`runner/src/check-runner.ts`):
//! - input binding: [`CheckRuntimeInput`] (`check-runtime.json`, written before spawn);
//! - admission and observation: [`RunnerRequest`] → [`RunnerResponse`] / [`RunnerRefusal`];
//! - grant and renewal: [`SupervisorControl`];
//! - same-attempt recovery: [`SupervisorRecoveryPreparation`], [`SupervisorRecovery`];
//! - recorded follow-up: [`SupervisorFollowUp`];
//! - retirement and quiescence: [`SupervisorRetirement`], [`SupervisorQuiescence`]
//!   (consumed by the host; the runner only receives cancellation);
//! - runner progress: [`RunnerEvent`], parsed strictly at the Rust boundary.
//!
//! Read-only transcript, answer and recovery inspection use their own bounded
//! subprocess wires in the submodules. Every defined operation has a real path.
use forge_app::{
    RegistrationError,
    durable_execution::{Admission, Launch},
};
use serde::{Deserialize, Serialize};

pub mod answer_inspection;
pub mod recovery_inspection;
pub mod transcript_inspection;

/// Separate trusted daemon-to-supervisor pipe control, not operator JSON-RPC or
/// an agent capability. Only the holder of the directly spawned host's stdin
/// write end may deliver a transactionally committed grant. Never read from disk.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorControl {
    pub host_control: u32,
    pub lease: forge_app::authorization_lease::Lease,
}

/// Additive, separately versioned retirement control on the same private pipe.
/// Its original operation must already be durably recorded after revocation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorRetirement {
    pub host_retire: u32,
    pub operation: forge_app::durable_execution::retirement::Operation,
}

/// Initial recovery delivery is the whole committed envelope, never an ordinary
/// lease with a relaxed starting sequence. This private control does not prepare
/// a consumer or authenticate retained evidence as fresh host verification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorRecovery {
    pub host_recovery: u32,
    pub grant: forge_app::durable_execution::recovery::authorization::Grant,
}

/// Prepare an unarmed waiting receiver from fresh trusted daemon composition.
/// This is not delivery or proof of commit. Retained grants never configure a
/// restarted owner: the candidate/predecessor and freshly captured owner differ.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorRecoveryPreparation {
    pub host_prepare_recovery: u32,
    pub candidate: forge_app::durable_execution::recovery::Candidate,
    pub grant: forge_app::durable_execution::recovery::authorization::Grant,
}

/// Stop an exactly owned predecessor for a persisted recovery reservation.
/// Unlike retirement, this preserves live Assignment authority and all fences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorQuiescence {
    pub host_quiesce: u32,
    pub operation: forge_app::durable_execution::recovery::quiescence::Operation,
}

/// Exact recorded operator input for the resumed conversation, sent only on the
/// protected supervisor pipe. It carries identity and text, never binding fields.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowUpPayload {
    pub operation_id: String,
    pub content: String,
    pub content_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorFollowUp {
    pub host_followup: u32,
    pub followup: FollowUpPayload,
}

pub const SUPERVISOR_FOLLOWUP_VERSION: u32 = 1;
pub const SUPERVISOR_RECOVERY_PREPARATION_VERSION: u32 = 1;
pub const SUPERVISOR_QUIESCENCE_VERSION: u32 = 1;
pub const SUPERVISOR_RECOVERY_VERSION: u32 = 1;
pub const SUPERVISOR_RETIREMENT_VERSION: u32 = 1;
pub const SUPERVISOR_CONTROL_VERSION: u32 = 1;
pub const RUNNER_PROTOCOL_VERSION: u32 = 1;
pub const DURABLE_RUNTIME_SCHEMA: u32 = forge_app::durable_execution::RUNTIME_SCHEMA_VERSION;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerCompatibility {
    pub protocol: u32,
    pub runtime_schema: u32,
    pub component_hash: String,
}

impl RunnerCompatibility {
    /// Exact matched-component negotiation, before writable opening or admission.
    /// # Errors
    /// Rejects unsupported versions or a different retained component.
    pub fn validate(&self, expected_component: &str) -> Result<(), RegistrationError> {
        if self.protocol != RUNNER_PROTOCOL_VERSION {
            return Err(RegistrationError::new(
                "runner-protocol-incompatible",
                "expected runner protocol 1",
            ));
        }
        if self.runtime_schema != DURABLE_RUNTIME_SCHEMA {
            return Err(RegistrationError::new(
                "runner-schema-incompatible",
                "expected runtime schema 2; no automatic conversion",
            ));
        }
        if expected_component.is_empty() || self.component_hash != expected_component {
            return Err(RegistrationError::new(
                "runner-component-incompatible",
                "select the matched retained Forge runner component",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerRequest {
    pub compatibility: RunnerCompatibility,
    pub assignment_id: String,
    pub runtime_id: String,
    pub operation_id: String,
    pub operation: RunnerOperation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RunnerOperation {
    Admit { launch: Launch },
    Observe { admission_operation_id: String },
}

/// A request that could not be correlated or parsed. It carries a stable code
/// only: never input, paths, credentials or upstream diagnostics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerRefusal {
    pub error: RunnerRefusalCode,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerRefusalCode {
    pub code: String,
}

/// Kernel ownership evidence the host persisted and the worker re-verified.
/// Descriptive only: never a grant, adoption permission or cleanup proof.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerOwnerEvidence {
    pub owner: RunnerHostOwner,
    pub worker_pid: u32,
    pub worker_start: String,
    pub process_group: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerHostOwner {
    pub host_pid: u32,
    pub host_start: String,
    pub boot_id: String,
    pub lock_device: String,
    pub lock_inode: String,
    pub lock_fd: i32,
}

/// Runner progress on stdout. Events are observations, never Forge Outcomes,
/// authority or cleanup evidence. Unknown events or fields are protocol errors.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RunnerEvent {
    Owned {
        identity: RunnerOwnerEvidence,
        compatibility: RunnerCompatibility,
    },
    Authorized {
        sequence: u64,
    },
    RecoveryPrepared {},
    Bootstrapped {},
    Resumed {
        root: u64,
        admission_id: String,
    },
    Settled {
        status: String,
    },
    RuntimeFailed {},
    FollowupAccepted {
        operation_id: String,
        admission_id: String,
        replayed: bool,
    },
    FollowupFailed {
        operation_id: Option<String>,
    },
}

/// Exactly one JSON value per runner stdout line.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RunnerOutput {
    Event(RunnerEvent),
    Response(RunnerResponse),
    Refusal(RunnerRefusal),
}

impl RunnerOutput {
    /// Parse one bounded stdout line strictly.
    /// # Errors
    /// Rejects anything that is not exactly one defined protocol-1 value.
    pub fn parse(line: &[u8]) -> Result<Self, RegistrationError> {
        serde_json::from_slice(line).map_err(|_| {
            RegistrationError::new(
                "runner-output-invalid",
                "runner output is not a protocol 1 value",
            )
        })
    }
}

pub const CHECK_RUNTIME_VERSION: u32 = 1;
pub const CHECK_RUNTIME_FILE: &str = "check-runtime.json";

/// Delivered capability metadata and the protected secret-file PATH only.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckCapability {
    #[serde(rename = "FORGE_ASSIGNMENT_ID")]
    pub assignment_id: String,
    #[serde(rename = "FORGE_ASSIGNMENT_DAEMON_URL")]
    pub daemon_url: String,
    #[serde(rename = "FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE")]
    pub secret_file: std::path::PathBuf,
    #[serde(rename = "FORGE_ASSIGNMENT_FORGE_CLI")]
    pub forge_cli: std::path::PathBuf,
}

/// Original Check binding of one durable runtime, used for first admission and
/// for any later same-attempt recovery. It contains no key or capability bytes,
/// behavior or lease, and grants nothing: execution still needs a current lease
/// and successful authenticated bootstrap.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRuntimeInput {
    pub version: u32,
    pub launch: Launch,
    pub generation: u64,
    pub bound: forge_app::assignment::BoundIdentity,
    pub configuration: forge_app::durable_profiles::ExecutionConfiguration,
    pub credential_file: std::path::PathBuf,
    pub capability: CheckCapability,
}

impl std::fmt::Debug for CheckRuntimeInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckRuntimeInput")
            .field("version", &self.version)
            .field("launch", &self.launch)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl CheckRuntimeInput {
    /// Structural consistency the runner also enforces. Bootstrap and profile
    /// preflight remain the authority for binding, resources and authentication.
    /// # Errors
    /// Rejects mismatched identity, generation, cwd or credential separation.
    pub fn validate(&self) -> Result<(), RegistrationError> {
        self.launch.validate()?;
        let attempt = self.bound.check_attempt.as_ref();
        let consistent = self.version == CHECK_RUNTIME_VERSION
            && self.generation > 0
            && self.bound.assignment_id == self.launch.assignment_id
            && self.bound.profile_revision == self.launch.profile_revision
            && attempt.is_some_and(|attempt| {
                attempt.id == self.launch.attempt_id && attempt.generation == self.generation
            })
            && self.bound.execution_directory == self.configuration.cwd
            && self.capability.assignment_id == self.launch.assignment_id
            && self.credential_file.is_absolute()
            && self.credential_file != self.capability.secret_file;
        if consistent {
            Ok(())
        } else {
            Err(RegistrationError::new(
                "runner-check-input-invalid",
                "durable Check runtime input is inconsistent",
            ))
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerResponse {
    pub compatibility: RunnerCompatibility,
    pub assignment_id: String,
    pub runtime_id: String,
    pub operation_id: String,
    pub admission: Admission,
}

impl RunnerRequest {
    /// Validate correlation before adapting a request to a runner action.
    /// # Errors
    /// Rejects mismatched identity or compatibility, including nested launch identity.
    pub fn validate(&self, expected_component: &str) -> Result<(), RegistrationError> {
        self.compatibility.validate(expected_component)?;
        if let RunnerOperation::Admit { launch } = &self.operation {
            launch.validate()?;
        }
        let invalid = self.assignment_id.trim().is_empty()
            || self.runtime_id.trim().is_empty()
            || self.operation_id.trim().is_empty();
        let mismatch = matches!(&self.operation, RunnerOperation::Admit { launch }
            if launch.assignment_id != self.assignment_id || launch.runtime_id != self.runtime_id
            || launch.operation_id != self.operation_id || launch.component_hash != self.compatibility.component_hash
            || launch.runtime_schema != self.compatibility.runtime_schema);
        if invalid || mismatch {
            return Err(RegistrationError::new(
                "runner-identity-conflict",
                "runner request identities do not match",
            ));
        }
        Ok(())
    }
}

impl RunnerResponse {
    /// # Errors
    /// Rejects a reply from a different operation, runtime or matched component.
    pub fn validate(&self, request: &RunnerRequest) -> Result<(), RegistrationError> {
        request.validate(&request.compatibility.component_hash)?;
        self.compatibility
            .validate(&request.compatibility.component_hash)?;
        if self.assignment_id != request.assignment_id
            || self.runtime_id != request.runtime_id
            || self.operation_id != request.operation_id
        {
            return Err(RegistrationError::new(
                "runner-response-conflict",
                "runner reply does not match the requested operation",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_preparation_is_strict_distinct_and_not_a_grant_delivery() {
        use forge_app::{
            authorization_lease::Lease,
            durable_execution::recovery::{Candidate, Claim, authorization::Grant},
            outcomes::{Attempt, Evidence},
        };
        let original: Lease = serde_json::from_str(include_str!(
            "../../../runner/test/authorization-lease.json"
        ))
        .unwrap();
        let candidate = Candidate {
            launch: original.launch.clone(),
            admission_id: "original".into(),
            attempt: Attempt {
                id: original.launch.attempt_id.clone(),
                record_id: 1,
                check_id: 1,
                generation: original.generation,
                evidence: Evidence::Repository {
                    repository: "repository".into(),
                    commit: "a".repeat(40),
                },
                actor: original.launch.assignment_id.clone(),
                body: "bound task".into(),
                continuation: None,
                recovery: None,
            },
            execution_directory: "/bound/worktree".into(),
            last_lease: original.clone(),
        };
        let claim = Claim::for_candidate(&candidate, "recover".into(), "new-epoch".into()).unwrap();
        let mut lease = original;
        lease.sequence += 1;
        lease.owner.host_pid += 100;
        lease.owner.worker_pid += 100;
        lease.daemon_epoch.clone_from(&claim.daemon_epoch);
        let control = SupervisorRecoveryPreparation {
            host_prepare_recovery: SUPERVISOR_RECOVERY_PREPARATION_VERSION,
            candidate,
            grant: Grant {
                claim,
                lease,
                evidence_sha256: "a".repeat(64),
            },
        };
        control
            .grant
            .validate_candidate(&control.candidate, 1_000)
            .unwrap();
        let value = serde_json::to_value(&control).unwrap();
        assert_eq!(
            serde_json::from_value::<SupervisorRecoveryPreparation>(value.clone()).unwrap(),
            control
        );
        assert!(serde_json::from_value::<SupervisorRecovery>(value.clone()).is_err());
        assert!(serde_json::from_value::<SupervisorControl>(value.clone()).is_err());
        for path in [
            vec![],
            vec!["candidate"],
            vec!["candidate", "attempt"],
            vec!["grant"],
            vec!["grant", "claim"],
            vec!["grant", "lease"],
        ] {
            let mut changed = value.clone();
            let mut target = &mut changed;
            for key in path {
                target = &mut target[key];
            }
            target["unknown"] = true.into();
            assert!(serde_json::from_value::<SupervisorRecoveryPreparation>(changed).is_err());
        }
    }

    #[test]
    fn supervisor_control_is_strict_and_uses_the_application_grant_shape() {
        let lease: serde_json::Value = serde_json::from_str(include_str!(
            "../../../runner/test/authorization-lease.json"
        ))
        .unwrap();
        let value = serde_json::json!({"host_control": SUPERVISOR_CONTROL_VERSION, "lease": lease});
        let control: SupervisorControl = serde_json::from_value(value.clone()).unwrap();
        control.lease.validate(None, 1_000).unwrap();
        assert_eq!(serde_json::to_value(control).unwrap(), value);
        let mut unknown = value;
        unknown["agent_input"] = true.into();
        assert!(serde_json::from_value::<SupervisorControl>(unknown).is_err());
    }

    #[test]
    fn quiescence_control_is_strict_and_distinct_from_retirement_and_recovery() {
        let control: SupervisorRecovery =
            serde_json::from_str(include_str!("../../../runner/test/recovery-control.json"))
                .unwrap();
        let mut predecessor = control.grant.lease;
        predecessor.daemon_epoch = "original-daemon".into();
        let control = SupervisorQuiescence {
            host_quiesce: SUPERVISOR_QUIESCENCE_VERSION,
            operation: forge_app::durable_execution::recovery::quiescence::Operation {
                claim: control.grant.claim,
                predecessor,
            },
        };
        control.operation.validate().unwrap();
        let value = serde_json::to_value(&control).unwrap();
        assert_eq!(
            serde_json::from_value::<SupervisorQuiescence>(value.clone()).unwrap(),
            control
        );
        assert!(serde_json::from_value::<SupervisorRetirement>(value.clone()).is_err());
        assert!(serde_json::from_value::<SupervisorRecovery>(value.clone()).is_err());
        for path in [
            "",
            "/operation",
            "/operation/claim",
            "/operation/predecessor",
        ] {
            let mut wrong = value.clone();
            wrong.pointer_mut(path).unwrap()["unknown"] = true.into();
            assert!(serde_json::from_value::<SupervisorQuiescence>(wrong).is_err());
        }
    }

    #[test]
    fn runner_events_are_typed_strict_and_cover_every_runner_emission() {
        let compatibility =
            serde_json::json!({"protocol": 1, "runtime_schema": 2, "component_hash": "pinned"});
        let identity = serde_json::json!({"owner": {"host_pid": 1, "host_start": "10", "boot_id": "boot",
            "lock_device": "1", "lock_inode": "2", "lock_fd": 3}, "worker_pid": 2, "worker_start": "11", "process_group": 2});
        let lines = [
            serde_json::json!({"event": "owned", "identity": identity, "compatibility": compatibility}),
            serde_json::json!({"event": "authorized", "sequence": 1}),
            serde_json::json!({"event": "recovery-prepared"}),
            serde_json::json!({"event": "bootstrapped"}),
            serde_json::json!({"event": "resumed", "root": 1, "admission_id": "2"}),
            serde_json::json!({"event": "settled", "status": "done"}),
            serde_json::json!({"event": "runtime-failed"}),
            serde_json::json!({"event": "followup-accepted", "operation_id": "f", "admission_id": "3", "replayed": false}),
            serde_json::json!({"event": "followup-failed", "operation_id": null}),
        ];
        for line in lines {
            let bytes = serde_json::to_vec(&line).unwrap();
            let RunnerOutput::Event(event) = RunnerOutput::parse(&bytes).unwrap() else {
                panic!("not an event: {line}");
            };
            assert_eq!(serde_json::to_value(event).unwrap(), line);
            let mut extra = line.clone();
            extra["unexpected"] = true.into();
            assert!(RunnerOutput::parse(&serde_json::to_vec(&extra).unwrap()).is_err());
        }
        for invalid in [
            serde_json::json!({"event": "recovered-model"}),
            serde_json::json!({"event": "admission", "response": {}}),
            serde_json::json!({"settled": true}),
        ] {
            assert_eq!(
                RunnerOutput::parse(&serde_json::to_vec(&invalid).unwrap())
                    .unwrap_err()
                    .code,
                "runner-output-invalid"
            );
        }
        let refusal = serde_json::json!({"error": {"code": "assignment-lease-denied"}});
        assert!(matches!(
            RunnerOutput::parse(&serde_json::to_vec(&refusal).unwrap()).unwrap(),
            RunnerOutput::Refusal(_)
        ));
        let response = serde_json::json!({"compatibility": compatibility, "assignment_id": "a",
            "runtime_id": "r", "operation_id": "op", "admission": {"state": "rejected", "code": "assignment-runtime-failed"}});
        let RunnerOutput::Response(response) =
            RunnerOutput::parse(&serde_json::to_vec(&response).unwrap()).unwrap()
        else {
            panic!("not a response");
        };
        assert_eq!(
            response.admission,
            Admission::Rejected {
                code: "assignment-runtime-failed".into()
            }
        );
    }

    #[test]
    fn protocol_one_defines_no_rejected_request_operations() {
        let base = serde_json::json!({"compatibility": {"protocol": 1, "runtime_schema": 2,
            "component_hash": "pinned"}, "assignment_id": "a", "runtime_id": "r", "operation_id": "op"});
        for operation in [
            serde_json::json!({"kind": "transcript", "conversation_id": "c", "cursor": null, "limit": 1}),
            serde_json::json!({"kind": "recover", "incarnation_id": "i", "authorization_id": "x"}),
            serde_json::json!({"kind": "follow-up", "input_id": "i", "content": "c", "authorization_id": "x"}),
            serde_json::json!({"kind": "retire", "revocation_id": "r"}),
        ] {
            let mut request = base.clone();
            request["operation"] = operation;
            assert!(serde_json::from_value::<RunnerRequest>(request).is_err());
        }
    }

    #[test]
    fn negotiation_fails_closed_with_structured_diagnostics() {
        let mut c = RunnerCompatibility {
            protocol: 1,
            runtime_schema: 2,
            component_hash: "pinned".into(),
        };
        assert!(c.validate("pinned").is_ok());
        c.protocol = 2;
        assert_eq!(
            c.validate("pinned").unwrap_err().code,
            "runner-protocol-incompatible"
        );
        c.protocol = 1;
        c.runtime_schema = 3;
        assert_eq!(
            c.validate("pinned").unwrap_err().code,
            "runner-schema-incompatible"
        );
        c.runtime_schema = 2;
        assert_eq!(
            c.validate("other").unwrap_err().code,
            "runner-component-incompatible"
        );
    }
    #[test]
    fn typescript_runner_fixture_matches_rust_admission_wire() {
        let value: serde_json::Value =
            serde_json::from_str(include_str!("../../../runner/test/wire-v1.json")).unwrap();
        let request: RunnerRequest = serde_json::from_value(value.clone()).unwrap();
        request.validate("fixture-component").unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), value);
    }

    #[test]
    fn wire_is_typed_strict_and_correlated() {
        let request = RunnerRequest {
            compatibility: RunnerCompatibility {
                protocol: 1,
                runtime_schema: 2,
                component_hash: "pinned".into(),
            },
            assignment_id: "a".into(),
            runtime_id: "r".into(),
            operation_id: "op".into(),
            operation: RunnerOperation::Observe {
                admission_operation_id: "launch".into(),
            },
        };
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<RunnerRequest>(&json).unwrap(),
            request
        );
        assert!(
            serde_json::from_str::<RunnerRequest>(
                &json.replace("\"kind\":\"observe\"", "\"kind\":\"future\"")
            )
            .is_err()
        );
        let mut response = RunnerResponse {
            compatibility: request.compatibility.clone(),
            assignment_id: "a".into(),
            runtime_id: "r".into(),
            operation_id: "op".into(),
            admission: Admission::Unknown,
        };
        assert!(response.validate(&request).is_ok());
        response.operation_id = "other".into();
        assert_eq!(
            response.validate(&request).unwrap_err().code,
            "runner-response-conflict"
        );
    }
}
