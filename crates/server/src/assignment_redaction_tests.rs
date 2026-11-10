#[derive(Clone)]
struct AssignmentLogWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for AssignmentLogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn legacy_authenticated_routes_and_normal_logs_do_not_retain_caller_secrets() {
    use tracing::instrument::WithSubscriber;
    let (fixture, assignment, target) = assignment_test_daemon();
    let root = fixture
        .database_path
        .with_extension("PRIVATE-DELIVERY-PATH");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(&assignment), "actual-secret\n").unwrap();
    let mut daemon = fixture.daemon();
    daemon.capability_secret_root = Some(root.clone());
    let logs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer = AssignmentLogWriter(logs.clone());
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(move || writer.clone())
        .finish();
    async {
        tracing::info!("assignment redaction fixture exercised");
        let request = |secret: &str| serde_json::json!({"assignment_id":assignment,"secret":secret,"correlation_id":"actual-secret PRIVATE-DELIVERY-PATH MCP-CREDENTIAL-SENTINEL BEHAVIOR-SENTINEL"});
        for method in ["agentDispatch.assignmentAcknowledge", "agentDispatch.assignmentInstructions", "agentDispatch.assignmentBootstrapFailure", "agentDispatch.assignmentTaskStart"] {
            let mut denied = request("wrong");
            if method.ends_with("Failure") { denied["message"] = "BEHAVIOR-SENTINEL actual-secret".into(); }
            if method.ends_with("Start") { denied["task_id"] = target.into(); }
            let response = assignment_rpc(daemon.clone(), method, denied).await;
            assert!(response.get("error").is_some());
            for sentinel in ["actual-secret", "PRIVATE-DELIVERY-PATH", "MCP-CREDENTIAL-SENTINEL", "BEHAVIOR-SENTINEL"] { assert!(!response.to_string().contains(sentinel)); }
        }
        let ack = assignment_rpc(daemon.clone(), "agentDispatch.assignmentAcknowledge", request("actual-secret")).await;
        assert!(ack.get("result").is_some(), "{ack}");
        let mut failure = request("actual-secret");
        failure["message"] = "BEHAVIOR-SENTINEL actual-secret PRIVATE-DELIVERY-PATH".into();
        let reported = assignment_rpc(daemon.clone(), "agentDispatch.assignmentBootstrapFailure", failure).await;
        assert!(reported.get("result").is_some(), "{reported}");
        let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
        let owner = store.agent_assignment(&assignment).unwrap().record_id;
        let evidence = format!("{:?} {:?}", store.list_assignment_authorization_audits(&assignment).unwrap(), store.list_record_comments(owner).unwrap());
        for sentinel in ["actual-secret", "PRIVATE-DELIVERY-PATH", "MCP-CREDENTIAL-SENTINEL", "BEHAVIOR-SENTINEL"] { assert!(!evidence.contains(sentinel)); }
    }.with_subscriber(subscriber).await;
    let logs = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("assignment redaction fixture exercised"));
    for sentinel in [
        "actual-secret",
        "PRIVATE-DELIVERY-PATH",
        "MCP-CREDENTIAL-SENTINEL",
        "BEHAVIOR-SENTINEL",
    ] {
        assert!(!logs.contains(sentinel));
    }
    std::fs::remove_dir_all(root).unwrap();
}
