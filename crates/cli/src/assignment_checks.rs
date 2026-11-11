//! Capability-bound Check client. Credentials never pass through operator routing.
use forge_rpc::{
    AssessmentInput, AssignmentApiClient, AssignmentCheckAssessRequest,
    AssignmentCheckInvalidateRequest, AssignmentCheckListRequest, AssignmentCheckShowRequest,
    InvalidationInput,
};

/// Own-Task Check operation; no Task selector or operator attribution is transmitted.
pub enum AssignmentCheckAction {
    List {
        compare_revision: Option<String>,
    },
    Show {
        check: u64,
        after: Option<u64>,
        limit: u32,
        compare_revision: Option<String>,
    },
    Assess {
        check: u64,
        input: AssessmentInput,
    },
    Invalidate {
        check: u64,
        input: InvalidationInput,
    },
}

/// Execute a typed Assignment Check request using protected delivery credentials.
/// # Errors
/// Returns delivery failures or redacted stable domain categories.
/// # Panics
/// Panics if a typed Check response cannot be serialized to JSON.
pub async fn assignment_check(action: AssignmentCheckAction) -> Result<serde_json::Value, String> {
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
        AssignmentCheckAction::List { compare_revision } => client
            .check_list(AssignmentCheckListRequest {
                assignment_id,
                secret,
                compare_revision,
            })
            .await
            .map(|value| serde_json::to_value(value).expect("serializable Checks")),
        AssignmentCheckAction::Show {
            check,
            after,
            limit,
            compare_revision,
        } => client
            .check_show(AssignmentCheckShowRequest {
                assignment_id,
                secret,
                check,
                after,
                limit,
                compare_revision,
            })
            .await
            .map(|value| serde_json::to_value(value).expect("serializable Check page")),
        AssignmentCheckAction::Assess { check, input } => client
            .check_assess(AssignmentCheckAssessRequest {
                assignment_id,
                secret,
                check,
                expected_version: input.expected_version,
                verdict: input.verdict,
                message: input.message,
                revision: input.revision,
                dirty: input.dirty,
            })
            .await
            .map(|value| serde_json::to_value(value).expect("serializable Check")),
        AssignmentCheckAction::Invalidate { check, input } => client
            .check_invalidate(AssignmentCheckInvalidateRequest {
                assignment_id,
                secret,
                check,
                expected_version: input.expected_version,
                assessment_id: input.assessment_id,
                message: input.message,
                revision: input.revision,
                dirty: input.dirty,
            })
            .await
            .map(|value| serde_json::to_value(value).expect("serializable Check")),
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
                    | "check-not-found"
                    | "invalid-record-type"
                    | "record-done"
                    | "stale-version"
                    | "invalid-transition"
                    | "invalid-assessment"
                    | "invalid-revision"
                    | "invalid-page"
            )
        });
        format!(
            "Assignment Check request failed{}",
            category.map_or_else(String::new, |c| format!(": {c}"))
        )
    })
}
