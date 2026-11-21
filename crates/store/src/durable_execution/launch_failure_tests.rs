//! T26: the launch claim and admission intent are one transaction, and a launch
//! that never received a grant is reconciled only with acceptable proof.
use super::*;
use forge_app::{
    AgentAssignmentClosureReason,
    durable_execution::launch_failure::{Failure, Proof, Repository as _},
};

fn resource(
    store: &SqliteProjectRepository,
    kind: &str,
) -> Option<forge_app::AgentAssignmentResource> {
    store
        .list_agent_assignment_resources("assignment")
        .unwrap()
        .into_iter()
        .find(|r| r.resource_kind == kind)
}

fn close(f: &Fixture) {
    f.store
        .close_agent_assignment("assignment", AgentAssignmentClosureReason::Retired)
        .unwrap();
}

fn bump_generation(f: &Fixture) {
    f.store.connection.lock().unwrap().execute("INSERT INTO record_work(record_id, generation) SELECT record_id, 2 FROM agent_assignments WHERE id = 'assignment' ON CONFLICT(record_id) DO UPDATE SET generation = 2", []).unwrap();
}

#[test]
fn claim_and_reservation_commit_together_or_not_at_all() {
    // A stale binding refuses inside the claim's own transaction: no claim stays.
    let f = Fixture::new();
    bump_generation(&f);
    assert!(f.store.claim("test", &f.launch).is_err());
    assert!(resource(&f.store, "provision-intent").is_none());
    assert!(f.store.runtime("test", "assignment").unwrap().is_none());

    // Closed before the claim: nothing is written and nothing can be later.
    let f = Fixture::new();
    close(&f);
    assert_eq!(f.store.claim("test", &f.launch).unwrap(), None);
    assert!(resource(&f.store, "provision-intent").is_none());
    assert!(f.store.runtime("test", "assignment").unwrap().is_none());

    // Success: marker, runtime row and capacity together; a second claim
    // (any daemon lifetime) never respawns, even with fresh identities.
    let f = Fixture::new();
    let runtime = f.store.claim("test", &f.launch).unwrap().unwrap();
    assert_eq!(runtime.admission, Admission::Pending);
    assert!(resource(&f.store, "provision-intent").is_some());
    assert!(resource(&f.store, "durable-runtime").is_some());
    let mut other = f.launch.clone();
    other.operation_id = "other".into();
    other.runtime_id = "other-runtime".into();
    assert_eq!(f.reopen().claim("test", &other).unwrap(), None);
    assert_eq!(
        f.store
            .runtime("test", "assignment")
            .unwrap()
            .unwrap()
            .launch,
        f.launch
    );
}

#[test]
fn unreserved_legacy_claim_is_discharged_only_after_revocation() {
    let f = Fixture::new();
    // A pre-T26 claim whose separate reservation never committed.
    assert!(f.store.begin_agent_provision("assignment").unwrap());
    assert_eq!(
        f.store
            .reconcile_unreserved_claim("test", "assignment")
            .unwrap_err()
            .code,
        "assignment-not-revoked"
    );
    close(&f);
    // Closed: a reservation can never commit afterwards.
    assert!(f.store.reserve("test", &f.launch).is_err());
    f.store
        .reconcile_unreserved_claim("test", "assignment")
        .unwrap();
    let discharged = resource(&f.store, "provision-intent").unwrap();
    assert!(discharged.retired_at_millis.is_some());
    // Exact retry after restart keeps the original result.
    f.reopen()
        .reconcile_unreserved_claim("test", "assignment")
        .unwrap();
    assert_eq!(
        resource(&f.reopen(), "provision-intent").unwrap(),
        discharged
    );
    // A reserved runtime is never discharged this way.
    let g = Fixture::new();
    g.store.claim("test", &g.launch).unwrap().unwrap();
    close(&g);
    assert_eq!(
        g.store
            .reconcile_unreserved_claim("test", "assignment")
            .unwrap_err()
            .code,
        "durable-launch-reserved"
    );
}

