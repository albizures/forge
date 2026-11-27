CREATE TABLE IF NOT EXISTS assignment_nudges (
    assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES agent_assignments(id),
    last_completion_seq INTEGER NOT NULL CHECK (last_completion_seq >= 0),
    total INTEGER NOT NULL CHECK (total >= 0),
    consecutive_no_commit INTEGER NOT NULL CHECK (consecutive_no_commit >= 0),
    last_head TEXT
);
CREATE TABLE IF NOT EXISTS assignment_prompt_intents (
    assignment_id TEXT NOT NULL REFERENCES agent_assignments(id),
    sequence INTEGER NOT NULL CHECK (sequence >= 1),
    completion_seq INTEGER NOT NULL CHECK (completion_seq >= 0),
    consecutive_no_commit INTEGER NOT NULL CHECK (consecutive_no_commit >= 0),
    head TEXT NOT NULL,
    created_at_millis INTEGER NOT NULL,
    settled_at_millis INTEGER,
    settlement TEXT CHECK (settlement IN ('confirmed', 'uncertain')),
    PRIMARY KEY (assignment_id, sequence),
    CHECK ((settled_at_millis IS NULL) = (settlement IS NULL))
);
