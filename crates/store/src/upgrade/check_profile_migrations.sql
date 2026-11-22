CREATE TABLE IF NOT EXISTS check_profile_migrations (
    id INTEGER PRIMARY KEY NOT NULL,
    record_id INTEGER NOT NULL REFERENCES records(id),
    operation_id TEXT NOT NULL,
    request_sha256 TEXT NOT NULL,
    actor TEXT NOT NULL,
    message TEXT NOT NULL,
    generation_before INTEGER NOT NULL,
    generation_after INTEGER NOT NULL CHECK (generation_after >= generation_before),
    created_at_millis INTEGER NOT NULL,
    UNIQUE (record_id, operation_id)
);
CREATE TABLE IF NOT EXISTS check_profile_migration_changes (
    migration_id INTEGER NOT NULL REFERENCES check_profile_migrations(id),
    check_id INTEGER NOT NULL REFERENCES record_checks(id),
    prior_revision INTEGER NOT NULL CHECK (prior_revision > 0),
    new_revision INTEGER NOT NULL CHECK (new_revision > 0 AND new_revision <> prior_revision),
    PRIMARY KEY (migration_id, check_id)
);
CREATE TRIGGER IF NOT EXISTS check_profile_migration_immutable
BEFORE UPDATE ON check_profile_migrations
BEGIN SELECT RAISE(ABORT, 'Check profile migration history is immutable'); END;
CREATE TRIGGER IF NOT EXISTS check_profile_migration_retained
BEFORE DELETE ON check_profile_migrations
BEGIN SELECT RAISE(ABORT, 'Check profile migration history is retained'); END;
CREATE TRIGGER IF NOT EXISTS check_profile_change_immutable
BEFORE UPDATE ON check_profile_migration_changes
BEGIN SELECT RAISE(ABORT, 'Check profile migration history is immutable'); END;
CREATE TRIGGER IF NOT EXISTS check_profile_change_retained
BEFORE DELETE ON check_profile_migration_changes
BEGIN SELECT RAISE(ABORT, 'Check profile migration history is retained'); END;
