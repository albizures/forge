use tokio::time;

fn subscription_params(assignment: &str, targets: Vec<u64>) -> serde_json::Value {
    let targets = serde_json::Value::from(targets);
    serde_json::json!({"assignment_id":assignment,"capability_id":"subscription-test-secret","correlation_id":"caller-not-authority","target_record_ids":targets,"kind_filters":[]})
}

fn subscription_manage(assignment: &str, id: u64) -> serde_json::Value {
    serde_json::json!({"assignment_id":assignment,"capability_id":"subscription-test-secret","correlation_id":"caller-not-authority","subscription_id":id})
}

fn subscription_graph(
    store: &SqliteProjectRepository,
    kind: RecordType,
) -> (forge_app::Record, forge_app::Record, Vec<u64>, Vec<u64>) {
    let open = |kind, body: &str| {
        store
            .open_typed_record(&NewTypedRecord {
                record_type: kind,
                task_kind: None,
                body: body.into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap()
    };
    let parent = open(RecordType::Wayfinder, "parent");
    let own = if kind == RecordType::Task {
        store
            .open_task_under_wayfinder("own", parent.id, None, None, vec![])
            .unwrap()
    } else {
        store
            .open_grilling_under_wayfinder("own", parent.id, None, vec![])
            .unwrap()
    };
    let sibling = store
        .open_task_under_wayfinder("sibling", parent.id, None, None, vec![])
        .unwrap();
    let prerequisite = open(RecordType::Task, "prerequisite");
    let dependent = open(RecordType::Task, "dependent");
    let generator = open(RecordType::Task, "generator");
    let generated = open(RecordType::Task, "generated");
    let transitive = open(RecordType::Task, "transitive");
    let shared = open(RecordType::Task, "shared Project only");
    for (from, to, edge) in [
        (dependent.id, own.id, RecordRelationshipKind::DependsOn),
        (own.id, generator.id, RecordRelationshipKind::GeneratedBy),
        (generated.id, own.id, RecordRelationshipKind::GeneratedBy),
        (
            generator.id,
            transitive.id,
            RecordRelationshipKind::GeneratedBy,
        ),
    ] {
        store.relate_records(from, to, edge).unwrap();
    }
    let mut allowed = vec![own.id, parent.id, dependent.id, generator.id, generated.id];
    if kind == RecordType::Task {
        store
            .relate_records(own.id, prerequisite.id, RecordRelationshipKind::DependsOn)
            .unwrap();
        allowed.push(prerequisite.id);
    }
    store
        .create_agent_assignment(&NewAgentAssignment {
            id: "graph-subscription".into(),
            record_id: own.id,
            project_slug: "alpha".into(),
            profile_revision: 1,
        })
        .unwrap();
    (
        own,
        generated,
        allowed,
        vec![sibling.id, transitive.id, shared.id, u64::MAX],
    )
}

#[tokio::test]
async fn production_subscription_targets_equal_task_and_grilling_readable_sets() {
    for kind in [RecordType::Task, RecordType::Grilling] {
        let (fixture, _, _, _, _) = subscription_test_daemon();
        let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
        let (own, generated, allowed, denied) = subscription_graph(&store, kind);
        let daemon = fixture.daemon();
        std::fs::write(
            daemon
                .capability_secret_root
                .as_ref()
                .unwrap()
                .join("graph-subscription"),
            "subscription-test-secret",
        )
        .unwrap();
        for target in allowed {
            let created = assignment_rpc(
                daemon.clone(),
                "agentSubscription.create",
                subscription_params("graph-subscription", vec![target]),
            )
            .await;
            let id = created["result"]["id"]
                .as_u64()
                .unwrap_or_else(|| panic!("{created}"));
            let renewed = assignment_rpc(
                daemon.clone(),
                "agentSubscription.renew",
                subscription_manage("graph-subscription", id),
            )
            .await;
            assert!(renewed.get("result").is_some(), "{renewed}");
            let deleted = assignment_rpc(
                daemon.clone(),
                "agentSubscription.delete",
                subscription_manage("graph-subscription", id),
            )
            .await;
            assert!(deleted.get("result").is_some(), "{deleted}");
        }
        for target in denied {
            let denied = assignment_rpc(
                daemon.clone(),
                "agentSubscription.create",
                subscription_params("graph-subscription", vec![target]),
            )
            .await;
            assert!(denied.get("error").is_some(), "{denied}");
        }
        let created = assignment_rpc(
            daemon.clone(),
            "agentSubscription.create",
            subscription_params("graph-subscription", vec![generated.id]),
        )
        .await;
        let id = created["result"]["id"].as_u64().unwrap();
        let endpoint = format!(
            "/v1/agent-assignments/graph-subscription/record-update-subscriptions/{id}/events"
        );
        let response = router(daemon.clone())
            .oneshot(loopback_request(endpoint.clone()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.into_body().into_data_stream();
        stream.next().await.unwrap().unwrap(); // retry frame, no notifications yet
        store
            .unrelate_records(generated.id, own.id, RecordRelationshipKind::GeneratedBy)
            .unwrap();
        store
            .edit_record(generated.id, "must not be delivered")
            .unwrap();
        daemon.delivery.publish(&daemon.application);
        assert!(
            time::timeout(Duration::from_secs(1), stream.next())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            router(daemon.clone())
                .oneshot(loopback_request(endpoint))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        for method in ["agentSubscription.renew", "agentSubscription.delete"] {
            assert!(
                assignment_rpc(
                    daemon.clone(),
                    method,
                    subscription_manage("graph-subscription", id)
                )
                .await
                .get("error")
                .is_some()
            );
        }
    }
}

#[tokio::test]
async fn production_subscription_expiry_and_deletion_end_idle_delivery() {
    for expire in [true, false] {
        let (fixture, assignment, subscription, _, _) = subscription_test_daemon();
        let daemon = fixture.daemon();
        let params = subscription_manage(&assignment, subscription);
        let renewed =
            assignment_rpc(daemon.clone(), "agentSubscription.renew", params.clone()).await;
        assert!(renewed.get("result").is_some(), "{renewed}");
        let endpoint = format!(
            "/v1/agent-assignments/{assignment}/record-update-subscriptions/{subscription}/events"
        );
        let response = router(daemon.clone())
            .oneshot(loopback_request(endpoint.clone()))
            .await
            .unwrap();
        let mut stream = response.into_body().into_data_stream();
        stream.next().await.unwrap().unwrap();
        stream.next().await.unwrap().unwrap();
        if expire {
            // Arrange an expired durable lease without a 30-minute wall-clock test.
            rusqlite::Connection::open(&fixture.database_path).unwrap().execute(
                "UPDATE record_update_subscriptions SET lease_expires_at_millis = 0 WHERE id = ?1", [i64::try_from(subscription).unwrap()]
            ).unwrap();
        } else {
            let deleted =
                assignment_rpc(daemon.clone(), "agentSubscription.delete", params.clone()).await;
            assert!(deleted.get("result").is_some(), "{deleted}");
        }
        assert!(
            time::timeout(Duration::from_secs(1), stream.next())
                .await
                .unwrap()
                .is_none()
        );
        let status = router(daemon.clone())
            .oneshot(loopback_request(endpoint))
            .await
            .unwrap()
            .status();
        assert_eq!(
            status,
            if expire {
                StatusCode::GONE
            } else {
                StatusCode::NOT_FOUND
            }
        );
        for method in ["agentSubscription.renew", "agentSubscription.delete"] {
            assert!(
                assignment_rpc(daemon.clone(), method, params.clone())
                    .await
                    .get("error")
                    .is_some()
            );
        }
    }
}

#[tokio::test]
async fn subscription_replay_is_bounded_and_requires_authoritative_resync() {
    let (fixture, assignment, subscription, _, target) = subscription_test_daemon();
    let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
    for index in 0..257 {
        store
            .edit_record(target, &format!("Update {index}"))
            .unwrap();
    }
    let endpoint = format!(
        "/v1/agent-assignments/{assignment}/record-update-subscriptions/{subscription}/events"
    );
    let response = router(fixture.daemon())
        .oneshot(loopback_request(endpoint))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

async fn verify_subscription_header_auth(daemon: &Daemon, endpoint: &str, subscription: u64) {
    for method in ["agentSubscription.renew", "agentSubscription.delete"] {
        let denied = assignment_rpc(
            daemon.clone(),
            method,
            subscription_manage("target-assignment", subscription),
        )
        .await;
        assert!(denied.get("error").is_some(), "{denied}");
    }
    for header in [
        None,
        Some("Bearer incorrect"),
        Some("Bearer owner-assignment"),
    ] {
        let mut request = loopback_request(endpoint.into());
        request.headers_mut().remove(header::AUTHORIZATION);
        if let Some(header) = header {
            request
                .headers_mut()
                .insert(header::AUTHORIZATION, header.parse().unwrap());
        }
        assert_eq!(
            router(daemon.clone())
                .oneshot(request)
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn subscription_delivery_rechecks_revocation_closure_and_ownership() {
    for revoke in [true, false] {
        let (fixture, assignment, subscription, start, target) = subscription_test_daemon();
        let daemon = fixture.daemon();
        let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
        let root = daemon.capability_secret_root.as_ref().unwrap();
        std::fs::write(root.join("target-assignment"), "subscription-test-secret").unwrap();
        let foreign = format!(
            "/v1/agent-assignments/target-assignment/record-update-subscriptions/{subscription}/events"
        );
        assert_eq!(
            router(daemon.clone())
                .oneshot(loopback_request(foreign))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let endpoint = format!(
            "/v1/agent-assignments/{assignment}/record-update-subscriptions/{subscription}/events"
        );
        verify_subscription_header_auth(&daemon, &endpoint, subscription).await;
        let response = router(daemon.clone())
            .oneshot(loopback_request(endpoint.clone()))
            .await
            .unwrap();
        let mut stream = response.into_body().into_data_stream();
        stream.next().await.unwrap().unwrap();
        let update = String::from_utf8(stream.next().await.unwrap().unwrap().to_vec()).unwrap();
        assert!(update.contains("record-update"));
        // Reconnect from the acknowledged cursor: old notifications are not repeated.
        let last = store.record_update_head().unwrap().0;
        assert!(last > start);
        let response = router(daemon.clone())
            .oneshot(loopback_request(format!("{endpoint}?after={last}")))
            .await
            .unwrap();
        let mut reconnected = response.into_body().into_data_stream();
        reconnected.next().await.unwrap().unwrap();
        assert!(
            time::timeout(Duration::from_millis(10), reconnected.next())
                .await
                .is_err()
        );
        if !revoke {
            store
                .edit_record(target, "queued before revocation")
                .unwrap();
            daemon.delivery.publish(&daemon.application);
        }
        if revoke {
            std::fs::remove_file(root.join(&assignment)).unwrap();
        } else {
            store
                .close_agent_assignment(
                    &assignment,
                    forge_app::AgentAssignmentClosureReason::Retired,
                )
                .unwrap();
        }
        assert!(
            time::timeout(Duration::from_secs(1), reconnected.next())
                .await
                .unwrap()
                .is_none(),
            "queued notifications must be reauthorized"
        );
        assert_eq!(
            router(daemon.clone())
                .oneshot(loopback_request(endpoint))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        for method in ["agentSubscription.renew", "agentSubscription.delete"] {
            assert!(
                assignment_rpc(
                    daemon.clone(),
                    method,
                    subscription_manage(&assignment, subscription)
                )
                .await
                .get("error")
                .is_some()
            );
        }
        assert!(
            assignment_rpc(
                daemon,
                "agentSubscription.create",
                subscription_params(&assignment, vec![target])
            )
            .await
            .get("error")
            .is_some()
        );
    }
}

#[tokio::test]
async fn subscription_filtered_replay_and_invalid_cursor_controls() {
    let (fixture, assignment, _, _, target) = subscription_test_daemon();
    let daemon = fixture.daemon();
    let mut params = subscription_params(&assignment, vec![target]);
    params["kind_filters"] = serde_json::json!(["comment-added"]);
    let created = assignment_rpc(daemon.clone(), "agentSubscription.create", params.clone()).await;
    let id = created["result"]["id"].as_u64().unwrap();
    let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
    for index in 0..257 {
        store
            .edit_record(target, &format!("filtered {index}"))
            .unwrap();
    }
    let endpoint =
        format!("/v1/agent-assignments/{assignment}/record-update-subscriptions/{id}/events");
    let response = router(daemon.clone())
        .oneshot(loopback_request(endpoint.clone()))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "unselected updates cannot exhaust replay"
    );
    let mut stream = response.into_body().into_data_stream();
    stream.next().await.unwrap().unwrap();
    assert!(
        time::timeout(Duration::from_millis(10), stream.next())
            .await
            .is_err()
    );
    for suffix in [
        format!("?after={}", u64::MAX),
        "?secret=NEVER-IN-A-URL".into(),
    ] {
        let response = router(daemon.clone())
            .oneshot(loopback_request(format!("{endpoint}{suffix}")))
            .await
            .unwrap();
        assert!(!response.status().is_success());
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("NEVER-IN-A-URL"));
    }
    for method in [
        "agentSubscription.create",
        "agentSubscription.renew",
        "agentSubscription.delete",
    ] {
        for secret in [None, Some("wrong"), Some(assignment.as_str())] {
            let mut request = if method.ends_with("create") {
                params.clone()
            } else {
                subscription_manage(&assignment, id)
            };
            if let Some(secret) = secret {
                request["capability_id"] = secret.into();
            } else {
                request.as_object_mut().unwrap().remove("capability_id");
            }
            assert!(
                assignment_rpc(daemon.clone(), method, request)
                    .await
                    .get("error")
                    .is_some()
            );
        }
    }
    params["target_record_ids"] = serde_json::json!([]);
    assert!(
        assignment_rpc(daemon.clone(), "agentSubscription.create", params.clone())
            .await
            .get("error")
            .is_some()
    );
    params["target_record_ids"] = serde_json::json!(vec![target; 101]);
    assert!(
        assignment_rpc(daemon, "agentSubscription.create", params)
            .await
            .get("error")
            .is_some()
    );
}

#[tokio::test]
async fn production_subscriptions_require_actual_delivery_credentials() {
    let (fixture, assignment, subscription, _, target) = subscription_test_daemon();
    let daemon = fixture.daemon();
    let params = serde_json::json!({"assignment_id":assignment,"capability_id":assignment,"correlation_id":"untrusted","target_record_ids":[target],"kind_filters":[]});
    let denied = assignment_rpc(daemon.clone(), "agentSubscription.create", params).await;
    assert!(
        denied.get("error").is_some(),
        "Assignment ID is not a bearer: {denied}"
    );
    let endpoint = format!(
        "/v1/agent-assignments/{assignment}/record-update-subscriptions/{subscription}/events"
    );
    let mut request = loopback_request(endpoint);
    request.headers_mut().remove(header::AUTHORIZATION);
    let response = router(daemon).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
