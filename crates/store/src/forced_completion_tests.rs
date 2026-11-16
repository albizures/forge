use super::*;
use forge_app::outcomes::AttemptInput;
use rusqlite::params;

fn open(store: &Repository, kind: RecordType) -> Record {
    if kind == RecordType::Generic {
        return store
            .open_record(&NewRecord {
                body: "Administrative fixture".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
    }
    if kind == RecordType::Grilling {
        let parent = open(store, RecordType::Wayfinder);
        return store
            .open_grilling_under_wayfinder("Decision", parent.id, None, vec![])
            .unwrap();
    }
    store
        .open_typed_record(&NewTypedRecord {
            record_type: kind,
            body: "Administrative fixture".into(),
            project_slugs: if kind == RecordType::Spec {
                vec!["alpha".into()]
            } else {
                vec![]
            },
            generated_by_ids: vec![],
        })
        .unwrap()
}

#[test]
fn force_closes_every_type_from_open_or_in_progress_without_start_or_evidence_changes() {
    for kind in [
        RecordType::Generic,
        RecordType::Spec,
        RecordType::Task,
        RecordType::Wayfinder,
        RecordType::Grilling,
        RecordType::Initiative,
    ] {
        for started in [false, true] {
            let store = SqliteProjectRepository::in_memory().unwrap();
            store.register(&project("alpha", "/force-alpha")).unwrap();
            let record = open(&store, kind);
            if started {
                store.start_record(record.id).unwrap();
            }
            let before = store.show_record(record.id).unwrap();
            let updates = store.list_record_updates(record.id).unwrap();
            let checks = if kind == RecordType::Task {
                store.list_record_checks(record.id).unwrap()
            } else {
                vec![]
            };
            let result = store
                .force_done_record(record.id, Some("Obsolete; preserve evidence"), "operator")
                .unwrap();
            assert!(!result.already_done);
            let mut expected = before;
            expected.status = RecordStatus::Done;
            expected.completion_resolution = Some("Obsolete; preserve evidence".into());
            assert_eq!(result.record, expected);
            let audit = store.forced_completion(record.id).unwrap().unwrap();
            assert_eq!(audit.actor, "operator");
            assert_eq!(audit.resolution, "Obsolete; preserve evidence");
            assert_eq!(
                audit.bypassed_gates.iter().any(|g| g.code == "not-started"),
                !started
            );
            assert_eq!(
                store.list_record_updates(record.id).unwrap().len(),
                updates.len() + 1
            );
            assert_eq!(
                store.list_record_updates(record.id).unwrap()[0].id.0,
                audit.update_id
            );
            if kind == RecordType::Task {
                assert_eq!(store.list_record_checks(record.id).unwrap(), checks);
            }
            let frozen = store.list_record_updates(record.id).unwrap();
            for resolution in [None, Some(""), Some("different")] {
                assert!(
                    store
                        .force_done_record(record.id, resolution, "other")
                        .unwrap()
                        .already_done
                );
                assert_eq!(
                    store.forced_completion(record.id).unwrap(),
                    Some(audit.clone())
                );
                assert_eq!(store.list_record_updates(record.id).unwrap(), frozen);
            }
        }
    }
}

#[test]
fn missing_resolution_and_actor_are_nonmutating_and_normal_done_is_never_relabelled() {
    let store = SqliteProjectRepository::in_memory().unwrap();
    let record = open(&store, RecordType::Generic);
    let updates = store.list_record_updates(record.id).unwrap();
    for resolution in [None, Some(""), Some(" \n\t")] {
        assert_eq!(
            store
                .force_done_record(record.id, resolution, "operator")
                .unwrap_err()
                .code,
            "invalid-resolution"
        );
        assert_eq!(store.list_record_updates(record.id).unwrap(), updates);
        assert_eq!(store.show_record(record.id).unwrap(), record);
        assert!(store.forced_completion(record.id).unwrap().is_none());
    }
    assert_eq!(
        store
            .force_done_record(record.id, Some("close"), " ")
            .unwrap_err()
            .code,
        "invalid-actor"
    );
    store.start_record(record.id).unwrap();
    store.done_record(record.id, Some("ordinary")).unwrap();
    let updates = store.list_record_updates(record.id).unwrap();
    let result = store
        .force_done_record(record.id, Some("replacement"), "operator")
        .unwrap();
    assert!(result.already_done);
    assert_eq!(
        result.record.completion_resolution.as_deref(),
        Some("ordinary")
    );
    assert!(store.forced_completion(record.id).unwrap().is_none());
    assert_eq!(store.list_record_updates(record.id).unwrap(), updates);
}

#[test]
fn legacy_invalid_plan_open_parent_dependencies_and_children_are_observed_not_changed() {
    let store = SqliteProjectRepository::in_memory().unwrap();
    store.register(&project("alpha", "/force-alpha")).unwrap();
    let parent = open(&store, RecordType::Spec);
    let first = store
        .open_task_under_spec("First", parent.id, vec![])
        .unwrap();
    let second = store
        .open_task_under_spec("Second", parent.id, vec![])
        .unwrap();
    store
        .relate_records(second.id, first.id, RecordRelationshipKind::DependsOn)
        .unwrap();
    let checks = store.list_record_checks(second.id).unwrap();
    let relationships = store.show_record(second.id).unwrap().relationships;
    store
        .force_done_record(second.id, Some("Superseded"), "operator")
        .unwrap();
    let gates = store
        .forced_completion(second.id)
        .unwrap()
        .unwrap()
        .bypassed_gates;
    for code in [
        "not-started",
        "spec-execution-blocked",
        "dependencies-not-done",
        "checks-not-passed",
        "dispatched-completion-forbidden",
    ] {
        assert!(gates.iter().any(|g| g.code == code), "{code}: {gates:?}");
    }
    assert_eq!(store.list_record_checks(second.id).unwrap(), checks);
    assert_eq!(
        store.show_record(second.id).unwrap().relationships,
        relationships
    );
    assert_eq!(
        store.show_record(first.id).unwrap().status,
        RecordStatus::Open
    );
    // Legacy plan remains invalid even though its second child was administratively closed.
    store
        .force_done_record(parent.id, Some("Retire obsolete plan"), "operator")
        .unwrap();
    let gates = store
        .forced_completion(parent.id)
        .unwrap()
        .unwrap()
        .bypassed_gates;
    assert!(gates.iter().any(|g| g.code == "invalid-spec-plan"));
    assert!(gates.iter().any(|g| g.code == "children-not-done"));
    assert_eq!(
        store.show_record(first.id).unwrap().status,
        RecordStatus::Open
    );
}

#[test]
fn manual_attempt_blocks_force_without_status_or_audit_mutation() {
    let store = SqliteProjectRepository::in_memory().unwrap();
    let task = check_tests::task(&store);
    store.start_record(task.id).unwrap();
    let check = store.list_record_checks(task.id).unwrap()[0].id;
    check_tests::attempt(&store, task.id, check, "open-manual");
    let before = store.list_record_updates(task.id).unwrap();
    assert_eq!(
        store
            .force_done_record(task.id, Some("Obsolete"), "operator")
            .unwrap_err()
            .code,
        "attempt-active"
    );
    assert_eq!(store.list_record_updates(task.id).unwrap(), before);
    assert!(store.forced_completion(task.id).unwrap().is_none());
    assert_eq!(
        store.show_record(task.id).unwrap().status,
        RecordStatus::InProgress
    );
}

#[test]
fn closure_and_override_evidence_roll_back_together_on_insert_failure() {
    let store = SqliteProjectRepository::in_memory().unwrap();
    let record = open(&store, RecordType::Generic);
    let before = store.list_record_updates(record.id).unwrap();
    store.connection.lock().unwrap().execute_batch("CREATE TRIGGER reject_force BEFORE INSERT ON forced_completions BEGIN SELECT RAISE(ABORT, 'injected audit failure'); END;").unwrap();
    assert_eq!(
        store
            .force_done_record(record.id, Some("obsolete"), "operator")
            .unwrap_err()
            .code,
        "storage"
    );
    assert_eq!(store.show_record(record.id).unwrap(), record);
    assert_eq!(store.list_record_updates(record.id).unwrap(), before);
    assert!(store.forced_completion(record.id).unwrap().is_none());
}

#[test]
fn persisted_force_survives_reload_and_old_normal_snapshots_have_no_override() {
    let path = std::env::temp_dir().join(format!(
        "forge-force-{}-{}.sqlite",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SqliteProjectRepository::open(&path).unwrap();
    let normal = open(&store, RecordType::Generic);
    store.start_record(normal.id).unwrap();
    store.done_record(normal.id, None).unwrap();
    // An old snapshot needs explicit upgrade; historical Record evidence is preserved.
    store
        .connection
        .lock()
        .unwrap()
        .execute_batch("DROP TABLE forced_completions; PRAGMA user_version=10")
        .unwrap();
    drop(store);
    assert!(SqliteProjectRepository::open(&path).is_err());
    crate::upgrade::migrate(&path).unwrap();
    let store = SqliteProjectRepository::open(&path).unwrap();
    assert!(store.forced_completion(normal.id).unwrap().is_none());
    let forced = open(&store, RecordType::Generic);
    store
        .force_done_record(forced.id, Some("Superseded"), "operator")
        .unwrap();
    let audit = store.forced_completion(forced.id).unwrap();
    drop(store);
    let store = SqliteProjectRepository::open(&path).unwrap();
    assert_eq!(store.forced_completion(forced.id).unwrap(), audit);
    assert_eq!(
        store.show_record(forced.id).unwrap().status,
        RecordStatus::Done
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

fn project_task(store: &Repository) -> Record {
    store.register(&project("alpha", "/force-alpha")).unwrap();
    store
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Task,
            body: "Retained work".into(),
            project_slugs: vec!["alpha".into()],
            generated_by_ids: vec![],
        })
        .unwrap()
}

#[test]
fn ownership_and_retained_resources_block_but_safely_retired_dispatched_work_can_close() {
    for state in ["active", "unknown", "retained", "retired"] {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let task = project_task(&store);
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "old".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        if state != "active" {
            let c = store.connection.lock().unwrap();
            c.execute("UPDATE agent_assignments SET closed_at_millis = 1, closure_reason = ?1 WHERE id = 'old'", [if state == "unknown" { "lost" } else { "retired" }]).unwrap();
            if state == "retained" {
                c.execute_batch("INSERT INTO agent_assignment_resources (assignment_id, resource_kind, resource_id, created_at_millis) VALUES ('old','agent','retained-agent',1)").unwrap();
            }
        }
        let before = store.list_record_updates(task.id).unwrap();
        let result = store.force_done_record(task.id, Some("Obsolete"), "operator");
        if state == "retired" {
            assert_eq!(
                result.unwrap().record.execution_mode,
                forge_app::ExecutionMode::Dispatched
            );
        } else {
            assert_eq!(result.unwrap_err().code, "execution-mode-unsafe-ownership");
            assert_eq!(store.show_record(task.id).unwrap(), task);
            assert_eq!(store.list_record_updates(task.id).unwrap(), before);
            assert!(store.forced_completion(task.id).unwrap().is_none());
        }
    }
}

#[test]
fn retained_checkout_claims_and_holds_are_not_cleaned_or_reinterpreted_as_manual_mode() {
    for state in ["reserving", "cleanup-required", "active"] {
        let store = SqliteProjectRepository::in_memory().unwrap();
        let task = project_task(&store);
        {
            let c = store.connection.lock().unwrap();
            c.execute("INSERT INTO worktree_claims (record_id, project_slug, path, state) VALUES (?1, 'alpha', '/retained-checkout', ?2)", params![task.id.cast_signed(), state]).unwrap();
            c.execute("INSERT INTO dispatch_holds(record_id, reason) VALUES (?1, 'Historical operator hold')", [task.id.cast_signed()]).unwrap();
        }
        let claim = store.worktree_claim(task.id).unwrap();
        let result =
            store.force_done_record(task.id, Some("Retire work, not checkout"), "operator");
        if state == "active" {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert_eq!(result.unwrap_err().code, "worktree-unsafe");
            assert_eq!(store.show_record(task.id).unwrap(), task);
            assert!(store.forced_completion(task.id).unwrap().is_none());
        }
        assert_eq!(store.worktree_claim(task.id).unwrap(), claim);
        assert!(
            store
                .connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM dispatch_holds WHERE record_id = ?1)",
                    [task.id.cast_signed()],
                    |r| r.get::<_, bool>(0)
                )
                .unwrap()
        );
    }
}

fn snapshot(store: &Repository) -> Vec<(String, Vec<Vec<rusqlite::types::Value>>)> {
    let c = store.connection.lock().unwrap();
    let mut names = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT IN ('records','record_updates','forced_completions','sqlite_sequence') ORDER BY name").unwrap();
    let names = names
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    names
        .into_iter()
        .map(|name| {
            let mut stmt = c
                .prepare(&format!("SELECT * FROM \"{name}\" ORDER BY rowid"))
                .unwrap();
            let count = stmt.column_count();
            let rows = stmt
                .query_map([], |r| {
                    (0..count).map(|i| r.get(i)).collect::<Result<Vec<_>, _>>()
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            (name, rows)
        })
        .collect()
}

#[test]
fn failed_checks_findings_stack_and_all_associated_tables_are_preserved() {
    use forge_app::{
        checks::Verdict,
        findings::{FindingRaiseInput, Severity},
        stacks::{StackDestination, StackSource},
    };
    let store = SqliteProjectRepository::in_memory().unwrap();
    store.register(&project("alpha", "/force-alpha")).unwrap();
    store
        .replace_check_template(
            "alpha",
            &forge_app::check_templates::TemplateReplacement {
                expected_version: 0,
                actor: "operator".into(),
                message: "Require publication".into(),
                criteria: ["Implementation", "Publication"]
                    .into_iter()
                    .map(|name| forge_app::check_templates::CheckCriterion {
                        name: name.into(),
                        description: format!("Verify {name}"),
                        profile_revision: None,
                    })
                    .collect(),
            },
        )
        .unwrap();
    let task = project_task(&store);
    check_tests::manual(&store, task.id);
    store
        .configure_task_stack(
            task.id,
            &StackSource::Root {
                destination: StackDestination {
                    repository: "acme/test".into(),
                    remote: "origin".into(),
                    branch: "main".into(),
                },
            },
        )
        .unwrap();
    // No output or publication exists. A pending Check and a blocking Finding are independent gates.
    store
        .raise_record_finding(
            task.id,
            &FindingRaiseInput {
                severity: Severity::Major,
                blocking: None,
                check_id: None,
                assessment_id: None,
                actor: "reviewer".into(),
                message: "Unresolved risk".into(),
                revision: None,
                dirty: None,
            },
        )
        .unwrap();
    let before = snapshot(&store);
    store
        .force_done_record(
            task.id,
            Some("Accept closure without certifying checks"),
            "operator",
        )
        .unwrap();
    assert_eq!(snapshot(&store), before);
    let gates = store
        .forced_completion(task.id)
        .unwrap()
        .unwrap()
        .bypassed_gates;
    for code in ["stack-blocked", "checks-not-passed", "completion-blocked"] {
        assert!(gates.iter().any(|g| g.code == code), "{gates:?}");
    }
    // A separate failed assessment stays failed, with its exact immutable history intact.
    let task = check_tests::task(&store);
    check_tests::assess(&store, task.id, Verdict::Fail);
    let checks = store.list_record_checks(task.id).unwrap();
    let history = store
        .show_record_check(task.id, checks[0].id, None, 100)
        .unwrap();
    store
        .force_done_record(task.id, Some("Superseded failed work"), "operator")
        .unwrap();
    assert_eq!(store.list_record_checks(task.id).unwrap(), checks);
    assert_eq!(
        store
            .show_record_check(task.id, checks[0].id, None, 100)
            .unwrap(),
        history
    );
}

#[test]
fn force_racing_assignment_claim_cannot_leave_done_with_live_ownership() {
    use std::sync::{Arc, Barrier};
    for n in 0..12 {
        let path = std::env::temp_dir().join(format!(
            "forge-force-claim-{}-{n}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteProjectRepository::open(&path).unwrap();
        let task = project_task(&store);
        let other = SqliteProjectRepository::open(&path).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let b = barrier.clone();
        let complete = std::thread::spawn(move || {
            b.wait();
            store.force_done_record(task.id, Some("Obsolete"), "operator")
        });
        let claim = std::thread::spawn(move || {
            barrier.wait();
            other.create_agent_assignment(&NewAgentAssignment {
                id: "racing-claim".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
        });
        let completed = complete.join().unwrap();
        let claimed = claim.join().unwrap();
        assert_ne!(
            completed.is_ok(),
            claimed.is_ok(),
            "{completed:?} {claimed:?}"
        );
        let store = SqliteProjectRepository::open(&path).unwrap();
        assert_eq!(
            store.forced_completion(task.id).unwrap().is_some(),
            completed.is_ok()
        );
        assert_eq!(
            !store.list_agent_assignments(task.id).unwrap().is_empty(),
            claimed.is_ok()
        );
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn force_racing_manual_attempt_never_leaves_a_done_record_with_open_authority() {
    use std::sync::{Arc, Barrier};
    for n in 0..12 {
        let path = std::env::temp_dir().join(format!(
            "forge-force-race-{}-{n}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteProjectRepository::open(&path).unwrap();
        let task = check_tests::task(&store);
        store.start_record(task.id).unwrap();
        let input = AttemptInput {
            id: "racing-attempt".into(),
            check_id: store.list_record_checks(task.id).unwrap()[0].id,
            generation: 1,
            evidence: check_tests::evidence(),
            actor: "operator".into(),
        };
        let other = SqliteProjectRepository::open(&path).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let b = barrier.clone();
        let complete = std::thread::spawn(move || {
            b.wait();
            store.force_done_record(task.id, Some("Obsolete"), "operator")
        });
        let attempt = std::thread::spawn(move || {
            barrier.wait();
            other.begin_record_attempt(task.id, &input)
        });
        let completed = complete.join().unwrap();
        let begun = attempt.join().unwrap();
        assert_ne!(completed.is_ok(), begun.is_ok(), "{completed:?} {begun:?}");
        let store = SqliteProjectRepository::open(&path).unwrap();
        assert_eq!(
            store.show_record(task.id).unwrap().status == RecordStatus::Done,
            completed.is_ok()
        );
        assert_eq!(
            store.forced_completion(task.id).unwrap().is_some(),
            completed.is_ok()
        );
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
}
