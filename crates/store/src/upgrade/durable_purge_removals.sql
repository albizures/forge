CREATE TABLE IF NOT EXISTS durable_runtime_purge_removals (
    assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES durable_runtime_purges(assignment_id),
    removal TEXT NOT NULL CHECK (removal IN ('removed', 'already-absent'))
);
CREATE TRIGGER IF NOT EXISTS durable_runtime_purge_removal_completed
BEFORE INSERT ON durable_runtime_purge_removals
WHEN NOT EXISTS (SELECT 1 FROM durable_runtime_purges p WHERE p.assignment_id = NEW.assignment_id AND p.state = 'purged')
BEGIN SELECT RAISE(ABORT, 'a runtime purge removal is recorded only for a completed purge'); END;
CREATE TRIGGER IF NOT EXISTS durable_runtime_purge_removal_immutable
BEFORE UPDATE ON durable_runtime_purge_removals
BEGIN SELECT RAISE(ABORT, 'a runtime purge removal is immutable'); END;
CREATE TRIGGER IF NOT EXISTS durable_runtime_purge_removal_retained
BEFORE DELETE ON durable_runtime_purge_removals
BEGIN SELECT RAISE(ABORT, 'durable runtime purge history is retained'); END;
