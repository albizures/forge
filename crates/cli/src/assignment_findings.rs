//! Capability-bound Finding client; no operator routing or caller Task/actor fields.
use forge_rpc::{
    AssignmentApiClient, AssignmentFindingListRequest, AssignmentFindingMutationRequest,
    AssignmentFindingRaiseRequest, AssignmentFindingShowRequest, FindingMutationInput,
    FindingRaiseInput,
};

/// Own-Task Finding operations. Caller attribution is never transmitted.
pub enum AssignmentFindingAction {
    List {
        after: Option<u64>,
        limit: u32,
        compare_revision: Option<String>,
    },
    Show {
        finding: u64,
        after: Option<u64>,
        limit: u32,
        compare_revision: Option<String>,
    },
    Raise(FindingRaiseInput),
    Mutate {
        finding: u64,
        input: FindingMutationInput,
    },
}

/// Execute a typed request using protected Assignment delivery credentials.
/// # Errors
/// Returns delivery failures or redacted stable domain categories.
/// # Panics
/// Panics if a typed Finding response cannot be serialized.
pub async fn assignment_finding(
    action: AssignmentFindingAction,
) -> Result<serde_json::Value, String> {
    let assignment_id =
        std::env::var("FORGE_ASSIGNMENT_ID").map_err(|_| "missing Assignment ID")?;
    let endpoint = std::env::var("FORGE_ASSIGNMENT_DAEMON_URL")
        .map_err(|_| "missing Assignment daemon URL")?;
    let file = std::env::var_os("FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE")
        .ok_or("missing Assignment secret file")?;
    let secret = super::read_assignment_secret(file)?;
    let client = super::client(&format!("{}/rpc", endpoint.trim_end_matches('/')))
        .map_err(|_| "invalid Assignment daemon URL")?;
    let result = match action {
        AssignmentFindingAction::List {
            after,
            limit,
            compare_revision,
        } => client
            .finding_list(AssignmentFindingListRequest {
                assignment_id,
                secret,
                after,
                limit,
                compare_revision,
            })
            .await
            .map(|v| serde_json::to_value(v).expect("serializable Findings")),
        AssignmentFindingAction::Show {
            finding,
            after,
            limit,
            compare_revision,
        } => client
            .finding_show(AssignmentFindingShowRequest {
                assignment_id,
                secret,
                finding,
                after,
                limit,
                compare_revision,
            })
            .await
            .map(|v| serde_json::to_value(v).expect("serializable Finding page")),
        AssignmentFindingAction::Raise(input) => client
            .finding_raise(AssignmentFindingRaiseRequest {
                assignment_id,
                secret,
                severity: input.severity,
                blocking: input.blocking,
                check_id: input.check_id,
                assessment_id: input.assessment_id,
                message: input.message,
                revision: input.revision,
                dirty: input.dirty,
            })
            .await
            .map(|v| serde_json::to_value(v).expect("serializable Finding")),
        AssignmentFindingAction::Mutate { finding, input } => client
            .finding_mutate(AssignmentFindingMutationRequest {
                assignment_id,
                secret,
                finding,
                change: input.change,
                expected_version: input.explanation.expected_version,
                message: input.explanation.message,
                revision: input.explanation.revision,
                dirty: input.explanation.dirty,
            })
            .await
            .map(|v| serde_json::to_value(v).expect("serializable Finding")),
    };
    result.map_err(|error| {
        // Never echo arbitrary server messages, payloads, endpoints or credentials.
        let category = if let jsonrpsee::core::client::Error::Call(error) = error {
            error
                .data()
                .and_then(|data| serde_json::from_str::<String>(data.get()).ok())
        } else {
            None
        };
        let category = category.as_deref().filter(|code| {
            matches!(
                *code,
                "assignment-capability-denied"
                    | "finding-not-found"
                    | "invalid-finding-origin"
                    | "invalid-record-type"
                    | "record-done"
                    | "stale-version"
                    | "invalid-transition"
                    | "invalid-finding"
                    | "invalid-revision"
                    | "invalid-page"
            )
        });
        format!(
            "Assignment Finding request failed{}",
            category.map_or_else(String::new, |c| format!(": {c}"))
        )
    })
}
