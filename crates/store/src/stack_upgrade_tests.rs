//! Completed publication -> unchanged review -> prepared successor survives conversion.
use super::*;
use std::{fs, sync::atomic::{AtomicU64, Ordering}};

#[test]
fn legacy_completed_and_prepared_chain_round_trips_without_inventing_evidence() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let store = fixture();
    let first = task(&store);
    let review = task(&store);
    let successor = task(&store);
    store.configure_task_stack(first, &root()).unwrap();
    store.relate_records(review, first, RecordRelationshipKind::BuildsOn).unwrap();
    store.relate_records(successor, review, RecordRelationshipKind::BuildsOn).unwrap();
    pin(&store,first,&"1".repeat(40));
    complete(&store,first,&"a".repeat(40));
    pin(&store,review,&"a".repeat(40));
    store.start_record(review).unwrap();
    pass(&store,review,0,&"a".repeat(40));
    pass(&store,review,1,&"a".repeat(40));
    store.record_stack_unchanged(&store.task_stack(review).unwrap().unwrap(), &forge_app::stacks::UnchangedResult {
        generation:1,
        evidence: Evidence::Repository {repository:store.project("alpha").unwrap().project_root.display().to_string(),commit:"a".repeat(40)},
    },None).unwrap();
    pass(&store,review,2,&"a".repeat(40));
    store.done_record(review,None).unwrap();
    pin(&store,successor,&"a".repeat(40));
    let retired = task(&store);
    store.force_done_record(retired,Some("Retain forced completion"),"operator").unwrap();
    let ids = [first,review,successor,retired];
    let stacks = ids.map(|id| store.task_stack(id).unwrap());
    let checks = ids.map(|id| store.list_record_checks(id).unwrap());
    let work = ids.map(|id| store.record_work_state(id).unwrap());
    let updates = ids.map(|id| store.list_record_updates(id).unwrap());
    let forced = store.forced_completion(retired).unwrap();
    let directory = std::env::temp_dir().join(format!("forge-stack-upgrade-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
    fs::create_dir(&directory).unwrap();
    let source = directory.join("source.db");
    let copy = directory.join("staging.db");
    store.connection.lock().unwrap().backup(rusqlite::MAIN_DB,&source,None).unwrap();
    let c = rusqlite::Connection::open(&source).unwrap();
    // Encode the historical predecessor representation explicitly; production no
    // longer offers that declaration path.
    for (id, predecessor) in [(review, first), (successor, review)] {
        c.execute("UPDATE task_stacks SET predecessor=?2, source_json=?3 WHERE record_id=?1", params![id.cast_signed(), predecessor.cast_signed(), serde_json::to_string(&StackSource::Predecessor { record_id: predecessor }).unwrap()]).unwrap();
    }
    c.execute_batch("DROP TABLE assignment_prompt_intents; DROP TABLE assignment_nudges; DROP TABLE retained_work_recoveries; DROP TABLE assignment_sessions; DROP TABLE builds_on_dependencies; PRAGMA user_version=11;").unwrap();
    drop(c);
    let bytes = fs::read(&source).unwrap();
    assert!(SqliteProjectRepository::open(&source).is_err());
    let before = crate::upgrade::snapshot(&source,&copy).unwrap();
    let after = crate::upgrade::migrate(&copy).unwrap();
    assert_eq!(fs::read(&source).unwrap(),bytes);
    for old in before.tables.into_iter().filter(|t| !t.name.starts_with("check_continuation")) {
        if !matches!(old.name.as_str(),"task_stacks" | "ordering_only_dependencies") {
            assert!(after.tables.contains(&old),"changed {}",old.name);
        }
    }
    for _ in 0..2 {
        let upgraded = SqliteProjectRepository::open(&copy).unwrap();
        for (index,id) in ids.into_iter().enumerate() {
            assert_eq!(upgraded.task_stack(id).unwrap(),stacks[index]);
            assert_eq!(upgraded.list_record_checks(id).unwrap(),checks[index]);
            assert_eq!(upgraded.record_work_state(id).unwrap(),work[index]);
            assert_eq!(upgraded.list_record_updates(id).unwrap(),updates[index]);
        }
        assert_eq!(upgraded.forced_completion(retired).unwrap(),forced);
        // Ordinary persisted authorization consumes the same exact prepared input.
        upgraded.validate_task_execution_plan(successor).unwrap();
    }
    let upgraded = SqliteProjectRepository::open(&copy).unwrap();
    upgraded.start_record(successor).unwrap();
    drop(upgraded);
    // Stale retained preparation is rejected, never repinned by conversion.
    let corrupt = directory.join("corrupt.db");
    fs::copy(&source,&corrupt).unwrap();
    let c = rusqlite::Connection::open(&corrupt).unwrap();
    c.execute("UPDATE task_base_commits SET base_commit=?1 WHERE record_id=?2",params!["b".repeat(40),successor.cast_signed()]).unwrap();
    drop(c);
    let old = crate::upgrade::inspect(&corrupt).unwrap();
    assert!(crate::upgrade::migrate(&corrupt).is_err());
    assert_eq!(crate::upgrade::inspect(&corrupt).unwrap(),old);
    fs::remove_dir_all(directory).unwrap();
}
