//! Internal committed-answer subprocess contract, not completion authority.
pub use super::recovery_inspection::Request;
use forge_app::{RegistrationError, durable_execution::Launch};
use serde::{Deserialize, Serialize};
pub const MAX_RESPONSE_BYTES: u64 = 16384;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Answer {
    Answered,
    Unanswered,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub protocol: u32,
    pub launch: Launch,
    pub answer: Answer,
}
impl Response {
    /// # Errors
    /// Rejects incompatible/foreign observations; no scheduling is authorized.
    pub fn validate(&self, launch: &Launch) -> Result<(), RegistrationError> {
        if self.protocol != super::RUNNER_PROTOCOL_VERSION || self.launch != *launch {
            return Err(RegistrationError::new(
                "runner-answer-inspection-invalid",
                "expected exact original runtime and compatible committed-answer inspection",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn answer_inspection_rejects_foreign_unknown_and_incompatible_wire_records() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../../runner/test/wire-v1.json")).unwrap();
        let launch: Launch =
            serde_json::from_value(fixture["operation"]["launch"].clone()).unwrap();
        for answer in ["answered", "unanswered", "unknown"] {
            let record = serde_json::json!({"protocol": 1, "launch": launch, "answer": answer});
            let response: Response = serde_json::from_value(record.clone()).unwrap();
            response.validate(&launch).unwrap();
            let mut extra = record;
            extra["private"] = "PRIVATE-CANARY".into();
            assert!(serde_json::from_value::<Response>(extra).is_err());
        }
        let mut response = Response {
            protocol: 2,
            launch: launch.clone(),
            answer: Answer::Answered,
        };
        assert!(response.validate(&launch).is_err());
        response.protocol = 1;
        response.launch.attempt_id.push_str("-foreign");
        assert!(response.validate(&launch).is_err());
        for answer in ["done", "", "Answered", "continue"] {
            assert!(
                serde_json::from_value::<Response>(
                    serde_json::json!({"protocol": 1, "launch": launch, "answer": answer})
                )
                .is_err()
            );
        }
    }
}
