CREATE TABLE IF NOT EXISTS durable_launch_rollbacks (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    operation_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    disabled_at_millis INTEGER NOT NULL
);
CREATE TRIGGER IF NOT EXISTS durable_launch_rollback_immutable
BEFORE UPDATE ON durable_launch_rollbacks
BEGIN SELECT RAISE(ABORT, 'durable launch rollback is immutable'); END;
CREATE TRIGGER IF NOT EXISTS durable_launch_rollback_retained
BEFORE DELETE ON durable_launch_rollbacks
BEGIN SELECT RAISE(ABORT, 'durable launch rollback is retained'); END;
