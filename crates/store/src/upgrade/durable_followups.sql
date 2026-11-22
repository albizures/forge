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
CREATE TABLE IF NOT EXISTS durable_assignment_followup_deliveries (
    assignment_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('unknown', 'delivered')),
    admission_id TEXT,
    updated_at_millis INTEGER NOT NULL,
    PRIMARY KEY (assignment_id, operation_id),
    FOREIGN KEY (assignment_id, operation_id) REFERENCES durable_assignment_followups(assignment_id, operation_id),
    CHECK ((state = 'delivered') = (admission_id IS NOT NULL))
);
CREATE TRIGGER IF NOT EXISTS durable_followup_delivery_final
BEFORE UPDATE ON durable_assignment_followup_deliveries
WHEN OLD.state = 'delivered'
BEGIN SELECT RAISE(ABORT, 'a delivered follow-up is final'); END;
CREATE TRIGGER IF NOT EXISTS durable_followup_delivery_retained
BEFORE DELETE ON durable_assignment_followup_deliveries
BEGIN SELECT RAISE(ABORT, 'durable follow-up delivery history is retained'); END;