#[test]
fn never_granted_launch_reconciles_once_with_acceptable_proof() {
    let f = Fixture::new();
    f.store.claim("test", &f.launch).unwrap().unwrap();
    f.store
        .record_launch_failure("test", &f.launch, Failure::RuntimeInput)
        .unwrap();
    // Status names the specific failure point.
    let info = f.store.agent_assignment_info("assignment").unwrap();
    assert!(
        info.recovery_blockers()
            .contains(&Failure::RuntimeInput.code().to_owned())
    );
    // Open authority: refused, and the capacity trigger still holds.
    assert_eq!(
        f.store
            .reconcile_ungranted_launch("test", &f.launch, Proof::NeverLaunched)
            .unwrap_err()
            .code,
        "assignment-not-revoked"
    );
    assert!(
        f.store
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE agent_assignment_resources SET retired_at_millis = 1 WHERE resource_kind = 'durable-runtime'", [])
            .is_err(),
        "capacity is released only with a journaled proof"
    );
    close(&f);
    f.store
        .reconcile_ungranted_launch("test", &f.launch, Proof::NeverLaunched)
        .unwrap();
    let retired = f.store.agent_assignment_info("assignment").unwrap();
    assert!(!retired.capacity_reserved(), "{retired:?}");
    // Exact retry: the same result and journal, with original timestamps.
    f.reopen()
        .reconcile_ungranted_launch("test", &f.launch, Proof::NeverLaunched)
        .unwrap();
    assert_eq!(
        f.reopen().agent_assignment_info("assignment").unwrap(),
        retired
    );
    let journal: (String, i64) = f
        .store
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT proof, reconciled_at_millis FROM durable_launch_reconciliations",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(journal.0, "never-launched");
    assert!(
        f.store
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM durable_launch_reconciliations", [])
            .is_err()
    );
}

#[test]
fn launched_granted_or_sent_launches_need_host_proof() {
    use forge_app::authorization_lease::Repository as _;
    // A committed launch intent: the store alone cannot prove no spawn.
    let f = Fixture::new();
    f.store.claim("test", &f.launch).unwrap().unwrap();
    assert!(f.store.begin_agent_launch("assignment", 1_000).unwrap());
    close(&f);
    assert_eq!(
        f.store
            .reconcile_ungranted_launch("test", &f.launch, Proof::NeverLaunched)
            .unwrap_err()
            .code,
        "durable-launch-started"
    );
    // Host proof of a never-owned or quiesced ungranted host is accepted.
    f.store
        .reconcile_ungranted_launch("test", &f.launch, Proof::NeverOwned)
        .unwrap();

    // Any committed grant: only the host's retirement, crash or fence proof.
    let g = Fixture::new();
    g.store.claim("test", &g.launch).unwrap().unwrap();
    g.store.grant_lease("test", &lease(&g), 1_000).unwrap();
    close(&g);
    for proof in [Proof::NeverOwned, Proof::UngrantedQuiescence] {
        assert_eq!(
            g.store
                .reconcile_ungranted_launch("test", &g.launch, proof)
                .unwrap_err()
                .code,
            "durable-launch-granted"
        );
    }
    assert!(
        g.store
            .agent_assignment_info("assignment")
            .unwrap()
            .capacity_reserved()
    );

    // A sent admission is never reinterpreted.
    let h = Fixture::new();
    h.store.claim("test", &h.launch).unwrap().unwrap();
    assert!(h.store.begin_send("test", &h.launch).unwrap());
    close(&h);
    assert_eq!(
        h.store
            .reconcile_ungranted_launch("test", &h.launch, Proof::UngrantedQuiescence)
            .unwrap_err()
            .code,
        "durable-launch-granted"
    );
}

#[test]
fn reconciled_never_sent_launch_is_purge_eligible_only_after_proof() {
    use forge_app::durable_execution::purge::Repository as _;
    let f = Fixture::new();
    f.store.claim("test", &f.launch).unwrap().unwrap();
    close(&f);
    assert_eq!(
        f.store
            .begin_purge("test", &f.launch, "purge")
            .unwrap_err()
            .code,
        "runtime-purge-uncertainty-retained"
    );
    f.store
        .reconcile_ungranted_launch("test", &f.launch, Proof::NeverLaunched)
        .unwrap();
    f.store.begin_purge("test", &f.launch, "purge").unwrap();
}
