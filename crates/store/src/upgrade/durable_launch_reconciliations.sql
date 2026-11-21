CREATE TABLE IF NOT EXISTS durable_launch_reconciliations (
    assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES durable_assignment_runtimes(assignment_id),
    proof TEXT NOT NULL CHECK (proof IN ('never-launched', 'never-owned', 'ungranted-quiescence')),
    reconciled_at_millis INTEGER NOT NULL
);
CREATE TRIGGER IF NOT EXISTS durable_launch_reconciliation_immutable
BEFORE UPDATE ON durable_launch_reconciliations
BEGIN SELECT RAISE(ABORT, 'a durable launch reconciliation is immutable'); END;
CREATE TRIGGER IF NOT EXISTS durable_launch_reconciliation_retained
BEFORE DELETE ON durable_launch_reconciliations
BEGIN SELECT RAISE(ABORT, 'durable launch reconciliation history is retained'); END;
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
AND NOT EXISTS (
    SELECT 1 FROM durable_launch_reconciliations l JOIN agent_assignments a ON a.id = l.assignment_id
    JOIN durable_assignment_runtimes r ON r.assignment_id = a.id
    WHERE l.assignment_id = OLD.assignment_id AND r.runtime_id = OLD.resource_id
    AND a.closed_at_millis IS NOT NULL
)
BEGIN SELECT RAISE(ABORT, 'durable runtime requires verified cleanup'); END;
