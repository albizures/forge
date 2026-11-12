use super::*;
use forge_app::checks::{AssessmentInput, Verdict};
use forge_rpc::{
    DoneRecordRequest, OpenTaskRequest, RecordCheckAssessRequest, RecordCheckShowRequest,
    RecordIdRequest, SelectedRecordRequest,
};

pub(super) fn pass_checks(repository: &dyn forge_app::RecordRepository, record: u64) {
    for check in repository.list_record_checks(record).unwrap() {
        if check.current_verdict == Some(Verdict::Pass) {
            continue;
        }
        if repository.show_record(record).unwrap().status == forge_app::RecordStatus::Open {
            repository.start_record(record).unwrap();
        }
        let work = repository.record_work_state(record).unwrap();
        let evidence = if check.position == 0 {
            work.input
        } else {
            work.output
        }
        .unwrap_or_else(|| forge_app::outcomes::Evidence::Artifact {
            uri: "urn:fixture:deliverable".into(),
            sha256: "a".repeat(64),
        });
        let id = format!("fixture-{record}-{}-{}", check.id, work.generation);
        repository
            .begin_record_attempt(
                record,
                &forge_app::outcomes::AttemptInput {
                    id: id.clone(),
                    check_id: check.id,
                    generation: work.generation,
                    evidence: evidence.clone(),
                    actor: "test operator".into(),
                },
            )
            .unwrap();
        repository
            .submit_record_outcome(
                record,
                &forge_app::outcomes::OutcomeInput {
                    attempt_id: id,
                    generation: work.generation,
                    evidence,
                    verdict: Verdict::Pass,
                    actor: "test operator".into(),
                    message: "Fixture deliverables verified".into(),
                },
            )
            .unwrap();
    }
}

