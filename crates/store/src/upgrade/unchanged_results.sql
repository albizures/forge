CREATE TABLE task_unchanged_results (
    record_id INTEGER PRIMARY KEY REFERENCES task_stacks(record_id),
    evidence_json TEXT NOT NULL
);
