//! Trusted operator surface. Never part of the Assignment capability API.
pub use forge_app::durable_execution::{
    effect_resolution::{ExternalOutcome, Resolution, ResolutionReceipt},
    effects::InterruptedEffect,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveInterruptedEffectRequest {
    pub assignment_id: String,
    pub resolution: Resolution,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operator_resolution_rejects_credentials_paths_unknown_fields_and_outcomes() {
        let value = serde_json::json!({"assignment_id": "assignment", "resolution": {
            "operation_id": "resolve", "effect": {"task_id": "tool", "conversation_id": "child", "call_id": "call"},
            "outcome": "applied", "actor": "operator", "evidence": "external receipt reference", "message": "investigated",
            "confirm_old_agent_cannot_act": true, "confirm_external_work_safe": true
        }});
        let request: ResolveInterruptedEffectRequest =
            serde_json::from_value(value.clone()).unwrap();
        request.resolution.validate().unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), value);
        for field in [
            "secret",
            "capability_id",
            "node",
            "inspector",
            "runtime_directory",
            "launch",
        ] {
            let mut invalid = value.clone();
            invalid[field] = "NOT-OPERATOR-AUTH".into();
            assert!(serde_json::from_value::<ResolveInterruptedEffectRequest>(invalid).is_err());
        }
        let mut unknown = value;
        unknown["resolution"]["outcome"] = "unknown".into();
        assert!(serde_json::from_value::<ResolveInterruptedEffectRequest>(unknown).is_err());
    }
}
