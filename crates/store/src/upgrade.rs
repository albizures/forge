//! Explicit offline upgrades. Ordinary repository opening never migrates a store.

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod tests;
use std::{fs, path::Path};

use forge_app::{
    RegistrationError,
    storage_upgrade::{
        StoreInspection, TableInventory, UPGRADE_INTERFACE_VERSION, UpgradeCapabilities,
    },
};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, types::ValueRef};
use sha2::{Digest, Sha256};

use super::{stacks, storage_error};

mod v007_to_v008;

const APPLICATION_ID: i64 = 0x464f_5247;

fn error(message: impl Into<String>) -> RegistrationError {
    RegistrationError::new("state-upgrade-blocked", message)
}

#[must_use]
pub fn capabilities() -> UpgradeCapabilities {
    UpgradeCapabilities {
        interface_version: UPGRADE_INTERFACE_VERSION,
        target_schema: 8,
        supported_sources: vec![7, 8],
    }
}

/// Inspect a store without initialization, checkpointing, or marker writes.
///
/// # Errors
/// Rejects corrupt, unknown, structurally incompatible, or newer stores.
pub fn inspect(path: &Path) -> Result<StoreInspection, RegistrationError> {
    if fs::metadata(path).map_err(|e| error(e.to_string()))?.len() == 0 {
        return Ok(StoreInspection {
            schema: None,
            fingerprint: format!("{:x}", Sha256::digest([])),
            pristine: true,
            blockers: vec![],
            tables: vec![],
        });
    }
    let connection = read_only(path)?;
    let transaction = connection.unchecked_transaction().map_err(storage_error)?;
    inspection(&transaction)
}

fn read_only(path: &Path) -> Result<Connection, RegistrationError> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(storage_error)
}

fn inventory(c: &Connection) -> Result<Vec<TableInventory>, RegistrationError> {
    let mut statement = c.prepare("SELECT name, sql FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").map_err(storage_error)?;
    let tables = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    tables
        .into_iter()
        .map(|(name, definition)| {
            let quoted = name.replace('"', "\"\"");
            let mut query = c
                .prepare(&format!("SELECT * FROM \"{quoted}\""))
                .map_err(storage_error)?;
            let columns = query.column_count();
            let mut rows = query.query([]).map_err(storage_error)?;
            let mut hashes = Vec::new();
            while let Some(row) = rows.next().map_err(storage_error)? {
                let mut hash = Sha256::new();
                for column in 0..columns {
                    match row.get_ref(column).map_err(storage_error)? {
                        ValueRef::Null => hash.update([0]),
                        ValueRef::Integer(v) => {
                            hash.update([1]);
                            hash.update(v.to_le_bytes());
                        }
                        ValueRef::Real(v) => {
                            hash.update([2]);
                            hash.update(v.to_bits().to_le_bytes());
                        }
                        ValueRef::Text(v) | ValueRef::Blob(v) => {
                            hash.update([
                                if matches!(
                                    row.get_ref(column).map_err(storage_error)?,
                                    ValueRef::Text(_)
                                ) {
                                    3
                                } else {
                                    4
                                },
                            ]);
                            hash.update((v.len() as u64).to_le_bytes());
                            hash.update(v);
                        }
                    }
                }
                hashes.push(hash.finalize());
            }
            hashes.sort();
            let count = hashes.len() as u64;
            let mut hash = Sha256::new();
            for row in hashes {
                hash.update(row);
            }
            Ok(TableInventory {
                name,
                definition,
                rows: count,
                digest: format!("{:x}", hash.finalize()),
            })
        })
        .collect()
}

fn check_integrity(c: &Connection) -> Result<(), RegistrationError> {
    let integrity: String = c
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(storage_error)?;
    if integrity != "ok" {
        return Err(error("SQLite integrity check failed"));
    }
    if c.prepare("PRAGMA foreign_key_check")
        .map_err(storage_error)?
        .exists([])
        .map_err(storage_error)?
    {
        return Err(error("foreign key validation failed"));
    }
    Ok(())
}

