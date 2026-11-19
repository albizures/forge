CREATE TABLE IF NOT EXISTS assignment_authorization_leases (
    assignment_id TEXT NOT NULL REFERENCES durable_assignment_runtimes(assignment_id),
    sequence INTEGER NOT NULL CHECK(sequence > 0),
    lease_json TEXT NOT NULL,
    PRIMARY KEY(assignment_id, sequence)
);
CREATE TRIGGER IF NOT EXISTS authorization_lease_history_immutable
BEFORE UPDATE ON assignment_authorization_leases
BEGIN SELECT RAISE(ABORT, 'authorization lease history is immutable'); END;
CREATE TRIGGER IF NOT EXISTS authorization_lease_history_retained
BEFORE DELETE ON assignment_authorization_leases
BEGIN SELECT RAISE(ABORT, 'authorization lease history is retained'); END;
