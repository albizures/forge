CREATE TABLE IF NOT EXISTS ordering_only_dependencies (
    from_id INTEGER NOT NULL,
    to_id INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind = 'depends-on'),
    PRIMARY KEY (from_id, to_id),
    FOREIGN KEY (from_id, to_id, kind) REFERENCES record_relationships(from_id, to_id, kind) ON DELETE CASCADE
);
