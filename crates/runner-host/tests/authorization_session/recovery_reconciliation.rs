//! Actual application daemon scan -> controlled recovery factory/driver ->
//! protected proof -> committed grant -> executable original-runtime resume.
use super::*;
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentProfile,
    Application, RepositoryInspector,
};
use forge_server::durable_authorization::{recovery, retirement};
use std::{sync::Arc, time::Instant};
struct NoHerdr;
impl AgentDispatchAdapterFactory for NoHerdr {
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        panic!("no Herdr recovery fallback")
    }
}
struct NoGit;
impl RepositoryInspector for NoGit {
    fn inspect(&self, _: &Path) -> Result<Project, forge_app::RegistrationError> {
        panic!("recovery must use its canonical Git adapter")
    }
}

fn controlled_components() -> recovery::Components {
    let components = effect_admission::components();
    recovery::Components {
        host: PathBuf::from(env!("CARGO_BIN_EXE_forge-runner-host"))
            .canonicalize()
            .unwrap(),
        host_arguments: vec![],
        node: components.node,
        inspector: components.inspector.with_file_name("inspect-recovery.ts"),
        answer_inspector: components.inspector.with_file_name("inspect-answer.ts"),
        runner: components
            .inspector
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("test/recovered-check-worker.mjs")
            .canonicalize()
            .unwrap(),
    }
}

