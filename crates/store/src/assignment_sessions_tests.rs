use super::*;
use forge_app::AgentDispatchRepository;

fn fixture() -> SqliteProjectRepository {
    let store = SqliteProjectRepository::in_memory().unwrap();
    store.connection.lock().unwrap().execute_batch(
        "INSERT INTO workspaces VALUES('w','Workspace','/w');
         INSERT INTO projects VALUES('p','/w/p','git@example:p','w','p');
         INSERT INTO records(id,record_type,body,status,workspace_id) VALUES
           (1,'task','First','in_progress','w'),(2,'task','Second','in_progress','w');
         INSERT INTO agent_assignments(id,record_id,project_slug,profile_record_type,profile_revision,attempt_number,claimed_at_millis,acknowledged_at_millis) VALUES
           ('a',1,'p','task',1,1,1,1),('b',2,'p','task',1,1,1,1);
         INSERT INTO assignment_context_bootstrap VALUES('a'),('b');"
    ).unwrap();
    store
}

#[test]
fn session_identity_is_assignment_scoped_and_observations_are_concurrently_deduplicated() {
    let store = fixture();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                store
                    .register_assignment_session("a", "same-pi-session")
                    .unwrap();
            });
        }
    });
    store
        .register_assignment_session("b", "same-pi-session")
        .unwrap();
    let connection = store.connection.lock().unwrap();
    for assignment in ["a", "b"] {
        let sessions = load(&connection, assignment).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "same-pi-session");
        assert!(sessions[0].last_bootstrap_at_millis >= sessions[0].first_bootstrap_at_millis);
    }
}

#[test]
fn mutable_bootstrap_preconditions_are_enforced_without_losing_retained_history() {
    for mutation in [
        "UPDATE agent_assignments SET closed_at_millis=2 WHERE id='a'",
        "UPDATE agent_assignments SET acknowledged_at_millis=NULL WHERE id='a'",
        "UPDATE records SET status='done' WHERE id=1",
        "DELETE FROM assignment_context_bootstrap WHERE assignment_id='a'",
    ] {
        let store = fixture();
        store.register_assignment_session("a", "retained").unwrap();
        let before = load(&store.connection.lock().unwrap(), "a").unwrap();
        store
            .connection
            .lock()
            .unwrap()
            .execute_batch(mutation)
            .unwrap();
        for session in ["new", "retained"] {
            assert_eq!(
                store
                    .register_assignment_session("a", session)
                    .unwrap_err()
                    .code,
                "assignment-bootstrap-incomplete"
            );
        }
        assert_eq!(
            load(&store.connection.lock().unwrap(), "a").unwrap(),
            before
        );
    }
}
