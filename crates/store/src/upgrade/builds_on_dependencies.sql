CREATE TABLE IF NOT EXISTS builds_on_dependencies (
    from_id INTEGER PRIMARY KEY,
    to_id INTEGER NOT NULL UNIQUE,
    kind TEXT NOT NULL DEFAULT 'depends-on' CHECK(kind = 'depends-on'),
    FOREIGN KEY(from_id, to_id, kind) REFERENCES record_relationships(from_id, to_id, kind) ON DELETE CASCADE,
    CHECK(from_id != to_id)
);