#[test]
fn crash_receipt_retirement_requires_revocation_and_never_repairs_a_host_receipt() {
    use forge_app::durable_execution::retirement::Repository as _;
    let (f, _, _) = recovery_crash::crashed();
    let components = effect_admission::components();
    assert!(f.store.revoked_lease("test", &f.launch).is_err());
    let controller = Arc::new(
        retirement::Controller::default()
            .with_inspection(components)
            .unwrap(),
    );
    let app = Application::with_agent_dispatch(
        Box::new(open(&f.database)),
        Box::new(open(&f.database)),
        Box::new(retirement::Factory {
            database: f.database.clone(),
            controller,
            launcher: None,
            recovery: None,
            original: Box::new(NoHerdr),
        }),
        Box::new(NoGit),
    );
    let bytes = recovery_inspection::runtime_bytes(&f.directory);
    let first = app.retire_agent_assignment("assignment").unwrap();
    assert!(first.closed_at_millis.is_some());
    assert!(
        !f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    assert!(!f.directory.join("retirement-receipt.json").exists());
    assert!(f.directory.join("recovery-crash-1.json").exists());
    assert_eq!(app.retire_agent_assignment("assignment").unwrap(), first);
    assert_eq!(recovery_inspection::runtime_bytes(&f.directory), bytes);
}

#[test]
fn historical_crash_proof_cannot_waive_a_later_host_fault_during_retirement() {
    let (f, _, _) = recovery_crash::crashed();
    fs::write(f.directory.join("watchdog.json"), "later-uncertainty").unwrap();
    let controller = Arc::new(
        retirement::Controller::default()
            .with_inspection(effect_admission::components())
            .unwrap(),
    );
    let app = Application::with_agent_dispatch(
        Box::new(open(&f.database)),
        Box::new(open(&f.database)),
        Box::new(retirement::Factory {
            database: f.database.clone(),
            controller,
            launcher: None,
            recovery: None,
            original: Box::new(NoHerdr),
        }),
        Box::new(NoGit),
    );
    assert!(app.retire_agent_assignment("assignment").is_err());
    let info = f.store.agent_assignment_info("assignment").unwrap();
    assert!(info.assignment.closed_at_millis.is_some());
    assert!(info.capacity_reserved());
    assert!(!f.directory.join("retirement-receipt.json").exists());
    assert_eq!(
        fs::read_to_string(f.directory.join("watchdog.json")).unwrap(),
        "later-uncertainty"
    );
}

#[test]
fn recovery_controller_refuses_missing_foreign_and_revoked_proof_before_spawning_or_claiming() {
    for fault in 0..4 {
        let (f, _, _) = recovery_crash::crashed();
        let components = effect_admission::components();
        let retirement = Arc::new(
            retirement::Controller::default()
                .with_inspection(components)
                .unwrap(),
        );
        let controller =
            recovery::Controller::new(f.database.clone(), controlled_components(), retirement)
                .unwrap();
        let original_owner = fs::read(f.directory.join("owner.json")).unwrap();
        match fault {
            0 => fs::remove_file(f.directory.join("recovery-crash-1.json")).unwrap(),
            1 => fs::write(f.directory.join("recovery-crash-1.json"), "PRIVATE-CANARY").unwrap(),
            2 => {
                f.store
                    .close_agent_assignment(
                        "assignment",
                        forge_app::AgentAssignmentClosureReason::Retired,
                    )
                    .unwrap();
            }
            _ => {}
        }
        assert!(
            controller
                .recover(if fault == 3 { "foreign" } else { "test" }, &f.launch)
                .is_err()
        );
        assert_eq!(
            fs::read(f.directory.join("owner.json")).unwrap(),
            original_owner
        );
        assert!(
            !f.store
                .list_agent_assignment_resources("assignment")
                .unwrap()
                .iter()
                .any(|r| r.resource_kind == "recovery-claim")
        );
        assert!(
            f.store
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
    }
}

#[test]
fn daemon_reconciliation_recovers_once_and_keeps_missing_outcome_as_intervention() {
    let _ = reconcile(false);
}

#[test]
fn daemon_reconciliation_handles_a_second_proven_crash_without_new_attempt_or_input() {
    let _ = reconcile(true);
}

pub(super) fn reconcile(crash_once: bool) -> Fixture {
    let (f, _, _) = recovery_crash::crashed();
    if crash_once {
        fs::write(f.directory.join("controlled-crash-once"), "controlled").unwrap();
    }
    rusqlite::Connection::open(&f.database)
        .unwrap()
        .execute(
            "UPDATE records SET status = 'in_progress' WHERE id = ?1",
            [f.record.cast_signed()],
        )
        .unwrap();
    let components = effect_admission::components();
    let retirement = Arc::new(
        retirement::Controller::default()
            .with_inspection(components.clone())
            .unwrap(),
    );
    let recovery = Arc::new(
        recovery::Controller::new(
            f.database.clone(),
            controlled_components(),
            retirement.clone(),
        )
        .unwrap(),
    );
    let app = Application::with_agent_dispatch(
        Box::new(open(&f.database)),
        Box::new(open(&f.database)),
        Box::new(retirement::Factory {
            database: f.database.clone(),
            controller: retirement,
            launcher: None,
            recovery: Some(recovery.clone()),
            original: Box::new(NoHerdr),
        }),
        Box::new(NoGit),
    );
    let attempt = f.store.assignment_check_attempt("assignment").unwrap();
    app.reconcile_agent_dispatch("recovery-test-daemon", 1_000_000)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if f.store
            .list_agent_assignment_resources("assignment")
            .unwrap()
            .iter()
            .any(|r| r.resource_kind == "missing-outcome")
        {
            break;
        }
        assert!(Instant::now() < deadline, "committed answer not observed");
        std::thread::sleep(Duration::from_millis(10));
    }
    app.reconcile_agent_dispatch("recovery-test-daemon", 1_040_000)
        .unwrap();
    assert_eq!(
        f.store.assignment_check_attempt("assignment").unwrap(),
        attempt
    );
    let db = rusqlite::Connection::open(&f.database).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM assignment_authorization_leases",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        if crash_once { 3 } else { 2 }
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM agent_assignments", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(
        f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    assert!(f.store.dispatch_hold(f.record).unwrap().is_some());
    assert_eq!(
        fs::read_to_string(f.directory.join("controlled-recovery-models")).unwrap(),
        "model\n"
    );
    let preparations =
        fs::read_to_string(f.directory.join("controlled-recovery-preparations")).unwrap();
    assert_eq!(preparations.lines().count(), if crash_once { 2 } else { 1 });
    app.retire_agent_assignment("assignment").unwrap();
    assert!(
        !f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );
    f
}

#[test]
fn daemon_delivers_a_recorded_follow_up_once_to_the_resumed_original_conversation() {
    use forge_app::durable_execution::followup::{Repository as _, Request};
    let (f, _, _) = recovery_crash::crashed();
    fs::write(f.directory.join("controlled-slow-model"), "slow").unwrap();
    rusqlite::Connection::open(&f.database)
        .unwrap()
        .execute(
            "UPDATE records SET status = 'in_progress' WHERE id = ?1",
            [f.record.cast_signed()],
        )
        .unwrap();
    let request = Request {
        assignment_id: "assignment".into(),
        operation_id: "operator:1".into(),
        message: "use another commit".into(),
    };
    let receipt = f.store.record_followup("test", &request).unwrap();
    assert_eq!(receipt.delivery, "recorded");
    let retirement = Arc::new(
        retirement::Controller::default()
            .with_inspection(effect_admission::components())
            .unwrap(),
    );
    let recovery = Arc::new(
        recovery::Controller::new(
            f.database.clone(),
            controlled_components(),
            retirement.clone(),
        )
        .unwrap(),
    );
    let app = Application::with_agent_dispatch(
        Box::new(open(&f.database)),
        Box::new(open(&f.database)),
        Box::new(retirement::Factory {
            database: f.database.clone(),
            controller: retirement,
            launcher: None,
            recovery: Some(recovery),
            original: Box::new(NoHerdr),
        }),
        Box::new(NoGit),
    );
    let attempt = f.store.assignment_check_attempt("assignment").unwrap();
    app.reconcile_agent_dispatch("recovery-test-daemon", 1_000_000)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let delivered = loop {
        let current = &f.store.followups("test", "assignment").unwrap()[0];
        if current.delivery == "delivered" {
            break current.clone();
        }
        assert!(Instant::now() < deadline, "follow-up never confirmed");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(delivered.admission_id.is_some());
    // Idempotent: a retry still returns the one original record.
    let again = f.store.record_followup("test", &request).unwrap();
    assert!(again.replayed && again.sequence == 1);
    assert_eq!(f.store.followups("test", "assignment").unwrap().len(), 1);
    assert_eq!(
        f.store.assignment_check_attempt("assignment").unwrap(),
        attempt
    );
    assert!(
        f.store
            .undelivered_followups("test", "assignment")
            .unwrap()
            .is_empty()
    );
}

/// M6 (T12/T23 correction): the runner commits a follow-up but its acceptance
/// is lost with the crashed session, so the daemon retains `unknown`. The next
/// recovered session re-sends the same operation; the runner recognizes its
/// committed request and replays the original admission. Exactly one input.
#[test]
fn unknown_follow_up_is_resent_to_the_next_session_with_exactly_one_admission() {
    use forge_app::durable_execution::followup::{Repository as _, Request};
    let (f, _, _) = recovery_crash::crashed();
    fs::write(f.directory.join("controlled-async-model"), "async").unwrap();
    fs::write(f.directory.join("controlled-drop-followup-once"), "drop").unwrap();
    rusqlite::Connection::open(&f.database)
        .unwrap()
        .execute(
            "UPDATE records SET status = 'in_progress' WHERE id = ?1",
            [f.record.cast_signed()],
        )
        .unwrap();
    let request = Request {
        assignment_id: "assignment".into(),
        operation_id: "operator:unknown".into(),
        message: "use another commit".into(),
    };
    f.store.record_followup("test", &request).unwrap();
    let retirement = Arc::new(
        retirement::Controller::default()
            .with_inspection(effect_admission::components())
            .unwrap(),
    );
    let recovery = Arc::new(
        recovery::Controller::new(
            f.database.clone(),
            controlled_components(),
            retirement.clone(),
        )
        .unwrap(),
    );
    let app = Application::with_agent_dispatch(
        Box::new(open(&f.database)),
        Box::new(open(&f.database)),
        Box::new(retirement::Factory {
            database: f.database.clone(),
            controller: retirement,
            launcher: None,
            recovery: Some(recovery),
            original: Box::new(NoHerdr),
        }),
        Box::new(NoGit),
    );
    let attempt = f.store.assignment_check_attempt("assignment").unwrap();
    let events = f.directory.join("controlled-followup-events");
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut now = 1_000_000;
    let mut observed_unknown = false;
    // Session 1 commits the input and crashes before its acceptance is read.
    let delivered = loop {
        app.reconcile_agent_dispatch("recovery-test-daemon", now)
            .unwrap();
        now += 40_000;
        // Read the runner's evidence first: while only session 1's acceptance
        // exists, Forge must already hold `unknown` (marked before the write).
        let accepted = fs::read_to_string(&events).map_or(0, |e| e.lines().count());
        let current = f.store.followups("test", "assignment").unwrap()[0].clone();
        if accepted == 1 {
            assert_ne!(
                current.delivery, "recorded",
                "unknown is committed before delivery"
            );
            observed_unknown |= current.delivery == "unknown";
        }
        if current.delivery == "delivered" {
            break current;
        }
        assert!(
            Instant::now() < deadline,
            "follow-up never confirmed: {} {:?}",
            current.delivery,
            fs::read_to_string(&events).ok()
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let lines: Vec<serde_json::Value> = fs::read_to_string(&events)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0]["replayed"], false);
    assert_eq!(lines[1]["replayed"], true);
    assert_eq!(lines[0]["admission_id"], lines[1]["admission_id"]);
    assert_eq!(
        delivered.admission_id.as_deref(),
        lines[0]["admission_id"].as_str()
    );
    assert_eq!(f.store.followups("test", "assignment").unwrap().len(), 1);
    assert_eq!(
        f.store.assignment_check_attempt("assignment").unwrap(),
        attempt
    );
    assert!(!f.directory.join("controlled-drop-followup-once").exists());
    // Two recovery sessions: the crashed one and the one that received the re-send.
    let preparations =
        fs::read_to_string(f.directory.join("controlled-recovery-preparations")).unwrap();
    assert_eq!(preparations.lines().count(), 2);
    // The new session needs a fresh host spawn, far longer than one poll.
    assert!(
        observed_unknown,
        "the store never showed `unknown` between sessions"
    );
}
