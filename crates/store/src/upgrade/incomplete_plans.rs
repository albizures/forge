//! Offline inspection may retain an Open plan before execution configuration exists.
use std::collections::BTreeSet;

use forge_app::{
    RecordStatus, RegistrationError,
    stacks::{StackSource, TaskStack},
};
use rusqlite::{Connection, OptionalExtension};

use super::{error, storage_error};
use crate::{load_record, stacks};

type StoredConfiguration = (Option<i64>, String, Option<String>, Option<String>);

/// Resolve complete plans normally. Missing configuration is permitted only for
/// Open lineage without preparation or result evidence; never hide a decode error
/// or downgrade retained execution evidence to an incomplete plan.
pub(super) fn load(c: &Connection, record: u64) -> Result<Option<TaskStack>, RegistrationError> {
    let mut visited = BTreeSet::new();
    let mut cursor = Some(record);
    let mut incomplete = false;
    let mut retained = false;
    while let Some(id) = cursor {
        if !visited.insert(id) {
            return Err(error(format!("Stack cycle affects Record {id}")));
        }
        let previous = stacks::predecessor(c, id)?;
        let row: Option<StoredConfiguration> = c.query_row(
            "SELECT predecessor,source_json,base_commit,publication_json FROM task_stacks WHERE record_id=?1",
            [id.cast_signed()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).optional().map_err(storage_error)?;
        if let Some((legacy, source, pin, publication)) = row {
            retained |= pin.is_some() || publication.is_some();
            if stacks::builds_on(c, id)?.is_some() {
                if legacy.is_some() || source != "{}" {
                    return Err(error(format!(
                        "Record {id} has competing starting-code configuration"
                    )));
                }
            } else {
                let source: StackSource = serde_json::from_str(&source)
                    .map_err(|e| error(format!("Record {id}: {e}")))?;
                match source {
                    StackSource::Root { destination } if previous.is_none() => {
                        destination.validate()?;
                    }
                    StackSource::Predecessor { record_id } if previous == Some(record_id) => {}
                    _ => return Err(error(format!("Record {id} has contradictory Stack source"))),
                }
            }
        } else {
            incomplete = true;
        }
        let unchanged_table: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='task_unchanged_results')",
                [],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        if unchanged_table {
            retained |= c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM task_unchanged_results WHERE record_id=?1)",
                    [id.cast_signed()],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(storage_error)?;
        }
        retained |= c.query_row("SELECT EXISTS(SELECT 1 FROM record_work WHERE record_id=?1 AND output_json IS NOT NULL)", [id.cast_signed()], |r| r.get::<_, bool>(0)).map_err(storage_error)?;
        cursor = previous;
    }
    // A non-stacked Record with no configuration is not an incomplete Stack.
    let canonical_table: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='builds_on_dependencies')",
            [],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    let participates = (canonical_table
        && c.query_row(
            "SELECT EXISTS(SELECT 1 FROM builds_on_dependencies WHERE from_id=?1 OR to_id=?1)",
            [record.cast_signed()],
            |r| r.get::<_, bool>(0),
        )
        .map_err(storage_error)?)
        || c.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_stacks WHERE record_id=?1)",
            [record.cast_signed()],
            |r| r.get::<_, bool>(0),
        )
        .map_err(storage_error)?;
    if incomplete && participates {
        for id in visited {
            if let Some(previous) = stacks::predecessor(c, id)? {
                forge_app::stacks::validate_stack_predecessor(
                    &load_record(c, id)?,
                    &load_record(c, previous)?,
                )?;
            }
            if retained || load_record(c, id)?.status != RecordStatus::Open {
                return Err(error(format!(
                    "Record {id}: incomplete Stack configuration cannot retain started work or execution evidence"
                )));
            }
        }
        return Ok(None);
    }
    stacks::load(c, record)
}
