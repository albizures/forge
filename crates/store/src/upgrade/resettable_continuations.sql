ALTER TABLE check_continuations RENAME TO legacy_check_continuations;
CREATE TABLE check_continuations (
    check_id INTEGER PRIMARY KEY REFERENCES record_checks(id),
    total INTEGER NOT NULL CHECK(total BETWEEN 0 AND 20),
    consecutive_unchanged INTEGER NOT NULL CHECK(consecutive_unchanged BETWEEN 0 AND 2),
    checkpoint_json TEXT NOT NULL
);
INSERT INTO check_continuations SELECT * FROM legacy_check_continuations;
DROP TABLE legacy_check_continuations;
