use super::*;
use forge_app::{PrerequisiteKind, storage_upgrade::WaitsForDecision};
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};

fn fixture(schema: u32) -> Connection {
    let c = reference(schema).unwrap();
    c.pragma_update(None, "user_version", schema).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    c.execute_batch("INSERT INTO workspaces VALUES('w','Workspace','/w');
        INSERT INTO projects VALUES('p','/w/p','git@example:p','w','p');
        INSERT INTO projects VALUES('q','/w/q','git@example:q','w','q');
        INSERT INTO records(id,record_type,body,status,workspace_id) VALUES
        (10,'spec','Spec','open','w'),(1,'task','Root','open','w'),
        (2,'task','Middle','open','w'),(3,'task','Leaf','open','w'),
        (4,'task','Ambiguous','open','w'),(5,'task','Standalone','open','w'),
        (6,'task','Cross Project','open','w'),(7,'task','Projectless','open','w'),
        (20,'spec','Other Spec','open','w'),(30,'wayfinder','Map','open','w'),(31,'grilling','Decision','open','w');
        INSERT INTO record_projects VALUES(10,'p'),(1,'p'),(2,'p'),(3,'p'),(4,'p'),(5,'p'),(6,'q'),(20,'p'),(30,'p'),(31,'p');
        INSERT INTO record_relationships(from_id,to_id,parent_id,child_id,kind) VALUES
        (1,10,10,1,'parent-child'),(2,10,10,2,'parent-child'),
        (3,10,10,3,'parent-child'),(4,10,10,4,'parent-child'),(31,30,30,31,'parent-child');
        INSERT INTO record_relationships(from_id,to_id,kind) VALUES
        (2,1,'depends-on'),(3,2,'depends-on'),(3,1,'depends-on'),
        (4,1,'depends-on'),(5,1,'depends-on'),(6,5,'depends-on'),(7,6,'depends-on'),
        (10,20,'depends-on'),(5,31,'depends-on');
        INSERT INTO record_updates(id,record_id,created_at_millis,kind,related_record_id,relationship_kind)
        VALUES(100,2,123,'relationship-added',1,'depends-on');").unwrap();
    if schema >= 8 {
        c.execute_batch(r#"INSERT INTO task_stacks(record_id,predecessor,source_json) VALUES
            (1,NULL,'{"kind":"root","destination":{"repository":"owner/repo","remote":"origin","branch":"feature/input"}}'),
            (2,1,'{"kind":"predecessor","record_id":1}'),
            (3,2,'{"kind":"predecessor","record_id":2}');"#).unwrap();
    }
    if schema >= 9 {
        c.execute_batch(
            "INSERT INTO record_relationships(from_id,to_id,kind) VALUES(4,2,'depends-on');
            INSERT INTO ordering_only_dependencies(from_id,to_id,kind) VALUES(4,2,'depends-on')",
        )
        .unwrap();
    }
    c
}

fn kind(c: &Connection, from: u64, to: u64) -> Option<PrerequisiteKind> {
    crate::load_record(c, from)
        .unwrap()
        .relationships
        .into_iter()
        .find(|r| r.from_id == from && r.to_id == to)
        .unwrap()
        .prerequisite_kind()
}

#[test]
fn historical_matrix_converts_only_explicit_or_provable_intent_and_preserves_history() {
    for schema in 7..=12 {
        let mut c = fixture(schema);
        let before = inspection(&c).unwrap();
        let failure = migrate_connection(&mut c).unwrap_err();
        assert!(failure.message.contains("4 -> 1"), "{}", failure.message);
        assert_eq!(inspection(&c).unwrap(), before);
        let mut decisions = vec![WaitsForDecision {
            from_id: 4,
            to_id: 1,
        }];
        if schema == 7 {
            for (from_id, to_id) in [(2, 1), (3, 2), (3, 1)] {
                decisions.push(WaitsForDecision { from_id, to_id });
            }
        }
        let after = migrate_decisions(&mut c, &decisions).unwrap();
        assert_eq!(after.schema, Some(20));
        for (from, to) in [(5, 1), (6, 5), (7, 6), (4, 1), (3, 1), (10, 20), (5, 31)] {
            assert_eq!(kind(&c, from, to), Some(PrerequisiteKind::WaitsFor));
        }
        for (from, to) in [(2, 1), (3, 2)] {
            assert_eq!(
                kind(&c, from, to),
                Some(if schema == 7 {
                    PrerequisiteKind::WaitsFor
                } else {
                    PrerequisiteKind::BuildsOn
                })
            );
        }
        if schema >= 9 {
            assert_eq!(kind(&c, 4, 2), Some(PrerequisiteKind::WaitsFor));
        }
        for old in before.tables {
            if !matches!(
                old.name.as_str(),
                "task_stacks" | "ordering_only_dependencies" | "builds_on_dependencies"
            ) {
                assert!(
                    after.tables.contains(&old),
                    "schema {schema}: changed {}",
                    old.name
                );
            }
        }
        assert_eq!(migrate_connection(&mut c).unwrap(), after);
    }
}

#[test]
fn mixed_twelve_topology_retains_canonical_links_and_absorbs_legacy_links() {
    let mut c = fixture(12);
    let old = stacks::load(&c, 3).unwrap();
    c.execute_batch("INSERT INTO builds_on_dependencies(from_id,to_id) VALUES(2,1); UPDATE task_stacks SET predecessor=NULL,source_json='{}' WHERE record_id=2").unwrap();
    let before = inspection(&c).unwrap();
    migrate_decisions(
        &mut c,
        &[WaitsForDecision {
            from_id: 4,
            to_id: 1,
        }],
    )
    .unwrap();
    assert_eq!(stacks::load(&c, 3).unwrap(), old);
    assert_eq!(kind(&c, 2, 1), Some(PrerequisiteKind::BuildsOn));
    assert_eq!(kind(&c, 3, 2), Some(PrerequisiteKind::BuildsOn));
    assert_eq!(kind(&c, 3, 1), Some(PrerequisiteKind::WaitsFor));
    let history = before
        .tables
        .into_iter()
        .find(|t| t.name == "record_updates")
        .unwrap();
    assert!(inspection(&c).unwrap().tables.contains(&history));
}

#[test]
fn incomplete_open_chains_survive_conversion_and_same_schema_snapshot_transfer() {
    for schema in [12, 13] {
        for missing in ["record_id=1", "record_id=2", "1=1"] {
            let mut c = fixture(12);
            migrate_decisions(
                &mut c,
                &[WaitsForDecision {
                    from_id: 4,
                    to_id: 1,
                }],
            )
            .unwrap();
            c.execute(&format!("DELETE FROM task_stacks WHERE {missing}"), [])
                .unwrap();
            c.execute_batch("DROP TABLE assignment_prompt_intents; DROP TABLE assignment_nudges; DROP TABLE retained_work_recoveries; DROP TABLE check_continuation_resets; DROP TABLE check_continuations; DROP TABLE assignment_sessions")
                .unwrap();
            c.pragma_update(None, "user_version", schema).unwrap();
            let dir = std::env::temp_dir().join(format!(
                "forge-incomplete-upgrade-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&dir).unwrap();
            let source = dir.join("source.sqlite");
            let target = dir.join("target.sqlite");
            c.backup(rusqlite::MAIN_DB, &source, None).unwrap();
            let bytes = fs::read(&source).unwrap();
            let before = inspect(&source).unwrap();
            assert!(before.blockers.is_empty());
            assert_eq!(snapshot(&source, &target).unwrap(), before);
            let after = migrate(&target).unwrap();
            assert_eq!(after.schema, Some(20));
            for table in &before.tables {
                assert!(after.tables.contains(table));
            }
            assert_eq!(
                after
                    .tables
                    .iter()
                    .find(|t| t.name == "assignment_sessions")
                    .unwrap()
                    .rows,
                0
            );
            assert_eq!(migrate(&target).unwrap(), after);
            assert_eq!(fs::read(&source).unwrap(), bytes);
            let upgraded = Connection::open(&target).unwrap();
            assert_eq!(kind(&upgraded, 2, 1), Some(PrerequisiteKind::BuildsOn));
            // Upgrade permits planning, not execution or evidence invention.
            assert!(stacks::load(&upgraded, 3).is_err());
            drop(upgraded);
            fs::remove_dir_all(dir).unwrap();
        }
    }
}

#[test]
fn incomplete_lineage_cannot_hide_preparation_results_or_invalid_configuration() {
    for schema in [12, 13] {
        for invalid in [
            "UPDATE task_stacks SET base_commit='pinned' WHERE record_id=3",
            "UPDATE task_stacks SET publication_json='{}' WHERE record_id=3",
            "UPDATE task_stacks SET source_json='invalid' WHERE record_id=3",
            "UPDATE records SET status='in_progress' WHERE id=3",
            "INSERT INTO task_unchanged_results VALUES(3,'{}')",
            "INSERT INTO record_work(record_id,output_json) VALUES(3,'{}')",
        ] {
            let mut c = fixture(12);
            migrate_decisions(
                &mut c,
                &[WaitsForDecision {
                    from_id: 4,
                    to_id: 1,
                }],
            )
            .unwrap();
            c.execute_batch("DELETE FROM task_stacks WHERE record_id=1")
                .unwrap();
            c.execute_batch(invalid).unwrap();
            c.execute_batch("DROP TABLE assignment_prompt_intents; DROP TABLE assignment_nudges; DROP TABLE retained_work_recoveries; DROP TABLE check_continuation_resets; DROP TABLE check_continuations; DROP TABLE assignment_sessions")
                .unwrap();
            c.pragma_update(None, "user_version", schema).unwrap();
            let before = inventory(&c).unwrap();
            assert!(inspection(&c).is_err(), "schema {schema}: {invalid}");
            assert!(
                migrate_connection(&mut c).is_err(),
                "schema {schema}: {invalid}"
            );
            assert_eq!(inventory(&c).unwrap(), before);
        }
    }
}

#[test]
fn semantic_stamp_failure_rolls_back_all_converted_rows_and_retry_is_exact() {
    let mut c = fixture(11);
    let before = inspection(&c).unwrap();
    let decisions = [WaitsForDecision {
        from_id: 4,
        to_id: 1,
    }];
    c.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Pragma {
            pragma_name: "user_version",
            pragma_value: Some("13"),
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    assert!(migrate_decisions(&mut c, &decisions).is_err());
    c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(inspection(&c).unwrap(), before);
    assert!(migrate_decisions(&mut c, &decisions).is_ok());
}

#[test]
fn decisions_cannot_override_known_intent_or_started_prepared_and_unfinished_work() {
    for sql in [
        "UPDATE records SET status='in_progress' WHERE id=4",
        "UPDATE records SET status='done' WHERE id=4",
        "UPDATE records SET status='done' WHERE id=4; UPDATE records SET status='in_progress' WHERE id=1",
        "UPDATE records SET status='in_progress' WHERE id=4; UPDATE records SET status='done' WHERE id=1",
        "UPDATE records SET status='in_progress' WHERE id=10",
        "INSERT INTO task_stacks(record_id,source_json,base_commit) SELECT 4,source_json,'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' FROM task_stacks WHERE record_id=1",
        // Known intent is not an operator override target.
        "INSERT INTO ordering_only_dependencies(from_id,to_id,kind) VALUES(4,1,'depends-on')",
    ] {
        let mut c = fixture(11);
        c.execute_batch(sql).unwrap();
        let before = inspection(&c).unwrap();
        assert!(
            migrate_decisions(
                &mut c,
                &[WaitsForDecision {
                    from_id: 4,
                    to_id: 1
                }]
            )
            .is_err()
        );
        assert_eq!(inspection(&c).unwrap(), before);
    }
}

#[test]
fn explicit_completed_waits_for_decisions_preserve_history_and_require_both_done() {
    for schema in 7..=12 {
        for parent_status in ["open", "in_progress", "done"] {
            let mut c = fixture(schema);
            // Use completed Tasks without Stack lineage; the existing chain still
            // exercises normal lineage conversion alongside this explicit decision.
            c.execute_batch("INSERT INTO records(id,record_type,body,status,workspace_id)
                VALUES(8,'task','Completed prerequisite','done','w');
                INSERT INTO record_projects VALUES(8,'p');
                INSERT INTO record_relationships(from_id,to_id,parent_id,child_id,kind)
                VALUES(8,10,10,8,'parent-child');
                DELETE FROM record_relationships WHERE from_id=4 AND to_id=1 AND kind='depends-on';
                INSERT INTO record_relationships(from_id,to_id,kind) VALUES(4,8,'depends-on');
                UPDATE records SET status='done',completion_resolution='Retained completion' WHERE id=4;")
                .unwrap();
            c.execute("UPDATE records SET status=?1 WHERE id=10", [parent_status])
                .unwrap();
            let before = inspection(&c).unwrap();
            // Done alone is not enough: the operator must supply the decision.
            assert!(migrate_connection(&mut c).is_err());
            assert_eq!(inspection(&c).unwrap(), before);
            if schema == 7 {
                // Schema 7 has no Stack metadata; exclude its unrelated
                // ambiguous Open edges when exercising a started parent.
                c.execute(
                    "DELETE FROM record_relationships WHERE kind='depends-on' AND from_id IN (2,3)",
                    [],
                )
                .unwrap();
            }
            let before = inspection(&c).unwrap();
            let after = migrate_decisions(
                &mut c,
                &[WaitsForDecision {
                    from_id: 4,
                    to_id: 8,
                }],
            )
            .unwrap();
            assert_eq!(kind(&c, 4, 8), Some(PrerequisiteKind::WaitsFor));
            for table in before.tables {
                if !matches!(
                    table.name.as_str(),
                    "task_stacks" | "ordering_only_dependencies" | "builds_on_dependencies"
                ) {
                    assert!(after.tables.contains(&table), "changed {}", table.name);
                }
            }
            assert_eq!(migrate_connection(&mut c).unwrap(), after);
        }
    }
}

#[test]
fn cycles_scope_and_conflicting_designations_fail_without_partial_conversion() {
    for sql in [
        "INSERT INTO record_relationships(from_id,to_id,kind) VALUES(1,3,'depends-on'); INSERT INTO ordering_only_dependencies(from_id,to_id,kind) VALUES(1,3,'depends-on')",
        "INSERT INTO task_stacks(record_id,source_json) SELECT 5,source_json FROM task_stacks WHERE record_id=1; INSERT INTO task_stacks(record_id,predecessor,source_json) VALUES(6,5,'{\"kind\":\"predecessor\",\"record_id\":5}')",
        "INSERT INTO ordering_only_dependencies(from_id,to_id,kind) VALUES(2,1,'depends-on')",
    ] {
        let mut c = fixture(11);
        c.execute_batch(sql).unwrap();
        let before = inspection(&c).unwrap();
        assert!(
            migrate_decisions(
                &mut c,
                &[WaitsForDecision {
                    from_id: 4,
                    to_id: 1
                }]
            )
            .is_err()
        );
        assert_eq!(inspection(&c).unwrap(), before);
    }
}
