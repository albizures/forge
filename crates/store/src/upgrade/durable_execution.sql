CREATE TABLE IF NOT EXISTS durable_assignment_runtimes (
    assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES agent_assignments(id),
    backend TEXT NOT NULL CHECK(backend = 'pi-durable'),
    operation_id TEXT NOT NULL UNIQUE,
    runtime_id TEXT NOT NULL UNIQUE,
    conversation_id TEXT NOT NULL UNIQUE,
    store_id TEXT NOT NULL UNIQUE,
    launch_json TEXT NOT NULL,
    admission_json TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS durable_runtime_identity_immutable
BEFORE UPDATE OF assignment_id, backend, operation_id, runtime_id, conversation_id, store_id, launch_json
ON durable_assignment_runtimes
BEGIN SELECT RAISE(ABORT, 'durable runtime identity is immutable'); END;
CREATE TRIGGER IF NOT EXISTS durable_runtime_retained
BEFORE DELETE ON durable_assignment_runtimes
BEGIN SELECT RAISE(ABORT, 'durable admission history is retained'); END;
CREATE TRIGGER IF NOT EXISTS durable_runtime_capacity_retained
BEFORE UPDATE OF retired_at_millis ON agent_assignment_resources
WHEN OLD.resource_kind = 'durable-runtime' AND NEW.retired_at_millis IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'durable runtime requires verified cleanup'); END;
CREATE TRIGGER IF NOT EXISTS durable_runtime_capacity_not_deleted
BEFORE DELETE ON agent_assignment_resources WHEN OLD.resource_kind = 'durable-runtime'
BEGIN SELECT RAISE(ABORT, 'durable runtime requires verified cleanup'); END;
