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
    async fn bootstrap(
        &self,
        request: AssignmentCredentials,
    ) -> RpcResult<AssignmentBootstrapResponse> {
        let result = self
            .assignment_session(&request.assignment_id, &request.secret)?
            .bootstrap();
        self.delivery.publish(&self.application);
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
        self.delivery.publish(&self.application);
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
        self.delivery.publish(&self.application);
        self.assignment_record_response(result.map_err(rpc_error)?)
    }
    async fn done(&self, request: AssignmentDoneRequest) -> RpcResult<AssignmentRecordResponse> {
        let result = self
            .assignment_session(&request.assignment_id, &request.secret)?
            .done(request.resolution.as_deref(), request.human.as_ref());
        self.delivery.publish(&self.application);
        self.assignment_record_response(result.map_err(rpc_error)?)
    }
}
