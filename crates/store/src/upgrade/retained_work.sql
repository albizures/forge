CREATE TABLE retained_work_recoveries (
    source_assignment TEXT PRIMARY KEY REFERENCES agent_assignments(id),
    record_id INTEGER NOT NULL REFERENCES records(id),
    recovery_json TEXT NOT NULL
);
CREATE INDEX retained_work_by_record ON retained_work_recoveries(record_id);
CREATE TRIGGER immutable_retained_work_recovery_update BEFORE UPDATE ON retained_work_recoveries
BEGIN SELECT RAISE(ABORT, 'retained work recovery is immutable'); END;
CREATE TRIGGER immutable_retained_work_recovery_delete BEFORE DELETE ON retained_work_recoveries
BEGIN SELECT RAISE(ABORT, 'retained work recovery is immutable'); END;
