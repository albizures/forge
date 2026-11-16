use super::*;
use forge_app::stacks::{CheckoutObservation, UnchangedResult};

pub(super) fn unchanged(store: &SqliteProjectRepository, id: u64, commit: &str) {
    store.start_record(id).unwrap();
    pass(store, id, 0, commit);
    pass(store, id, 1, commit);
    let work = store.record_work_state(id).unwrap();
    store.record_stack_unchanged(
        &store.task_stack(id).unwrap().unwrap(),
        &UnchangedResult { evidence: work.output.unwrap(), generation: work.generation },
        None,
    ).unwrap();
    pass(store, id, 2, commit);
    store.done_record(id, None).unwrap();
}

fn published(hosting: &Hosting, store: &SqliteProjectRepository, id: u64) {
    let pr = store.task_stack(id).unwrap().unwrap().publication.unwrap();
    hosting.branches.lock().unwrap().insert(pr.head_branch.clone(), pr.head_commit.clone());
    hosting.prs.lock().unwrap().insert(pr.url.clone(), observation(&pr));
}

#[test]
fn changed_unchanged_changed_retains_immediate_identity_and_earlier_publication_base() {
    let store = fixture();
    let hosting = Hosting::default();
    let app = app(&store, &hosting);
    let first = task(&store);
    let review = task(&store);
    let last = task(&store);
    store.configure_task_stack(first, &root()).unwrap();
    store.relate_records(review, first, RecordRelationshipKind::BuildsOn).unwrap();
    store.relate_records(last, review, RecordRelationshipKind::BuildsOn).unwrap();
    pin(&store, first, &"1".repeat(40));
    complete(&store, first, &"a".repeat(40));
    published(&hosting, &store, first);
    app.prepare_task_stack(review).unwrap();
    unchanged(&store, review, &"a".repeat(40));
    let prepared = app.prepare_task_stack(last).unwrap().unwrap();
    assert_eq!(prepared.predecessor, Some(review));
    assert_eq!(prepared.base_commit, Some("a".repeat(40)));
    assert_eq!(prepared.base_branch, task_branch(first));
    assert_eq!(app.prepare_task_stack(last).unwrap(), Some(prepared.clone()));
    // Re-reading reconstructs lineage without inventing a PR for the review.
    assert_eq!(super::app(&store, &hosting).task_stack(last).unwrap(), Some(prepared));
    store.start_record(last).unwrap();
    pass(&store, last, 0, &"b".repeat(40));
    pass(&store, last, 1, &"b".repeat(40));
    let pr = publication(last, &"b".repeat(40), &task_branch(first));
    hosting.prs.lock().unwrap().insert(pr.url.clone(), observation(&pr));
    hosting.branches.lock().unwrap().insert(task_branch(last), "b".repeat(40));
    let saved = app.publish_task_stack(last, &pr.url, None).unwrap();
    assert_eq!(saved.publication.unwrap().base_branch, task_branch(first));
    pass(&store, last, 2, &"b".repeat(40));
    app.done_record(last, None).unwrap();
    assert!(!hosting.branches.lock().unwrap().contains_key(&task_branch(review)));
}

#[test]
fn unchanged_root_and_consecutive_unchanged_results_preserve_selected_branch_and_exact_commit() {
    let store = fixture();
    let hosting = Hosting::default();
    let app = app(&store, &hosting);
    hosting.branches.lock().unwrap().insert("feature/payments".into(), "a".repeat(40));
    let mut previous = None;
    for _ in 0..3 {
        let id = task(&store);
        if let Some(predecessor) = previous {
            store.relate_records(id, predecessor, RecordRelationshipKind::BuildsOn).unwrap();
        } else {
            app.configure_task_stack(id, &root()).unwrap();
        }
        let prepared = app.prepare_task_stack(id).unwrap().unwrap();
        assert_eq!(prepared.base_branch, "feature/payments");
        assert_eq!(prepared.base_commit, Some("a".repeat(40)));
        store.start_record(id).unwrap();
        pass(&store, id, 0, &"a".repeat(40));
        pass(&store, id, 1, &"a".repeat(40));
        *hosting.checkout.lock().unwrap() = Some(CheckoutObservation { commit: "a".repeat(40), clean: true });
        app.publish_unchanged_task_stack(id, None).unwrap();
        pass(&store, id, 2, &"a".repeat(40));
        app.done_record(id, None).unwrap();
        previous = Some(id);
    }
    assert!(hosting.prs.lock().unwrap().is_empty());
    assert_eq!(hosting.branches.lock().unwrap().len(), 1);
}

