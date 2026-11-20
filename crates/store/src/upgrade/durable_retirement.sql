CREATE TABLE IF NOT EXISTS durable_assignment_retirements (
    assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES durable_assignment_runtimes(assignment_id),
    operation_json TEXT NOT NULL,
    reconciled_at_millis INTEGER
);
CREATE TRIGGER IF NOT EXISTS durable_retirement_identity_immutable
BEFORE UPDATE OF assignment_id, operation_json ON durable_assignment_retirements
BEGIN SELECT RAISE(ABORT, 'durable retirement identity is immutable'); END;
CREATE TRIGGER IF NOT EXISTS durable_retirement_result_immutable
BEFORE UPDATE OF reconciled_at_millis ON durable_assignment_retirements
WHEN OLD.reconciled_at_millis IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'durable retirement result is immutable'); END;
CREATE TRIGGER IF NOT EXISTS durable_retirement_retained
BEFORE DELETE ON durable_assignment_retirements
BEGIN SELECT RAISE(ABORT, 'durable retirement history is retained'); END;
DROP TRIGGER IF EXISTS durable_runtime_capacity_retained;
CREATE TRIGGER durable_runtime_capacity_retained
BEFORE UPDATE OF retired_at_millis ON agent_assignment_resources
WHEN OLD.resource_kind = 'durable-runtime' AND NEW.retired_at_millis IS NOT NULL
AND NOT EXISTS (
    SELECT 1 FROM durable_assignment_retirements d JOIN agent_assignments a ON a.id = d.assignment_id
    JOIN durable_assignment_runtimes r ON r.assignment_id = a.id
    WHERE d.assignment_id = OLD.assignment_id AND r.runtime_id = OLD.resource_id
    AND d.reconciled_at_millis IS NOT NULL AND a.closed_at_millis IS NOT NULL
)
BEGIN SELECT RAISE(ABORT, 'durable runtime requires verified cleanup'); END;
