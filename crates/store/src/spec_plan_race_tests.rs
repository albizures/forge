//! Deterministic writer ordering: pause an actual public write before commit.
use super::*;
use std::{sync::mpsc, time::Duration};

struct Database(std::path::PathBuf);
impl Database {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "forge-spec-plan-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fixture()
            .connection
            .lock()
            .unwrap()
            .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        Self(path)
    }
    fn open(&self) -> SqliteProjectRepository {
        SqliteProjectRepository::open(&self.0).unwrap()
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[derive(Clone, Copy, Debug)]
enum Mutation {
    Child,
    Dependency,
    OrderingOnly,
    Remove,
    Stack,
}
impl Mutation {
    fn apply(
        self,
        store: &SqliteProjectRepository,
        parent: u64,
        first: u64,
        second: u64,
    ) -> Result<(), forge_app::RegistrationError> {
        match self {
            Self::Child => store
                .open_task_under_spec("Competing child", parent, vec![])
                .map(|_| ()),
            Self::Dependency => store
                .relate_records(second, first, RecordRelationshipKind::DependsOn)
                .map(|_| ()),
            Self::OrderingOnly => store
                .relate_ordering_only_records(second, first)
                .map(|_| ()),
            Self::Remove => store
                .unrelate_records(second, first, RecordRelationshipKind::DependsOn)
                .map(|_| ()),
            Self::Stack => store.configure_task_stack(second, &root()).map(|_| ()),
        }
    }
    fn table(self) -> &'static str {
        match self {
            Self::Child => "records",
            Self::Stack => "task_stacks",
            _ => "record_relationships",
        }
    }
}
fn pause(
    store: &SqliteProjectRepository,
    table: &'static str,
) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (reserved_tx, reserved_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut paused = false;
    store
        .connection
        .lock()
        .unwrap()
        .update_hook(Some(
            move |_: rusqlite::hooks::Action, _: &str, changed: &str, _: i64| {
                if !paused && changed == table {
                    paused = true;
                    reserved_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                }
            },
        ))
        .unwrap();
    (reserved_rx, release_tx)
}

