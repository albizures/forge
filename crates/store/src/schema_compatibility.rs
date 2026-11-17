//! Fresh-store-only compatibility gate, checked before any initialization writes.
use crate::storage_error;
use forge_app::RegistrationError;
use rusqlite::Connection;

// Bump for every incompatible storage contract. Revision 7 requires durable
// retry/remediation accounting; older attempts cannot safely start with zero budgets.
// Revision 13 requires explicit canonical prerequisite intent after validated
// conversion; identical table definitions do not imply semantic compatibility.
// Revision 14 retains Assignment session bootstrap provenance.
// Revision 15 adds explicit Check-scoped continuation checkpoints and budgets.
// Revision 16 adds append-only, Check-scoped continuation reset attribution.
// Revision 17 records explicit retained-work salvage authority without historical backfill.
// Revision 18 requires content-bound dirty salvage enforcement, even with unchanged SQL tables.
const SCHEMA_REVISION: i64 = 18;
const APPLICATION_ID: i64 = 0x464f_5247; // FORG

pub(super) fn validate(connection: &Connection) -> Result<(), RegistrationError> {
    let revision: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage_error)?;
    let application_id: i64 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(storage_error)?;
    let has_schema: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%')",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if (application_id == 0 && revision == 0 && !has_schema)
        || (application_id == APPLICATION_ID && revision == SCHEMA_REVISION && has_schema)
    {
        return Ok(());
    }
    Err(RegistrationError::new(
        "fresh-store-required",
        "this store has an incompatible schema; select a fresh store. Existing data was not converted, deleted or reset; safely retire ownership and reconcile retained resources before any separate operator reset",
    ))
}

pub(super) fn stamp(connection: &Connection) -> Result<(), RegistrationError> {
    connection
        .pragma_update(None, "application_id", APPLICATION_ID)
        .map_err(storage_error)?;
    connection
        .pragma_update(None, "user_version", SCHEMA_REVISION)
        .map_err(storage_error)
}
