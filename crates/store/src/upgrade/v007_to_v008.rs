//! Frozen 7 → 8 conversion. Do not reuse mutable current-schema initializers.
use forge_app::RegistrationError;
use rusqlite::Connection;

pub(super) fn apply(c: &Connection) -> Result<(), RegistrationError> {
    c.execute_batch(
        "CREATE TABLE task_stacks (
        record_id INTEGER PRIMARY KEY REFERENCES records(id),
        predecessor INTEGER UNIQUE REFERENCES task_stacks(record_id),
        source_json TEXT NOT NULL,
        base_commit TEXT,
        publication_json TEXT,
        CHECK(predecessor IS NULL OR predecessor != record_id)
    );
    PRAGMA user_version = 8;",
    )
    .map_err(super::storage_error)
}
