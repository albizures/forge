use super::*;
use forge_app::stacks::UnchangedResult;

#[test]
fn invalidated_predecessor_between_observation_and_pin_is_rejected_atomically() {
    let store = fixture();
    let first = task(&store);
    let second = task(&store);
    store.configure_task_stack(first, &root()).unwrap();
    store.configure_task_stack(second, &StackSource::Predecessor { record_id: first }).unwrap();
    pin(&store, first, &"1".repeat(40));
    complete(&store, first, &"a".repeat(40));
    let expected = store.task_stack(second).unwrap().unwrap();
    let updates = store.list_record_updates(second).unwrap();
    // Simulate concurrent invalidation after the external hosting observation.
    store.connection.lock().unwrap().execute("UPDATE record_work SET generation = generation + 1 WHERE record_id = ?1", [first.cast_signed()]).unwrap();
    assert!(store.pin_task_stack(&expected, &"a".repeat(40)).is_err());
    assert_eq!(store.task_stack(second).unwrap(), Some(expected));
    assert!(store.task_base_commit(second).unwrap().is_none());
    assert_eq!(store.list_record_updates(second).unwrap(), updates);
}

#[test]
fn invalidated_ancestor_blocks_publication_and_final_outcome_in_writer_transaction() {
    let store = fixture();
    let first = task(&store);
    let review = task(&store);
    let last = task(&store);
    store.configure_task_stack(first, &root()).unwrap();
    store.configure_task_stack(review, &StackSource::Predecessor { record_id: first }).unwrap();
    store.configure_task_stack(last, &StackSource::Predecessor { record_id: review }).unwrap();
    pin(&store, first, &"1".repeat(40));
    complete(&store, first, &"a".repeat(40));
    pin(&store, review, &"a".repeat(40));
    super::cumulative::unchanged(&store, review, &"a".repeat(40));
    let stack = pin(&store, last, &"a".repeat(40));
    store.start_record(last).unwrap();
    pass(&store, last, 0, &"a".repeat(40));
    pass(&store, last, 1, &"a".repeat(40));
    let work = store.record_work_state(last).unwrap();
    let result = UnchangedResult { evidence: work.output.unwrap(), generation: work.generation };
    // Invalidate the published ancestor, not merely the immediate unchanged predecessor.
    store.connection.lock().unwrap().execute("UPDATE record_work SET generation = generation + 1 WHERE record_id = ?1", [first.cast_signed()]).unwrap();
    let updates = store.list_record_updates(last).unwrap();
    assert!(store.record_stack_unchanged(&stack, &result, None).is_err());
    assert!(store.record_stack_publication(&stack, &publication(last, &"b".repeat(40), &stack.base_branch), None).is_err());
    assert_eq!(store.list_record_updates(last).unwrap(), updates);
    assert_eq!(store.task_stack(last).unwrap(), Some(stack.clone()));
    // Restore only this test's simulated generation so verification can be recorded,
    // then reproduce invalidation between verification and final outcome acceptance.
    store.connection.lock().unwrap().execute("UPDATE record_work SET generation = generation - 1 WHERE record_id = ?1", [first.cast_signed()]).unwrap();
    store.record_stack_unchanged(&stack, &result, None).unwrap();
    let input = outcome(&store, last, 2, &"a".repeat(40));
    store.connection.lock().unwrap().execute("UPDATE record_work SET generation = generation + 1 WHERE record_id = ?1", [first.cast_signed()]).unwrap();
    assert!(store.submit_record_outcome(last, &input).is_err());
    assert!(store.done_record(last, None).is_err());
}