#[test]
fn child_start_waits_for_the_authoritative_spec_start_transaction() {
    let database = Database::new();
    let starter = database.open();
    let parent = spec(&starter);
    let task = child(&starter, parent);
    let worker = database.open();
    let (reserved, release) = pause(&starter, "records");
    std::thread::scope(|scope| {
        let start = scope.spawn(|| starter.start_record(parent));
        reserved.recv_timeout(Duration::from_secs(5)).unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let execute = scope.spawn(move || {
            entered_tx.send(()).unwrap();
            worker.start_record(task)
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        release.send(()).unwrap();
        start.join().unwrap().unwrap();
        assert_eq!(execute.join().unwrap().unwrap().status, RecordStatus::InProgress);
    });
}

#[test]
fn assignment_claim_waits_for_spec_start_and_legacy_partial_plan_never_claims() {
    use forge_app::AgentDispatchRepository;
    for partial in [false, true] {
        let database = Database::new();
        let starter = database.open();
        let parent = spec(&starter);
        let task = child(&starter, parent);
        if partial {
            let successor = child(&starter, parent);
            starter.relate_records(successor, task, RecordRelationshipKind::DependsOn).unwrap();
            // A pre-feature writer can have persisted In Progress without validation.
            starter.connection.lock().unwrap().execute("UPDATE records SET status = 'in_progress' WHERE id = ?1", [parent.cast_signed()]).unwrap();
            assert_eq!(starter.claim_record_agent_assignment(task, "partial").unwrap_err().code, "invalid-spec-plan");
            assert!(starter.list_agent_assignments(task).unwrap().is_empty());
            continue;
        }
        let worker = database.open();
        let (reserved, release) = pause(&starter, "records");
        std::thread::scope(|scope| {
            let start = scope.spawn(|| starter.start_record(parent));
            reserved.recv_timeout(Duration::from_secs(5)).unwrap();
            let (entered_tx, entered_rx) = mpsc::channel();
            let claim = scope.spawn(move || {
                entered_tx.send(()).unwrap();
                worker.claim_record_agent_assignment(task, "authorized")
            });
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            release.send(()).unwrap();
            start.join().unwrap().unwrap();
            assert!(!claim.join().unwrap().unwrap().1);
        });
        assert_eq!(database.open().list_agent_assignments(task).unwrap().len(), 1);
    }
}

#[test]
fn manual_begin_and_freshness_change_serialize_without_a_stale_live_attempt() {
    use forge_app::outcomes::{AttemptInput, Evidence, WorkInvalidationInput};
    for attempt_first in [true, false] {
        let database = Database::new();
        let writer = database.open();
        let parent = spec(&writer);
        let task = child(&writer, parent);
        writer.start_record(parent).unwrap();
        writer.start_record(task).unwrap();
        let evidence = Evidence::Repository { repository: "/stack-project".into(), commit: "a".repeat(40) };
        let input = AttemptInput { id: "manual".into(), check_id: writer.list_record_checks(task).unwrap()[0].id,
            generation: 1, evidence: evidence.clone(), actor: "operator".into() };
        let invalidation = WorkInvalidationInput { generation: 1, evidence,
            actor: "operator".into(), message: "Explicit refresh".into() };
        let other = database.open();
        let table = if attempt_first { "record_attempts" } else { "record_work" };
        let (reserved, release) = pause(&writer, table);
        std::thread::scope(|scope| {
            let input = &input;
            let invalidation = &invalidation;
            let writer = &writer;
            let first = scope.spawn(move || {
                if attempt_first { writer.begin_record_attempt(task, input).map(|_| ()) }
                else { writer.invalidate_record_work(task, invalidation).map(|_| ()) }
            });
            reserved.recv_timeout(Duration::from_secs(5)).unwrap();
            let (entered_tx, entered_rx) = mpsc::channel();
            let second = scope.spawn(move || {
                entered_tx.send(()).unwrap();
                if attempt_first { other.invalidate_record_work(task, invalidation).map(|_| ()) }
                else { other.begin_record_attempt(task, input).map(|_| ()) }
            });
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            release.send(()).unwrap();
            first.join().unwrap().unwrap();
            let result = second.join().unwrap();
            if attempt_first { result.unwrap(); }
            else { assert_eq!(result.unwrap_err().code, "stale-generation"); }
        });
        let reader = database.open();
        assert_eq!(reader.record_work_state(task).unwrap().generation, 2);
        let active: bool = reader.connection.lock().unwrap().query_row(
            "SELECT EXISTS(SELECT 1 FROM record_attempts WHERE record_id = ?1 AND closed = 0)",
            [task.cast_signed()], |r| r.get(0),
        ).unwrap();
        assert!(!active);
    }
}

#[test]
fn spec_start_wins_each_structural_race_and_late_writes_are_rejected() {
    for mutation in [
        Mutation::Child,
        Mutation::Dependency,
        Mutation::OrderingOnly,
        Mutation::Remove,
        Mutation::Stack,
    ] {
        let database = Database::new();
        let starter = database.open();
        let parent = spec(&starter);
        let first = child(&starter, parent);
        let second = child(&starter, parent);
        starter.configure_task_stack(first, &root()).unwrap();
        if matches!(mutation, Mutation::Remove) {
            starter.relate_ordering_only_records(second, first).unwrap();
        }
        let writer = database.open();
        let (reserved, release) = pause(&starter, "records");
        std::thread::scope(|scope| {
            let start = scope.spawn(|| starter.start_record(parent));
            reserved.recv_timeout(Duration::from_secs(5)).unwrap();
            let (entered_tx, entered_rx) = mpsc::channel();
            let writer = &writer;
            let write = scope.spawn(move || {
                entered_tx.send(()).unwrap();
                mutation.apply(writer, parent, first, second)
            });
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            release.send(()).unwrap();
            start.join().unwrap().unwrap();
            assert_eq!(
                write.join().unwrap().unwrap_err().code,
                "spec-plan-started",
                "{mutation:?}"
            );
        });
        assert_eq!(
            database.open().show_record(parent).unwrap().status,
            RecordStatus::InProgress
        );
    }
}

#[test]
fn structural_writes_win_and_start_validates_the_committed_plan() {
    for mutation in [
        Mutation::Child,
        Mutation::Dependency,
        Mutation::OrderingOnly,
        Mutation::Remove,
        Mutation::Stack,
    ] {
        let database = Database::new();
        let writer = database.open();
        let parent = spec(&writer);
        let first = child(&writer, parent);
        let second = child(&writer, parent);
        writer.configure_task_stack(first, &root()).unwrap();
        if matches!(mutation, Mutation::Remove) {
            writer.relate_ordering_only_records(second, first).unwrap();
        }
        if matches!(mutation, Mutation::Stack) {
            link(&writer, second, first);
        }
        let starter = database.open();
        let (reserved, release) = pause(&writer, mutation.table());
        std::thread::scope(|scope| {
            let write = scope.spawn(|| mutation.apply(&writer, parent, first, second));
            reserved.recv_timeout(Duration::from_secs(5)).unwrap();
            let (entered_tx, entered_rx) = mpsc::channel();
            let starter = &starter;
            let start = scope.spawn(move || {
                entered_tx.send(()).unwrap();
                starter.start_record(parent)
            });
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            release.send(()).unwrap();
            write.join().unwrap().unwrap();
            let result = start.join().unwrap();
            if matches!(mutation, Mutation::Dependency | Mutation::Stack) {
                assert_eq!(
                    result.unwrap_err().code,
                    "invalid-spec-plan",
                    "{mutation:?}"
                );
                assert_eq!(
                    starter.show_record(parent).unwrap().status,
                    RecordStatus::Open
                );
            } else {
                assert_eq!(
                    result.unwrap().status,
                    RecordStatus::InProgress,
                    "{mutation:?}"
                );
            }
        });
    }
}