fn reference(schema: u32) -> Result<Connection, RegistrationError> {
    let c = Connection::open_in_memory().map_err(storage_error)?;
    let ddl = match schema {
        7 => include_str!("upgrade/schema7.sql"),
        8 => include_str!("upgrade/schema8.sql"),
        _ => return Err(error("missing frozen schema contract")),
    };
    c.execute_batch(ddl).map_err(storage_error)?;
    super::seed_agent_profiles(&c)?;
    c.execute_batch("INSERT INTO dispatch_provider_generation VALUES (1, 0)")
        .map_err(storage_error)?;
    Ok(c)
}

fn inspection(c: &Connection) -> Result<StoreInspection, RegistrationError> {
    let application: i64 = c
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .map_err(storage_error)?;
    let schema: u32 = c
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(storage_error)?;
    if application != APPLICATION_ID || !capabilities().supported_sources.contains(&schema) {
        return Err(error(
            "unsupported application or schema; supported source schemas: 7, 8",
        ));
    }
    check_integrity(c)?;
    let tables = inventory(c)?;
    let expected = reference(schema)?;
    let reference = inventory(&expected)?;
    let definitions = |items: &[TableInventory]| {
        items
            .iter()
            .map(|t| (t.name.clone(), t.definition.clone()))
            .collect::<Vec<_>>()
    };
    if definitions(&tables) != definitions(&reference) {
        return Err(error(
            "store table definitions do not match the advertised schema",
        ));
    }
    // Indexes and triggers are also part of the storage contract. Unknown ones
    // could change migration semantics and must not be silently accepted.
    if objects(c)? != objects(&expected)? {
        return Err(error("unexpected indexes, views, or triggers"));
    }
    let mut ids = c
        .prepare("SELECT id FROM records ORDER BY id")
        .map_err(storage_error)?;
    for id in ids
        .query_map([], |r| r.get::<_, i64>(0))
        .map_err(storage_error)?
    {
        let id = id.map_err(storage_error)?.cast_unsigned();
        super::load_record(c, id)?;
        if schema == 8 {
            stacks::load(c, id)?;
        }
    }
    validate_workspace_ownership(c)?;
    let mut blockers = Vec::new();
    for (sql, message) in [
        (
            "SELECT EXISTS(SELECT 1 FROM agent_assignments WHERE closed_at_millis IS NULL)",
            "unresolved assignments",
        ),
        (
            "SELECT EXISTS(SELECT 1 FROM agent_assignment_resources WHERE retired_at_millis IS NULL)",
            "unretired assignment resources",
        ),
        (
            "SELECT EXISTS(SELECT 1 FROM worktree_claims WHERE state != 'active')",
            "unresolved worktree claims",
        ),
    ] {
        if c.query_row(sql, [], |r| r.get::<_, bool>(0))
            .map_err(storage_error)?
        {
            blockers.push(message.into());
        }
    }
    let now = super::now_millis()?;
    let scan: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM agent_dispatch_configuration WHERE scan_lease_until_millis > ?1)", [now], |r| r.get(0)).map_err(storage_error)?;
    if scan {
        blockers.push("active dispatch scan lease".into());
    }
    let mut hash = Sha256::new();
    hash.update(schema.to_le_bytes());
    for t in &tables {
        hash.update(serde_json::to_vec(t).map_err(|e| error(e.to_string()))?);
    }
    for object in objects(c)? {
        hash.update(serde_json::to_vec(&object).map_err(|e| error(e.to_string()))?);
    }
    Ok(StoreInspection {
        schema: Some(schema),
        fingerprint: format!("{:x}", hash.finalize()),
        pristine: tables == reference,
        blockers,
        tables,
    })
}

