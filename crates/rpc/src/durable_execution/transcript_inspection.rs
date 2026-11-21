//! Read-only internal transcript subprocess contract, not execution authority.
use crate::{TranscriptEntry, TranscriptHistory};
use forge_app::{RegistrationError, durable_execution::Launch};
use serde::{Deserialize, Serialize};

pub const MAX_REQUEST_BYTES: usize = 16384;
/// 100 entries at the runner's 8 KiB content bound, plus identity and JSON overhead.
pub const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u32,
    pub launch: Launch,
    pub after: Option<String>,
    pub limit: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub protocol: u32,
    pub launch: Launch,
    pub history: TranscriptHistory,
    #[serde(default)]
    pub code: Option<String>,
    pub entries: Vec<TranscriptEntry>,
    pub next_after: Option<String>,
    pub more: bool,
}

impl Response {
    /// # Errors
    /// Rejects foreign, incompatible, unordered or oversized observations.
    pub fn validate(&self, request: &Request) -> Result<(), RegistrationError> {
        let invalid = || {
            RegistrationError::new(
                "runner-transcript-inspection-invalid",
                "expected exact original runtime and an ordered bounded committed transcript page",
            )
        };
        let id = |e: &TranscriptEntry| e.id.parse::<u64>().ok().filter(|v| *v > 0);
        let floor = request.after.as_deref().map_or(Some(0), |a| a.parse().ok());
        let mut previous = floor.ok_or_else(invalid)?;
        for entry in &self.entries {
            let current = id(entry).ok_or_else(invalid)?;
            if current <= previous || entry.content.len() > 8192 {
                return Err(invalid());
            }
            previous = current;
        }
        let unavailable = self.history == TranscriptHistory::Unavailable;
        if self.protocol != super::RUNNER_PROTOCOL_VERSION
            || self.launch != request.launch
            || self.entries.len() > request.limit as usize
            || (self.history != TranscriptHistory::Available
                && (!self.entries.is_empty() || self.more))
            || unavailable != self.code.is_some()
            || self.code.as_deref().is_some_and(|c| c.len() > 64)
            || self.next_after
                != self
                    .entries
                    .last()
                    .map(|e| e.id.clone())
                    .or(request.after.clone())
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Launch, Request) {
        let value: serde_json::Value =
            serde_json::from_str(include_str!("../../../../runner/test/wire-v1.json")).unwrap();
        let launch: Launch = serde_json::from_value(value["operation"]["launch"].clone()).unwrap();
        let request = Request {
            protocol: 1,
            launch: launch.clone(),
            after: Some("4".into()),
            limit: 2,
        };
        (launch, request)
    }
    fn entry(id: &str) -> TranscriptEntry {
        TranscriptEntry {
            id: id.into(),
            kind: "k".into(),
            by_task_id: None,
            content: "{}".into(),
            truncated: false,
        }
    }
    fn response(request: &Request, ids: &[&str]) -> Response {
        let entries: Vec<_> = ids.iter().map(|id| entry(id)).collect();
        Response {
            protocol: 1,
            launch: request.launch.clone(),
            history: TranscriptHistory::Available,
            code: None,
            next_after: entries
                .last()
                .map(|e| e.id.clone())
                .or(request.after.clone()),
            entries,
            more: false,
        }
    }

    #[test]
    fn transcript_wire_accepts_only_ordered_bounded_exact_pages() {
        let (_, request) = fixture();
        response(&request, &["5", "9"]).validate(&request).unwrap();
        response(&request, &[]).validate(&request).unwrap();
        for ids in [
            &["5", "5"][..],
            &["9", "5"],
            &["4"],
            &["3"],
            &["5", "6", "7"],
            &["x"],
            &["0"],
        ] {
            assert!(
                response(&request, ids).validate(&request).is_err(),
                "{ids:?}"
            );
        }
        let mut foreign = response(&request, &["5"]);
        foreign.launch.attempt_id.push_str("-foreign");
        assert!(foreign.validate(&request).is_err());
        let mut protocol = response(&request, &["5"]);
        protocol.protocol = 2;
        assert!(protocol.validate(&request).is_err());
        let mut lying_cursor = response(&request, &["5"]);
        lying_cursor.next_after = Some("4".into());
        assert!(lying_cursor.validate(&request).is_err());
        let mut oversized = response(&request, &["5"]);
        oversized.entries[0].content = "x".repeat(8193);
        assert!(oversized.validate(&request).is_err());
    }

    #[test]
    fn unavailable_history_requires_a_code_and_never_carries_entries() {
        let (_, request) = fixture();
        let mut unavailable = response(&request, &[]);
        unavailable.history = TranscriptHistory::Unavailable;
        assert!(unavailable.validate(&request).is_err(), "code required");
        unavailable.code = Some("runner-history-unavailable".into());
        unavailable.validate(&request).unwrap();
        unavailable.entries = vec![entry("5")];
        unavailable.next_after = Some("5".into());
        assert!(unavailable.validate(&request).is_err());
        let mut available_with_code = response(&request, &[]);
        available_with_code.code = Some("x".into());
        assert!(available_with_code.validate(&request).is_err());
        assert!(
            serde_json::from_value::<Response>(
                serde_json::json!({"protocol": 1, "extra": "PRIVATE-CANARY"})
            )
            .is_err()
        );
    }
}