async fn manual_pass(
    client: &jsonrpsee::http_client::HttpClient,
    record: u64,
    check: u64,
    generation: u64,
    id: &str,
) -> forge_app::checks::RecordCheck {
    let evidence = forge_app::outcomes::Evidence::Artifact {
        uri: "urn:rpc:deliverable".into(),
        sha256: "a".repeat(64),
    };
    client
        .record_attempt_begin(selected(forge_rpc::RecordAttemptBeginRequest {
            record,
            attempt: forge_app::outcomes::AttemptInput {
                id: id.into(),
                check_id: check,
                generation,
                evidence: evidence.clone(),
                actor: "operator".into(),
            },
        }))
        .await
        .unwrap();
    client
        .record_outcome_submit(selected(forge_rpc::RecordOutcomeSubmitRequest {
            record,
            outcome: forge_app::outcomes::OutcomeInput {
                attempt_id: id.into(),
                generation,
                evidence,
                verdict: Verdict::Pass,
                actor: "operator".into(),
                message: "Deliverables verified".into(),
            },
        }))
        .await
        .unwrap()
        .check
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
            generated_by_ids: vec![],
        }))
        .await
        .unwrap();
    let check = client
        .record_check_list(selected(forge_rpc::RecordCheckListRequest {
            record: task.id,
            compare_revision: None,
        }))
        .await
        .unwrap()
        .remove(0);
    assert_eq!(check.name, "Implementation");
    assert_eq!(check.record_id, task.id);
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
    let passed = manual_pass(&client, task.id, check.id, 1, "rpc-first").await;
    assert_eq!(passed.version, check.version + 1);
    let stale = client
        .record_check_assess(selected(RecordCheckAssessRequest {
            record: task.id,
            check: check.id,
            assessment: input.clone(),
        }))
        .await
        .unwrap_err();
    assert!(stale.to_string().contains("stale-version"));
    let page = client
        .record_check_show(selected(RecordCheckShowRequest {
            compare_revision: None,
            record: task.id,
            check: check.id,
            after: None,
            limit: 1,
        }))
        .await
        .unwrap();
    assert_eq!(page.assessments.len(), 1);
    assert_eq!(page.check, passed);
    let wrong_workspace = SelectedRecordRequest::selected(
        "missing",
        forge_rpc::RecordCheckListRequest {
            record: task.id,
            compare_revision: None,
        },
    );
    assert!(client.record_check_list(wrong_workspace).await.is_err());
    let updates = client
        .updates(selected(RecordIdRequest { id: task.id }))
        .await
        .unwrap();
    assert_eq!(
        updates.updates[0].assessment_id,
        Some(page.assessments[0].id)
    );
    let invalidate = forge_rpc::RecordCheckInvalidateRequest {
        record: task.id,
        check: check.id,
        invalidation: forge_rpc::InvalidationInput {
            expected_version: passed.version,
            assessment_id: page.assessments[0].id,
            actor: "operator".into(),
            message: "Withdraw verdict".into(),
            revision: None,
            dirty: None,
        },
    };
    assert!(
        client
            .record_check_invalidate(SelectedRecordRequest::selected(
                "missing",
                invalidate.clone()
            ))
            .await
            .is_err()
    );
    let pending = client
        .record_check_invalidate(selected(invalidate.clone()))
        .await
        .unwrap();
    assert_eq!(pending.current_verdict, None);
    assert_eq!(pending.version, 3);
    let stale = client
        .record_check_invalidate(selected(invalidate))
        .await
        .unwrap_err();
    assert!(stale.to_string().contains("stale-version"));
    let summary = client
        .show(selected(RecordIdRequest { id: task.id }))
        .await
        .unwrap();
    assert_eq!(
        summary.checks.unwrap().as_slice(),
        std::slice::from_ref(&pending)
    );
    let page = client
        .record_check_show(selected(RecordCheckShowRequest {
            record: task.id,
            check: check.id,
            after: None,
            limit: 1,
            compare_revision: Some("a".repeat(40)),
        }))
        .await
        .unwrap();
    let next = client
        .record_check_show(selected(RecordCheckShowRequest {
            record: task.id,
            check: check.id,
            after: page.next_cursor,
            limit: 1,
            compare_revision: None,
        }))
        .await
        .unwrap();
    assert_eq!(
        next.activities[0].invalidation.as_ref().unwrap().message,
        "Withdraw verdict"
    );
    assert!(
        client
            .done(selected(DoneRecordRequest {
                id: task.id,
                completion_resolution: None
            }))
            .await
            .is_err()
    );
    let passed = manual_pass(&client, task.id, check.id, 2, "rpc-second").await;
    client
        .done(selected(DoneRecordRequest {
            id: task.id,
            completion_resolution: None,
        }))
        .await
        .unwrap();
    let frozen = client
        .record_check_assess(selected(RecordCheckAssessRequest {
            record: task.id,
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

#[tokio::test]
async fn obsolete_operator_methods_and_inputs_are_rejected_without_writes() {
    let daemon = Daemon::new(super::test_application());
    let created = super::assignment_rpc(
        daemon.clone(),
        "record.openTask",
        serde_json::json!({
            "workspace":"test", "body":"Research deliverables", "generated_by_ids":[]
        }),
    )
    .await;
    let record = created["result"]["id"].as_u64().unwrap();
    assert!(created["result"].get("task_kind").is_none());
    let selector = serde_json::json!({"workspace":"test", "record":record});
    let before =
        super::assignment_rpc(daemon.clone(), "record.recordCheckList", selector.clone()).await;
    assert_eq!(before["result"][0]["record_id"], record);
    assert!(before["result"][0].get("task_id").is_none());
    let updates = super::assignment_rpc(
        daemon.clone(),
        "record.updates",
        serde_json::json!({
            "workspace":"test", "id":record
        }),
    )
    .await;
    for method in [
        "taskCheckList",
        "taskCheckShow",
        "taskCheckAssess",
        "taskCheckInvalidate",
        "taskFindingList",
        "taskFindingShow",
        "taskFindingRaise",
        "taskFindingResolve",
        "taskFindingMutate",
    ] {
        let response = super::assignment_rpc(
            daemon.clone(),
            &format!("record.{method}"),
            selector.clone(),
        )
        .await;
        assert_eq!(response["error"]["code"], -32601, "{method}: {response}");
    }
    for method in ["recordCheckList", "recordFindingList"] {
        let mut params = serde_json::json!({"workspace":"test", "record":record, "task":record});
        if method == "recordFindingList" {
            params["limit"] = 50.into();
        }
        let response =
            super::assignment_rpc(daemon.clone(), &format!("record.{method}"), params).await;
        assert_eq!(response["error"]["code"], -32602, "{method}: {response}");
    }
    for (method, mut params) in [
        (
            "openTask",
            serde_json::json!({"body":"Do not create", "generated_by_ids":[]}),
        ),
        (
            "openTaskUnderSpec",
            serde_json::json!({"body":"Do not create", "spec_id":record, "generated_by_ids":[]}),
        ),
        (
            "openTaskUnderWayfinder",
            serde_json::json!({"body":"Do not create", "wayfinder_id":record, "generated_by_ids":[]}),
        ),
        ("list", serde_json::json!({})),
    ] {
        params["workspace"] = "test".into();
        params["task_kind"] = serde_json::Value::Null;
        let response =
            super::assignment_rpc(daemon.clone(), &format!("record.{method}"), params).await;
        assert_eq!(response["error"]["code"], -32602, "{method}: {response}");
    }
    assert_eq!(
        super::assignment_rpc(daemon.clone(), "record.recordCheckList", selector).await,
        before
    );
    assert_eq!(
        super::assignment_rpc(
            daemon.clone(),
            "record.updates",
            serde_json::json!({
                "workspace":"test", "id":record
            })
        )
        .await,
        updates
    );
    let records = super::assignment_rpc(
        daemon,
        "record.list",
        serde_json::json!({"workspace":"test"}),
    )
    .await;
    assert_eq!(records["result"]["records"].as_array().unwrap().len(), 1);
}
