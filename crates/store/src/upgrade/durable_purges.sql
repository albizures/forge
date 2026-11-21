CREATE TABLE IF NOT EXISTS durable_runtime_purges (
    assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES durable_assignment_runtimes(assignment_id),
    operation_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'purged', 'retained')),
    diagnostic TEXT,
    requested_at_millis INTEGER NOT NULL,
    completed_at_millis INTEGER,
    CHECK ((state = 'purged') = (completed_at_millis IS NOT NULL AND diagnostic IS NULL))
);
CREATE TRIGGER IF NOT EXISTS durable_runtime_purge_identity_immutable
BEFORE UPDATE OF assignment_id, operation_id, requested_at_millis ON durable_runtime_purges
BEGIN SELECT RAISE(ABORT, 'durable runtime purge identity is immutable'); END;
CREATE TRIGGER IF NOT EXISTS durable_runtime_purge_final
BEFORE UPDATE ON durable_runtime_purges
WHEN OLD.state = 'purged'
BEGIN SELECT RAISE(ABORT, 'a completed runtime purge is final'); END;
CREATE TRIGGER IF NOT EXISTS durable_runtime_purge_retained
BEFORE DELETE ON durable_runtime_purges
BEGIN SELECT RAISE(ABORT, 'durable runtime purge history is retained'); END;