#[test]
fn retained_publication_drift_and_stale_unchanged_evidence_block_without_repinning() {
    let store = fixture();
    let hosting = Hosting::default();
    let app = app(&store, &hosting);
    let first = task(&store);
    let review = task(&store);
    let last = task(&store);
    store.configure_task_stack(first, &root()).unwrap();
    store.relate_records(review, first, RecordRelationshipKind::BuildsOn).unwrap();
    store.relate_records(last, review, RecordRelationshipKind::BuildsOn).unwrap();
    pin(&store, first, &"1".repeat(40));
    complete(&store, first, &"a".repeat(40));
    published(&hosting, &store, first);
    app.prepare_task_stack(review).unwrap();
    unchanged(&store, review, &"a".repeat(40));
    let prepared = app.prepare_task_stack(last).unwrap().unwrap();
    let updates = store.list_record_updates(last).unwrap();
    let pr = store.task_stack(first).unwrap().unwrap().publication.unwrap();
    for bad in [PullRequestObservation { open: false, ..observation(&pr) }, PullRequestObservation { draft: true, ..observation(&pr) }, PullRequestObservation { head_commit: "b".repeat(40), ..observation(&pr) }] {
        hosting.prs.lock().unwrap().insert(pr.url.clone(), bad);
        assert!(app.prepare_task_stack(last).is_err());
    }
    hosting.prs.lock().unwrap().insert(pr.url.clone(), observation(&pr));
    hosting.branches.lock().unwrap().remove(&task_branch(first));
    assert!(app.prepare_task_stack(last).is_err());
    hosting.branches.lock().unwrap().insert(task_branch(first), "b".repeat(40));
    let error = app.prepare_task_stack(last).unwrap_err();
    assert!(error.message.contains(&"a".repeat(40)));
    assert!(error.message.contains(&"b".repeat(40)));
    hosting.branches.lock().unwrap().insert(task_branch(first), "a".repeat(40));
    // Simulate a retained result invalidated after observation; both application and
    // writer gates must fail closed, even though the remote commit remains equal.
    store.connection.lock().unwrap().execute("UPDATE record_work SET generation = generation + 1 WHERE record_id = ?1", [review.cast_signed()]).unwrap();
    assert!(app.prepare_task_stack(last).is_err());
    assert!(store.pin_task_stack(&prepared, &"a".repeat(40)).is_err());
    assert_eq!(store.task_stack(last).unwrap(), Some(prepared));
    assert_eq!(store.list_record_updates(last).unwrap(), updates);
}

#[test]
fn repeated_root_preparation_reports_drift_without_rewriting_the_pin() {
    let store = fixture();
    let hosting = Hosting::default();
    let app = app(&store, &hosting);
    let id = task(&store);
    app.configure_task_stack(id, &root()).unwrap();
    hosting.branches.lock().unwrap().insert("feature/payments".into(), "a".repeat(40));
    let pinned = app.prepare_task_stack(id).unwrap();
    hosting.branches.lock().unwrap().insert("feature/payments".into(), "b".repeat(40));
    assert!(app.prepare_task_stack(id).is_err());
    hosting.branches.lock().unwrap().clear();
    assert!(app.prepare_task_stack(id).is_err());
    assert_eq!(store.task_stack(id).unwrap(), pinned);
}
