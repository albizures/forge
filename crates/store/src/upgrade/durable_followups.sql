CREATE TABLE IF NOT EXISTS durable_assignment_followups (
    assignment_id TEXT NOT NULL REFERENCES durable_assignment_runtimes(assignment_id),
    operation_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    message TEXT NOT NULL,
    message_sha256 TEXT NOT NULL,
    binding_json TEXT NOT NULL,
    recorded_at_millis INTEGER NOT NULL,
    PRIMARY KEY (assignment_id, operation_id),
    UNIQUE (assignment_id, sequence)
);
CREATE TRIGGER IF NOT EXISTS durable_followup_immutable
BEFORE UPDATE ON durable_assignment_followups
BEGIN SELECT RAISE(ABORT, 'durable follow-up history is immutable'); END;
CREATE TRIGGER IF NOT EXISTS durable_followup_retained
BEFORE DELETE ON durable_assignment_followups
BEGIN SELECT RAISE(ABORT, 'durable follow-up history is retained'); END;
