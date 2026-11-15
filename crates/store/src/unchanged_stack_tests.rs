use super::*;
use forge_app::{
    checks::Verdict,
    outcomes::WorkInvalidationInput,
    stacks::{CheckoutObservation, UnchangedResult},
};

fn observe(hosting: &Hosting, commit: &str, clean: bool) {
    *hosting.checkout.lock().unwrap() = Some(CheckoutObservation {
        commit: commit.into(),
        clean,
    });
}

fn prepared(store: &SqliteProjectRepository) -> u64 {
    let id = task(store);
    store.configure_task_stack(id, &root()).unwrap();
    pin(store, id, &"a".repeat(40));
    store.start_record(id).unwrap();
    id
}

#[test]
fn manual_unchanged_result_requires_observation_and_all_reviews_then_completes_without_pr() {
    let store = fixture();
    let hosting = Hosting::default();
    let app = app(&store, &hosting);
    let id = prepared(&store);
    let commit = "a".repeat(40);
    assert!(app.publish_unchanged_task_stack(id, None).is_err());
    observe(&hosting, &commit, true);
    assert!(app.publish_unchanged_task_stack(id, None).is_err());
    pass(&store, id, 0, &commit);
    assert!(app.publish_unchanged_task_stack(id, None).is_err());
    let mut review = outcome(&store, id, 1, &commit);
    review.verdict = Verdict::Fail;
    review.disposition = OutcomeDisposition::Interrupted;
    store.submit_record_outcome(id, &review).unwrap();
    assert!(app.publish_unchanged_task_stack(id, None).is_err());
    // A fresh successful review, not a worker's unchanged assertion, unlocks verification.
    let check = store.list_record_checks(id).unwrap()[1].clone();
    let retry = AttemptInput {
        id: "review-retry".into(),
        check_id: check.id,
        generation: 1,
        evidence: review.evidence.clone(),
        actor: "operator".into(),
    };
    store.begin_record_attempt(id, &retry).unwrap();
    review.attempt_id = retry.id;
    review.verdict = Verdict::Pass;
    review.disposition = OutcomeDisposition::Complete;
    store.submit_record_outcome(id, &review).unwrap();
    let final_outcome = outcome(&store, id, 2, &commit);
    assert!(store.submit_record_outcome(id, &final_outcome).is_err());
    observe(&hosting, &commit, false);
    assert!(app.publish_unchanged_task_stack(id, None).is_err());
    observe(&hosting, &"b".repeat(40), true);
    assert!(app.publish_unchanged_task_stack(id, None).is_err());
    observe(&hosting, &commit, true);
    let saved = app.publish_unchanged_task_stack(id, None).unwrap();
    assert!(saved.publication.is_none());
    assert_eq!(saved.unchanged_result.as_ref().unwrap().generation, 1);
    let updates = store.list_record_updates(id).unwrap();
    assert_eq!(app.publish_unchanged_task_stack(id, None).unwrap(), saved);
    assert_eq!(store.list_record_updates(id).unwrap(), updates);
    let accepted = store.submit_record_outcome(id, &final_outcome).unwrap();
    assert_eq!(
        store.submit_record_outcome(id, &final_outcome).unwrap(),
        accepted
    );
    assert_eq!(
        store.show_record(id).unwrap().status,
        RecordStatus::InProgress
    );
    observe(&hosting, &commit, false);
    assert!(app.done_record(id, None).is_err());
    observe(&hosting, &"b".repeat(40), true);
    assert!(app.done_record(id, None).is_err());
    observe(&hosting, &commit, true);
    app.done_record(id, None).unwrap();
    // Hosting has no PRs or branches at all: completion never looked one up.
    assert!(hosting.prs.lock().unwrap().is_empty());
}

