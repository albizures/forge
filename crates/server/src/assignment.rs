use super::{Daemon, rpc_error};
use forge_rpc::{
    AssignmentApiServer, AssignmentBootstrapResponse, AssignmentCommentRequest,
    AssignmentCommentResponse, AssignmentContextResponse, AssignmentCredentials,
    AssignmentDoneRequest, AssignmentPageRequest, AssignmentPageResponse, AssignmentReadRequest,
    AssignmentRecordResponse, RecordCommentResponse, RecordUpdateResponse,
};
use jsonrpsee::core::{RpcResult, async_trait};

impl Daemon {
    pub(super) fn assignment_session<'a>(
        &'a self,
        id: &'a str,
        secret: &'a str,
    ) -> RpcResult<forge_app::assignment::AuthenticatedAssignment<'a>> {
        let root = self.capability_secret_root.as_deref().ok_or_else(|| {
            rpc_error(forge_app::RegistrationError::new(
                "assignment-capability-denied",
                "Assignment delivery unavailable",
            ))
        })?;
        self.application
            .assignment(root, id, secret)
            .map_err(rpc_error)
    }

    fn assignment_record_response(
        &self,
        record: forge_app::Record,
    ) -> RpcResult<AssignmentRecordResponse> {
        Ok(AssignmentRecordResponse {
            schema_version: 1,
            record: record.into(),
            freshness: self.application.record_update_head().map_err(rpc_error)?.0,
        })
    }
}

