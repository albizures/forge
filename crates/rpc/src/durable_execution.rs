//! Internal runner wire v1. Separate from operator JSON-RPC and management protocol.
//! Defining operations does not enable their execution or issue authorization leases.
use forge_app::{
    RegistrationError,
    durable_execution::{Admission, Launch},
};
use serde::{Deserialize, Serialize};

/// Separate trusted daemon-to-supervisor pipe control, not operator JSON-RPC or
/// an agent capability. Only the holder of the directly spawned host's stdin
/// write end may deliver a transactionally committed grant. Never read from disk.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorControl {
    pub host_control: u32,
    pub lease: forge_app::authorization_lease::Lease,
}

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
    Admit {
        launch: Launch,
    },
    Observe {
        admission_operation_id: String,
    },
    Transcript {
        conversation_id: String,
        cursor: Option<String>,
        limit: u16,
    },
    Recover {
        incarnation_id: String,
        authorization_id: String,
    },
    FollowUp {
        input_id: String,
        content: String,
        authorization_id: String,
    },
    Retire {
        revocation_id: String,
    },
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