#[test]
fn stale_mismatched_and_unauthorized_unchanged_evidence_is_atomic() {
    let store = fixture();
    let id = prepared(&store);
    let commit = "a".repeat(40);
    pass(&store, id, 0, &commit);
    pass(&store, id, 1, &commit);
    let stack = store.task_stack(id).unwrap().unwrap();
    let result = UnchangedResult {
        generation: 1,
        evidence: Evidence::Repository {
            repository: store
                .project("alpha")
                .unwrap()
                .project_root
                .display()
                .to_string(),
            commit,
        },
    };
    let updates = store.list_record_updates(id).unwrap();
    for bad in [
        UnchangedResult {
            generation: 2,
            ..result.clone()
        },
        UnchangedResult {
            evidence: Evidence::Repository {
                repository: "/elsewhere".into(),
                commit: "a".repeat(40),
            },
            ..result.clone()
        },
        UnchangedResult {
            evidence: Evidence::Repository {
                repository: store
                    .project("alpha")
                    .unwrap()
                    .project_root
                    .display()
                    .to_string(),
                commit: "b".repeat(40),
            },
            ..result.clone()
        },
    ] {
        assert!(store.record_stack_unchanged(&stack, &bad, None).is_err());
    }
    assert!(
        store
            .record_stack_unchanged(&stack, &result, Some("not-an-assignment"))
            .is_err()
    );
    assert_eq!(store.task_stack(id).unwrap(), Some(stack.clone()));
    assert_eq!(store.list_record_updates(id).unwrap(), updates);
    store.record_stack_unchanged(&stack, &result, None).unwrap();
    pass(&store, id, 2, &"a".repeat(40));
    store
        .invalidate_record_work(
            id,
            &WorkInvalidationInput {
                generation: 1,
                evidence: result.evidence.clone(),
                actor: "operator".into(),
                message: "Recheck unchanged output".into(),
            },
        )
        .unwrap();
    assert!(store.done_record(id, None).is_err());
    assert!(
        store
            .record_stack_unchanged(&store.task_stack(id).unwrap().unwrap(), &result, None)
            .is_err()
    );
    // Re-verification at a new generation is allowed after all reviews are repeated.
    pass(&store, id, 0, &"a".repeat(40));
    pass(&store, id, 1, &"a".repeat(40));
    let current = store.task_stack(id).unwrap().unwrap();
    store
        .record_stack_unchanged(
            &current,
            &UnchangedResult {
                generation: 2,
                ..result
            },
            None,
        )
        .unwrap();
    pass(&store, id, 2, &"a".repeat(40));
    store.done_record(id, None).unwrap();
}

#[test]
fn changed_output_after_invalidation_requires_pr_and_replaces_stale_unchanged_evidence() {
    let store = fixture();
    let hosting = Hosting::default();
    let app = app(&store, &hosting);
    let id = prepared(&store);
    pass(&store, id, 0, &"a".repeat(40));
    pass(&store, id, 1, &"a".repeat(40));
    observe(&hosting, &"a".repeat(40), true);
    let saved = app.publish_unchanged_task_stack(id, None).unwrap();
    pass(&store, id, 2, &"a".repeat(40));
    store
        .invalidate_record_work(
            id,
            &WorkInvalidationInput {
                generation: 1,
                evidence: saved.unchanged_result.unwrap().evidence,
                actor: "operator".into(),
                message: "New implementation".into(),
            },
        )
        .unwrap();
    // Bind Implementation to the unchanged input, then produce changed output.
    let mut implementation = outcome(&store, id, 0, &"a".repeat(40));
    implementation.evidence = Evidence::Repository {
        repository: store
            .project("alpha")
            .unwrap()
            .project_root
            .display()
            .to_string(),
        commit: "b".repeat(40),
    };
    store.submit_record_outcome(id, &implementation).unwrap();
    pass(&store, id, 1, &"b".repeat(40));
    observe(&hosting, &"b".repeat(40), true);
    assert!(app.publish_unchanged_task_stack(id, None).is_err());
    let final_outcome = outcome(&store, id, 2, &"b".repeat(40));
    assert!(store.submit_record_outcome(id, &final_outcome).is_err());
    let stack = store.task_stack(id).unwrap().unwrap();
    let mut pr = publication(id, &"b".repeat(40), &stack.base_branch);
    pr.generation = 2;
    hosting
        .prs
        .lock()
        .unwrap()
        .insert(pr.url.clone(), observation(&pr));
    hosting
        .branches
        .lock()
        .unwrap()
        .insert(task_branch(id), "b".repeat(40));
    let saved = app.publish_task_stack(id, &pr.url, None).unwrap();
    assert!(saved.unchanged_result.is_none());
    assert_eq!(saved.publication, Some(pr));
    store.submit_record_outcome(id, &final_outcome).unwrap();
    app.done_record(id, None).unwrap();
}

#[test]
fn failed_unchanged_audit_rolls_back_evidence() {
    let store = fixture();
    let id = prepared(&store);
    pass(&store, id, 0, &"a".repeat(40));
    pass(&store, id, 1, &"a".repeat(40));
    let stack = store.task_stack(id).unwrap().unwrap();
    let updates = store.list_record_updates(id).unwrap();
    store.connection.lock().unwrap().execute_batch("CREATE TRIGGER reject_unchanged_audit BEFORE INSERT ON record_comments WHEN NEW.author = 'forge-stack' BEGIN SELECT RAISE(ABORT, 'audit denied'); END;").unwrap();
    assert!(
        store
            .record_stack_unchanged(
                &stack,
                &UnchangedResult {
                    generation: 1,
                    evidence: Evidence::Repository {
                        repository: store
                            .project("alpha")
                            .unwrap()
                            .project_root
                            .display()
                            .to_string(),
                        commit: "a".repeat(40)
                    }
                },
                None
            )
            .is_err()
    );
    assert_eq!(store.task_stack(id).unwrap(), Some(stack));
    assert_eq!(store.list_record_updates(id).unwrap(), updates);
}
