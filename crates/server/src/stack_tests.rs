use super::*;
use forge_app::stacks::{PullRequestObservation, StackDestination, StackHosting, StackSource};
use forge_rpc::{AssignmentApiClient, StackDestination as Destination, TaskStackAction as Action, TaskStackRequest as Request};

struct Hosting;
impl StackHosting for Hosting {
    fn checkout(&self, _: &Path) -> Result<forge_app::stacks::CheckoutObservation, RegistrationError> {
        Ok(forge_app::stacks::CheckoutObservation { commit: "a".repeat(40), clean: true })
    }
    fn fetch_branch(&self, _: &Path, _: &StackDestination, branch: &str) -> Result<String, RegistrationError> {
        assert_eq!(branch, "release/2.x");
        Ok("a".repeat(40))
    }
    fn pull_request(&self, _: &Path, _: &str, _: &str) -> Result<PullRequestObservation, RegistrationError> {
        Err(RegistrationError::new("test", "PR is unavailable"))
    }
}

fn selected(record_id: u64, action: Action) -> forge_rpc::SelectedRecordRequest<Request> {
    forge_rpc::SelectedRecordRequest::selected("test", Request { record_id, action })
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn stack_configuration_and_preparation_round_trip_through_scoped_rpc() {
    let root = std::env::temp_dir().join(format!("forge-stack-rpc-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&root).unwrap();
    let app = super::test_application().with_stack_hosting(Box::new(Hosting));
    app.register_project(&root, "test/alpha").unwrap();
    app.replace_check_template("test/alpha", &forge_app::check_templates::TemplateReplacement {
        expected_version: 0, actor: "operator".into(), message: "Require publication".into(),
        criteria: ["Implementation", "Publication"].map(|name| forge_app::check_templates::CheckCriterion {
            name: name.into(), description: name.into(), profile_revision: None,
        }).into(),
    }).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router(Daemon::new(app))).await.unwrap(); });
    let client = HttpClientBuilder::default().build(format!("http://{address}/rpc")).unwrap();
    let task = client.open_task(forge_rpc::SelectedRecordRequest::selected("test", forge_rpc::OpenTaskRequest {
        body: "Publish a slice".into(), project_slug: Some("alpha".into()), generated_by_ids: vec![],
    })).await.unwrap();
    client.execution_mode_set(forge_rpc::SelectedRecordRequest::selected("test", forge_rpc::SetExecutionModeRequest {
        id: task.id, expected: forge_app::ExecutionMode::Dispatched, mode: forge_app::ExecutionMode::Manual,
        actor: "operator".into(), message: "Manual stack fixture".into(),
    })).await.unwrap();
    assert!(client.task_stack(selected(task.id, Action::Show)).await.unwrap().is_none());
    let source = StackSource::Root { destination: Destination { repository: "acme/widgets".into(), remote: "origin".into(), branch: "release/2.x".into() } };
    let configured = client.task_stack(selected(task.id, Action::Configure { source })).await.unwrap().unwrap();
    assert_eq!(configured.base_branch, "release/2.x");
    assert!(configured.base_commit.is_none());
    let prepared = client.task_stack(selected(task.id, Action::Prepare)).await.unwrap().unwrap();
    assert_eq!(prepared.base_commit, Some("a".repeat(40)));
    assert_eq!(client.task_stack(selected(task.id, Action::Show)).await.unwrap(), Some(prepared));
    assert!(client.task_stack(forge_rpc::SelectedRecordRequest::selected("missing", Request { record_id: task.id, action: Action::Show })).await.is_err());
    assert!(client.task_stack(selected(task.id, Action::Publish { url: "https://github.com/acme/widgets/pull/1".into() })).await.is_err());
    let secret = "must-not-appear-in-response";
    let denied = client.stack(forge_rpc::AssignmentStackRequest { assignment_id: "unknown".into(), secret: secret.into(), url: None, unchanged: false }).await.unwrap_err().to_string();
    assert!(!denied.contains(secret));
    assert!(client.task_stack(selected(task.id, Action::PublishUnchanged)).await.is_err());
    forge_rpc::RecordApiClient::start(&client, forge_rpc::SelectedRecordRequest::selected("test", forge_rpc::RecordIdRequest { id: task.id })).await.unwrap();
    let checks = client.record_check_list(forge_rpc::SelectedRecordRequest::selected("test", forge_rpc::RecordCheckListRequest { record: task.id, compare_revision: None })).await.unwrap();
    for (position, check) in checks.iter().enumerate() {
        let evidence = forge_app::outcomes::Evidence::Repository { repository: root.display().to_string(), commit: "a".repeat(40) };
        let attempt = forge_app::outcomes::AttemptInput { id: format!("rpc-{position}"), check_id: check.id, generation: 1, evidence: evidence.clone(), actor: "operator".into() };
        client.record_attempt_begin(forge_rpc::SelectedRecordRequest::selected("test", forge_rpc::RecordAttemptBeginRequest { record: task.id, attempt: attempt.clone() })).await.unwrap();
        if position == 1 {
            let verified = client.task_stack(selected(task.id, Action::PublishUnchanged)).await.unwrap().unwrap();
            assert!(verified.publication.is_none());
            assert!(verified.unchanged_result.is_some());
        }
        let outcome = forge_app::outcomes::OutcomeInput { attempt_id: attempt.id, generation: 1, evidence,
            verdict: forge_app::checks::Verdict::Pass, actor: "operator".into(), message: "Checked".into(),
            disposition: forge_app::outcomes::OutcomeDisposition::Complete, findings: vec![], verifications: vec![] };
        client.record_outcome_submit(forge_rpc::SelectedRecordRequest::selected("test", forge_rpc::RecordOutcomeSubmitRequest { record: task.id, outcome })).await.unwrap();
    }
    forge_rpc::RecordApiClient::done(&client, forge_rpc::SelectedRecordRequest::selected("test", forge_rpc::DoneRecordRequest { id: task.id, completion_resolution: None, force: false })).await.unwrap();
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(root).unwrap();
}