#[async_trait]
impl AssignmentApiServer for Daemon {
    async fn finding_list(
        &self,
        r: forge_rpc::AssignmentFindingListRequest,
    ) -> RpcResult<forge_rpc::FindingList> {
        self.assignment_session(&r.assignment_id, &r.secret)?
            .finding_list(r.after, r.limit, r.compare_revision.as_deref())
            .map_err(rpc_error)
    }
    async fn finding_show(
        &self,
        r: forge_rpc::AssignmentFindingShowRequest,
    ) -> RpcResult<forge_rpc::FindingPage> {
        self.assignment_session(&r.assignment_id, &r.secret)?
            .finding_show(r.finding, r.after, r.limit, r.compare_revision.as_deref())
            .map_err(rpc_error)
    }
    async fn finding_raise(
        &self,
        r: forge_rpc::AssignmentFindingRaiseRequest,
    ) -> RpcResult<forge_rpc::Finding> {
        self.assignment_session(&r.assignment_id, &r.secret)?
            .finding_raise(&forge_rpc::FindingRaiseInput {
                severity: r.severity,
                blocking: r.blocking,
                check_id: r.check_id,
                assessment_id: r.assessment_id,
                actor: String::new(),
                message: r.message,
                revision: r.revision,
                dirty: r.dirty,
            })
            .map_err(rpc_error)
    }
    async fn finding_mutate(
        &self,
        r: forge_rpc::AssignmentFindingMutationRequest,
    ) -> RpcResult<forge_rpc::Finding> {
        self.assignment_session(&r.assignment_id, &r.secret)?
            .finding_mutate(
                r.finding,
                &forge_rpc::FindingMutationInput {
                    change: r.change,
                    explanation: forge_rpc::FindingResolveInput {
                        expected_version: r.expected_version,
                        actor: String::new(),
                        message: r.message,
                        revision: r.revision,
                        dirty: r.dirty,
                    },
                },
            )
            .map_err(rpc_error)
    }
    async fn check_list(
        &self,
        r: forge_rpc::AssignmentCheckListRequest,
    ) -> RpcResult<Vec<forge_rpc::RecordCheck>> {
        self.assignment_session(&r.assignment_id, &r.secret)?
            .check_list(r.compare_revision.as_deref())
            .map_err(rpc_error)
    }
    async fn check_show(
        &self,
        r: forge_rpc::AssignmentCheckShowRequest,
    ) -> RpcResult<forge_rpc::CheckPage> {
        self.assignment_session(&r.assignment_id, &r.secret)?
            .check_show(r.check, r.after, r.limit, r.compare_revision.as_deref())
            .map_err(rpc_error)
    }
    async fn check_assess(
        &self,
        r: forge_rpc::AssignmentCheckAssessRequest,
    ) -> RpcResult<forge_rpc::RecordCheck> {
        self.assignment_session(&r.assignment_id, &r.secret)?
            .check_assess(
                r.check,
                &forge_rpc::AssessmentInput {
                    actor: String::new(),
                    expected_version: r.expected_version,
                    verdict: r.verdict,
                    message: r.message,
                    revision: r.revision,
                    dirty: r.dirty,
                },
            )
            .map_err(rpc_error)
    }
    async fn check_invalidate(
        &self,
        r: forge_rpc::AssignmentCheckInvalidateRequest,
    ) -> RpcResult<forge_rpc::RecordCheck> {
        self.assignment_session(&r.assignment_id, &r.secret)?
            .check_invalidate(
                r.check,
                &forge_rpc::InvalidationInput {
                    actor: String::new(),
                    expected_version: r.expected_version,
                    assessment_id: r.assessment_id,
                    message: r.message,
                    revision: r.revision,
                    dirty: r.dirty,
                },
            )
            .map_err(rpc_error)
    }
    async fn bootstrap(
        &self,
        request: AssignmentCredentials,
    ) -> RpcResult<AssignmentBootstrapResponse> {
        let result = self
            .assignment_session(&request.assignment_id, &request.secret)?
            .bootstrap();
        result
            .map(|value| AssignmentBootstrapResponse {
                schema_version: 1,
                context: value.context.into(),
                behavior: value.behavior,
            })
            .map_err(rpc_error)
    }
    async fn context(
        &self,
        request: AssignmentCredentials,
    ) -> RpcResult<AssignmentContextResponse> {
        self.assignment_session(&request.assignment_id, &request.secret)?
            .context()
            .map(Into::into)
            .map_err(rpc_error)
    }
    async fn record(&self, request: AssignmentReadRequest) -> RpcResult<AssignmentRecordResponse> {
        let record = self
            .assignment_session(&request.assignment_id, &request.secret)?
            .record(request.target)
            .map_err(rpc_error)?;
        self.assignment_record_response(record)
    }
    async fn comments(
        &self,
        request: AssignmentPageRequest,
    ) -> RpcResult<AssignmentPageResponse<RecordCommentResponse>> {
        let page = self
            .assignment_session(&request.assignment_id, &request.secret)?
            .comments(request.target, request.cursor.as_deref(), request.limit)
            .map_err(rpc_error)?;
        Ok(AssignmentPageResponse {
            schema_version: 1,
            items: page.items.into_iter().map(Into::into).collect(),
            next_cursor: page.next_cursor,
            freshness: page.freshness,
        })
    }
    async fn history(
        &self,
        request: AssignmentPageRequest,
    ) -> RpcResult<AssignmentPageResponse<RecordUpdateResponse>> {
        let page = self
            .assignment_session(&request.assignment_id, &request.secret)?
            .history(request.target, request.cursor.as_deref(), request.limit)
            .map_err(rpc_error)?;
        Ok(AssignmentPageResponse {
            schema_version: 1,
            items: page.items.into_iter().map(Into::into).collect(),
            next_cursor: page.next_cursor,
            freshness: page.freshness,
        })
    }
    async fn comment(
        &self,
        request: AssignmentCommentRequest,
    ) -> RpcResult<AssignmentCommentResponse> {
        let result = self
            .assignment_session(&request.assignment_id, &request.secret)?
            .comment(&request.body);
        result
            .map(|comment| AssignmentCommentResponse {
                schema_version: 1,
                comment: comment.into(),
            })
            .map_err(rpc_error)
    }
    async fn start(&self, request: AssignmentCredentials) -> RpcResult<AssignmentRecordResponse> {
        let result = self
            .assignment_session(&request.assignment_id, &request.secret)?
            .start();
        self.assignment_record_response(result.map_err(rpc_error)?)
    }
    async fn done(&self, request: AssignmentDoneRequest) -> RpcResult<AssignmentRecordResponse> {
        let result = self
            .assignment_session(&request.assignment_id, &request.secret)?
            .done(request.resolution.as_deref(), request.human.as_ref());
        self.assignment_record_response(result.map_err(rpc_error)?)
    }
}
