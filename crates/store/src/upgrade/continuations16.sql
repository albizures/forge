CREATE TABLE IF NOT EXISTS check_continuations (
    check_id INTEGER PRIMARY KEY REFERENCES record_checks(id),
    total INTEGER NOT NULL CHECK(total BETWEEN 0 AND 20),
    consecutive_unchanged INTEGER NOT NULL CHECK(consecutive_unchanged BETWEEN 0 AND 2),
    checkpoint_json TEXT NOT NULL
);
