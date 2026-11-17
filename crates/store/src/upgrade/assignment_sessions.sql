CREATE TABLE IF NOT EXISTS assignment_sessions (
    assignment_id TEXT NOT NULL REFERENCES agent_assignments(id),
    session_id TEXT NOT NULL,
    first_bootstrap_at_millis INTEGER NOT NULL,
    last_bootstrap_at_millis INTEGER NOT NULL,
    PRIMARY KEY (assignment_id, session_id)
);
