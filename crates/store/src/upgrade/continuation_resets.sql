CREATE TABLE IF NOT EXISTS check_continuation_resets (
    id INTEGER PRIMARY KEY,
    check_id INTEGER NOT NULL REFERENCES record_checks(id),
    actor TEXT NOT NULL,
    message TEXT NOT NULL,
    created_at_millis INTEGER NOT NULL
);