fn validate_workspace_ownership(c: &Connection) -> Result<(), RegistrationError> {
    let unowned: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM records WHERE workspace_id IS NULL)",
            [],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if unowned {
        return Err(error("records without Workspace ownership"));
    }
    let cross_workspace: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM record_projects rp JOIN records r ON r.id=rp.record_id JOIN projects p ON p.slug=rp.project_slug WHERE r.workspace_id != p.workspace_id)
         OR EXISTS(SELECT 1 FROM record_relationships rel JOIN records a ON a.id=rel.from_id JOIN records b ON b.id=rel.to_id WHERE a.workspace_id != b.workspace_id)",
        [], |r| r.get(0),
    ).map_err(storage_error)?;
    if cross_workspace {
        return Err(error("cross-Workspace scope or relationship"));
    }
    Ok(())
}

fn objects(c: &Connection) -> Result<Vec<(String, String)>, RegistrationError> {
    c.prepare("SELECT name, sql FROM sqlite_schema WHERE type != 'table' AND sql IS NOT NULL ORDER BY name").map_err(storage_error)?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>().map_err(storage_error)
}

/// Make a consistent, standalone SQLite snapshot, including committed WAL data.
/// Caller must hold the daemon sidecar ownership lock for the source.
///
/// # Errors
/// Refuses an existing output or a source with unresolved execution ownership.
pub fn snapshot(source: &Path, output: &Path) -> Result<StoreInspection, RegistrationError> {
    let c = read_only(source)?;
    let before = inspection(&c)?;
    if !before.blockers.is_empty() {
        return Err(error(before.blockers.join(", ")));
    }
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| error(e.to_string()))?;
    drop(file);
    // Reserve output, but never expose a partially written SQLite snapshot as
    // a complete artifact. A surviving worker after manager death writes only
    // this request's private working file; abandoned attempts never reuse it.
    let name = output
        .file_name()
        .ok_or_else(|| error("snapshot output needs a file name"))?;
    let working = output.with_file_name(format!("{}.working", name.to_string_lossy()));
    drop(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&working)
            .map_err(|e| error(e.to_string()))?,
    );
    c.backup(rusqlite::MAIN_DB, &working, None)
        .map_err(storage_error)?;
    // Produce a standalone artifact regardless of the source's WAL mode.
    let copy = Connection::open_with_flags(&working, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(storage_error)?;
    copy.pragma_update(None, "journal_mode", "DELETE")
        .map_err(storage_error)?;
    drop(copy);
    let after = inspect(&working)?;
    if before != after {
        return Err(error("source changed during snapshot"));
    }
    fs::File::open(&working)
        .and_then(|f| f.sync_all())
        .map_err(|e| error(e.to_string()))?;
    fs::rename(&working, output).map_err(|e| error(e.to_string()))?;
    Ok(after)
}

/// Upgrade only an explicit staging copy; never called during ordinary opening.
///
/// # Errors
/// Rolls back the entire chain on validation, preservation, or SQL failure.
pub fn migrate(path: &Path) -> Result<StoreInspection, RegistrationError> {
    let mut c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(storage_error)?;
    c.pragma_update(None, "journal_mode", "DELETE")
        .map_err(storage_error)?;
    migrate_connection(&mut c)
}

fn migrate_connection(c: &mut Connection) -> Result<StoreInspection, RegistrationError> {
    c.pragma_update(None, "foreign_keys", true)
        .map_err(storage_error)?;
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Exclusive)
        .map_err(storage_error)?;
    let before = inspection(&tx)?;
    if !before.blockers.is_empty() {
        return Err(error(before.blockers.join(", ")));
    }
    match before.schema {
        Some(7) => v007_to_v008::apply(&tx)?,
        Some(8) => {}
        _ => return Err(error("unsupported migration source")),
    }
    let after = inspection(&tx)?;
    for table in &before.tables {
        if !after.tables.contains(table) {
            return Err(error(format!(
                "migration changed preserved table {}",
                table.name
            )));
        }
    }
    if before.schema == Some(7)
        && after
            .tables
            .iter()
            .find(|t| t.name == "task_stacks")
            .is_none_or(|t| t.rows != 0)
    {
        return Err(error("migration invented Task stack membership"));
    }
    tx.commit().map_err(storage_error)?;
    Ok(after)
}
