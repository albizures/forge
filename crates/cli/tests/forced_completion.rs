use forge_app::{
    Application, NewRecord, NewTypedRecord, Project, ProjectRepository, RecordRepository,
    RecordType, WorkspaceRepository,
};
use forge_server::{Daemon, GitInspector, router};
use forge_store::SqliteProjectRepository;
use std::process::{Command, Stdio};

// Keep the complete cross-type CLI matrix together so each fixture exercises the same contract.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn operator_force_is_noninteractive_inspectable_and_idempotent_for_every_type() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("forge.sqlite");
    let store = SqliteProjectRepository::open(&database).unwrap();
    store.create_workspace("test", "Test", root.path()).unwrap();
    let other_root = tempfile::tempdir().unwrap();
    store
        .create_workspace("other", "Other", other_root.path())
        .unwrap();
    let store = store.in_record_workspace("test").unwrap();
    store
        .register(&Project {
            reference: "test/alpha".into(),
            workspace_id: "test".into(),
            slug: "alpha".into(),
            project_root: root.path().to_path_buf(),
            origin_url: "https://example.test/alpha.git".into(),
        })
        .unwrap();
    let app = Application::new(
        Box::new(SqliteProjectRepository::open(&database).unwrap()),
        Box::new(GitInspector),
    )
    .with_workspace_repository(Box::new(SqliteProjectRepository::open(&database).unwrap()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/rpc", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router(Daemon::new(app)))
            .await
            .unwrap();
    });
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(args)
            .args(["--server-url", &endpoint])
            .current_dir(root.path())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    for kind in [
        RecordType::Generic,
        RecordType::Spec,
        RecordType::Task,
        RecordType::Wayfinder,
        RecordType::Grilling,
        RecordType::Initiative,
    ] {
        for started in [false, true] {
            let record = match kind {
                RecordType::Generic => store
                    .open_record(&NewRecord {
                        body: "Retire obsolete work".into(),
                        project_slugs: vec![],
                        generated_by_ids: vec![],
                    })
                    .unwrap(),
                RecordType::Grilling => {
                    let parent = store
                        .open_typed_record(&NewTypedRecord {
                            record_type: RecordType::Wayfinder,
                            body: "Decision map".into(),
                            project_slugs: vec![],
                            generated_by_ids: vec![],
                        })
                        .unwrap();
                    store
                        .open_grilling_under_wayfinder("Old decision", parent.id, None, vec![])
                        .unwrap()
                }
                _ => store
                    .open_typed_record(&NewTypedRecord {
                        record_type: kind,
                        body: "Retire obsolete work".into(),
                        project_slugs: if kind == RecordType::Spec {
                            vec!["test/alpha".into()]
                        } else {
                            vec![]
                        },
                        generated_by_ids: vec![],
                    })
                    .unwrap(),
            };
            if started {
                store.start_record(record.id).unwrap();
            }
            let dependent = if kind == RecordType::Task {
                let dependent = store
                    .open_typed_record(&NewTypedRecord {
                        record_type: RecordType::Task,
                        body: "Ordinary dependent".into(),
                        project_slugs: vec![],
                        generated_by_ids: vec![],
                    })
                    .unwrap();
                store
                    .relate_records(
                        dependent.id,
                        record.id,
                        forge_app::RecordRelationshipKind::DependsOn,
                    )
                    .unwrap();
                Some(dependent.id)
            } else {
                None
            };
            let id = record.id.to_string();
            let before_record = store.show_record(record.id).unwrap();
            let before = store.list_record_updates(record.id).unwrap();
            // An existing foreign Workspace must not grant access by global Record ID,
            // even when retrying a completion. A missing Workspace also stays rejected.
            for workspace in ["missing", "other"] {
                assert!(
                    !run(&[
                        "--workspace",
                        workspace,
                        "record",
                        "done",
                        &id,
                        "--force",
                        "--resolution",
                        "Wrong scope",
                        "--json"
                    ])
                    .status
                    .success()
                );
                assert_eq!(store.show_record(record.id).unwrap(), before_record);
                assert_eq!(store.list_record_updates(record.id).unwrap(), before);
                assert!(store.forced_completion(record.id).unwrap().is_none());
            }
            for resolution in [None, Some(" \t")] {
                let mut args = vec!["record", "done", &id, "--force", "--json"];
                if let Some(resolution) = resolution {
                    args.extend(["--resolution", resolution]);
                }
                assert!(!run(&args).status.success());
                assert_eq!(store.list_record_updates(record.id).unwrap(), before);
            }
            let output = run(&[
                "record",
                "done",
                &id,
                "--force",
                "--resolution",
                "Superseded by replacement work",
                "--json",
            ]);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let done: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(done["status"], "done");
            assert_eq!(
                done["dependency_consequences"]["newly_eligible_record_ids"],
                serde_json::json!(dependent.into_iter().collect::<Vec<_>>())
            );
            assert!(
                done["dependency_consequences"]["warning"]
                    .as_str()
                    .unwrap()
                    .contains("may dispatch")
            );
            let response: forge_rpc::RecordResponse = serde_json::from_value(done.clone()).unwrap();
            let readable_result = forge_cli::render_record(&response);
            assert!(readable_result.contains("Warning: Normal reconciliation may dispatch"));
            assert!(readable_result.contains(if dependent.is_some() {
                "Newly workflow-eligible dependent IDs:"
            } else {
                "No newly workflow-eligible dependents."
            }));
            assert_eq!(done["forced_completion"]["actor"], "operator");
            assert_eq!(
                done["forced_completion"]["resolution"],
                "Superseded by replacement work"
            );
            let show = run(&["record", "show", &id, "--json"]);
            assert!(
                show.status.success(),
                "{}",
                String::from_utf8_lossy(&show.stderr)
            );
            let show: serde_json::Value = serde_json::from_slice(&show.stdout).unwrap();
            assert_eq!(show["forced_completion"], done["forced_completion"]);
            let readable = run(&["record", "show", &id]);
            assert!(
                String::from_utf8(readable.stdout)
                    .unwrap()
                    .contains("Forced completion by operator")
            );
            assert!(
                !run(&[
                    "--workspace",
                    "other",
                    "record",
                    "done",
                    &id,
                    "--force",
                    "--json"
                ])
                .status
                .success()
            );
            let retry = run(&["record", "done", &id, "--force", "--json"]);
            assert!(retry.status.success());
            let retry: serde_json::Value = serde_json::from_slice(&retry.stdout).unwrap();
            assert_eq!(retry["already_done"], true);
            assert!(retry.get("dependency_consequences").is_none());
            assert!(show.get("dependency_consequences").is_none());
            assert_eq!(retry["forced_completion"], done["forced_completion"]);
            assert_eq!(
                store.list_record_updates(record.id).unwrap().len(),
                before.len() + 1
            );
        }
    }
    server.abort();
}
