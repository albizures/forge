use super::*;
use forge_app::checks::{AssessmentInput, Verdict};
use forge_rpc::{
    DoneRecordRequest, OpenTaskRequest, RecordIdRequest, SelectedRecordRequest,
    TaskCheckAssessRequest, TaskCheckShowRequest,
};

pub(super) fn pass_checks(repository: &dyn forge_app::RecordRepository, task: u64) {
    for check in repository.list_task_checks(task).unwrap() {
        repository
            .assess_task_check(
                task,
                check.id,
                &AssessmentInput {
                    expected_version: check.version,
                    verdict: Verdict::Pass,
                    actor: "test operator".into(),
                    message: "Fixture deliverables verified".into(),
                    revision: None,
                    dirty: None,
                },
            )
            .unwrap();
    }
}

fn selected<T>(record: T) -> SelectedRecordRequest<T> {
    SelectedRecordRequest::selected("test", record)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn checks_round_trip_over_typed_rpc_and_preserve_workspace_bounds() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, router(Daemon::new(super::test_application())))
            .await
            .unwrap();
    });
    let client = HttpClientBuilder::default()
        .build(format!("http://{address}/rpc"))
        .unwrap();
    let task = client
        .open_task(selected(OpenTaskRequest {
            body: "Deliver work".into(),
            project_slug: None,
            task_kind: Some("research".into()),
            generated_by_ids: vec![],
        }))
        .await
        .unwrap();
    let check = client
        .task_check_list(selected(RecordIdRequest { id: task.id }))
        .await
        .unwrap()
        .remove(0);
    assert_eq!(check.name, "Implementation");
    client
        .start(selected(RecordIdRequest { id: task.id }))
        .await
        .unwrap();
    assert!(
        client
            .done(selected(DoneRecordRequest {
                id: task.id,
                completion_resolution: None
            }))
            .await
            .is_err()
    );
    let input = AssessmentInput {
        expected_version: check.version,
        verdict: Verdict::Pass,
        actor: "reviewer".into(),
        message: "Deliverables verified".into(),
        revision: None,
        dirty: Some(false),
    };
    let passed = client
        .task_check_assess(selected(TaskCheckAssessRequest {
            task: task.id,
            check: check.id,
            assessment: input.clone(),
        }))
        .await
        .unwrap();
    assert_eq!(passed.version, check.version + 1);
    let stale = client
        .task_check_assess(selected(TaskCheckAssessRequest {
            task: task.id,
            check: check.id,
            assessment: input.clone(),
        }))
        .await
        .unwrap_err();
    assert!(stale.to_string().contains("stale-version"));
    let page = client
        .task_check_show(selected(TaskCheckShowRequest {
            task: task.id,
            check: check.id,
            after: None,
            limit: 1,
        }))
        .await
        .unwrap();
    assert_eq!(page.assessments.len(), 1);
    assert_eq!(page.check, passed);
    let wrong_workspace =
        SelectedRecordRequest::selected("missing", RecordIdRequest { id: task.id });
    assert!(client.task_check_list(wrong_workspace).await.is_err());
    let updates = client
        .updates(selected(RecordIdRequest { id: task.id }))
        .await
        .unwrap();
    assert_eq!(
        updates.updates[0].assessment_id,
        Some(page.assessments[0].id)
    );
    client
        .done(selected(DoneRecordRequest {
            id: task.id,
            completion_resolution: None,
        }))
        .await
        .unwrap();
    let frozen = client
        .task_check_assess(selected(TaskCheckAssessRequest {
            task: task.id,
            check: check.id,
            assessment: AssessmentInput {
                expected_version: passed.version,
                ..input
            },
        }))
        .await
        .unwrap_err();
    assert!(frozen.to_string().contains("record-done"));
    server.abort();
}
