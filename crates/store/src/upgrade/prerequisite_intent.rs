//! Semantic schema 13 conversion. Only the private staging transaction is changed.
use std::collections::{BTreeMap, BTreeSet};

use forge_app::{RecordStatus, RecordType, RegistrationError, storage_upgrade::WaitsForDecision};
use rusqlite::{Connection, params};

use super::{error, storage_error};
use crate::{load_record, stacks};

fn pairs(c: &Connection, sql: &str) -> Result<Vec<(u64, u64)>, RegistrationError> {
    c.prepare(sql)
        .map_err(storage_error)?
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?.cast_unsigned(),
                r.get::<_, i64>(1)?.cast_unsigned(),
            ))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)
}

fn same_spec(c: &Connection, from: u64, to: u64) -> Result<bool, RegistrationError> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM record_relationships a JOIN record_relationships b ON a.parent_id=b.parent_id JOIN records p ON p.id=a.parent_id WHERE a.child_id=?1 AND b.child_id=?2 AND a.kind='parent-child' AND b.kind='parent-child' AND p.record_type='spec')", params![from.cast_signed(), to.cast_signed()], |r| r.get(0)).map_err(storage_error)
}

/// Validate topology and retained execution facts without requiring an incomplete
/// Open plan to be executable, or observing/minting external Git evidence.
pub(super) fn validate(c: &Connection) -> Result<(), RegistrationError> {
    let edges = pairs(
        c,
        "SELECT from_id,to_id FROM record_relationships WHERE kind='depends-on' ORDER BY from_id,to_id",
    )?;
    let builds = pairs(
        c,
        "SELECT from_id,to_id FROM builds_on_dependencies ORDER BY from_id",
    )?;
    let topology: BTreeMap<_, _> = builds.iter().copied().collect();
    for &(from, to) in &edges {
        let a = load_record(c, from)?;
        let b = load_record(c, to)?;
        forge_app::prerequisites::validate_endpoints(&a, &b)
            .map_err(|e| error(format!("Records {from} -> {to}: {}", e.message)))?;
        let cycle: bool = c.query_row("WITH RECURSIVE ancestors(id) AS (SELECT ?1 UNION SELECT to_id FROM record_relationships JOIN ancestors ON from_id=ancestors.id WHERE kind='depends-on') SELECT EXISTS(SELECT 1 FROM ancestors WHERE id=?2)", params![to.cast_signed(), from.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
        if cycle {
            return Err(error(format!(
                "prerequisite cycle affects Records {from} and {to}"
            )));
        }
        let wait = crate::ordering_dependencies::designated(c, from, to)?;
        if wait == (topology.get(&from) == Some(&to)) {
            return Err(error(format!(
                "Record {from} -> {to} has missing or conflicting canonical intent"
            )));
        }
    }
    let legacy: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM task_stacks WHERE predecessor IS NOT NULL)",
            [],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if legacy {
        return Err(error(
            "canonical schema contains legacy Stack predecessor links",
        ));
    }
    for &(from, to) in &builds {
        super::incomplete_plans::load(c, from)?;
        forge_app::stacks::validate_stack_predecessor(&load_record(c, from)?, &load_record(c, to)?)
            .map_err(|e| error(format!("Records {from} -> {to}: {}", e.message)))?;
    }
    let ids = c
        .prepare("SELECT record_id FROM task_stacks ORDER BY record_id")
        .map_err(storage_error)?
        .query_map([], |r| r.get::<_, i64>(0))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    for id in ids {
        let id = id.cast_unsigned();
        let Some(stack) = super::incomplete_plans::load(c, id)? else {
            // Structurally valid Open lineage with no execution evidence.
            continue;
        };
        let record = load_record(c, id)?;
        stack.destination.validate()?;
        if let Some(publication) = &stack.publication
            && (publication.repository != stack.destination.repository
                || publication.head_branch != forge_app::stacks::task_branch(id)
                || publication.base_branch != stack.base_branch)
        {
            return Err(error(format!(
                "Record {id} has contradictory retained publication lineage"
            )));
        }
        if record.record_type != RecordType::Task {
            return Err(error(format!("Stack Record {id} is not a Task")));
        }
        // Existing prepared work must still pass the ordinary persisted evidence gate.
        if stack.base_commit.is_some() {
            stacks::execution_gate(c, id, None)
                .map_err(|e| error(format!("Record {id}: {}", e.message)))?;
        }
        if stack.publication.is_some()
            || stack.unchanged_result.is_some()
            || (record.status == RecordStatus::Done && crate::completion::load(c, id)?.is_none())
        {
            let work = crate::outcomes::state(c, id)?;
            if let Some(output) = &work.output {
                output.validate()?;
            }
            stack
                .verified_commit(&work)
                .map_err(|e| error(format!("Record {id}: {}", e.message)))?;
        }
    }
    Ok(())
}

struct ConversionPlan {
    legacy: Vec<(u64, u64)>,
    before: Vec<(u64, Option<forge_app::stacks::TaskStack>)>,
    waits: Vec<(u64, u64)>,
}

fn validate_decision(c: &Connection, from: u64, to: u64) -> Result<(), RegistrationError> {
    let status = load_record(c, from)?.status;
    // Completed endpoints need no new execution authorization. An explicit
    // decision only classifies their legacy edge; retained execution facts
    // are still checked by validate and the preservation inventory.
    if status == RecordStatus::Done && load_record(c, to)?.status == RecordStatus::Done {
        return Ok(());
    }
    // Other supplied decisions are not a lifecycle/recovery bypass.
    if status != RecordStatus::Open {
        return Err(error(format!(
            "Record {from} is not Open and its endpoints are not both Done; cannot resolve legacy intent"
        )));
    }
    crate::spec_plans::editable_task(c, from)?;
    let prepared: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM task_stacks WHERE record_id=?1 AND base_commit IS NOT NULL)", [from.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
    if prepared {
        return Err(error(format!(
            "Record {from} is prepared; explicit recovery is required before an intent decision"
        )));
    }
    Ok(())
}

fn plan(
    c: &Connection,
    decisions: &[WaitsForDecision],
) -> Result<ConversionPlan, RegistrationError> {
    let legacy = pairs(
        c,
        "SELECT record_id,predecessor FROM task_stacks WHERE predecessor IS NOT NULL ORDER BY record_id",
    )?;
    let mut topology: BTreeMap<_, _> =
        pairs(c, "SELECT from_id,to_id FROM builds_on_dependencies")?
            .into_iter()
            .collect();
    let before = legacy
        .iter()
        .map(|&(id, _)| Ok((id, super::incomplete_plans::load(c, id)?)))
        .collect::<Result<Vec<_>, RegistrationError>>()?;
    for &(from, to) in &legacy {
        if let Some((&other, _)) = topology
            .iter()
            .find(|&(&id, &previous)| id != from && previous == to)
        {
            return Err(error(format!(
                "Records {from} and {other} fork Stack Predecessor {to}"
            )));
        }
        if topology.insert(from, to).is_some() {
            return Err(error(format!(
                "Record {from} has competing canonical and legacy predecessors"
            )));
        }
        let dependency: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM record_relationships WHERE from_id=?1 AND to_id=?2 AND kind='depends-on')", params![from.cast_signed(),to.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
        if !dependency || crate::ordering_dependencies::designated(c, from, to)? {
            return Err(error(format!(
                "Stack Records {from} -> {to} lack a compatible legacy dependency"
            )));
        }
    }
    let mut supplied = BTreeSet::new();
    for decision in decisions {
        if !supplied.insert((decision.from_id, decision.to_id)) {
            return Err(error("duplicate Waits-For decision"));
        }
    }
    let mut waits = Vec::new();
    let mut unresolved = Vec::new();
    for (from, to) in pairs(
        c,
        "SELECT from_id,to_id FROM record_relationships WHERE kind='depends-on' ORDER BY from_id,to_id",
    )? {
        if topology.get(&from) == Some(&to) {
            continue;
        }
        if crate::ordering_dependencies::designated(c, from, to)? {
            continue;
        }
        let mut cursor = topology.get(&from).copied();
        let mut visited = BTreeSet::new();
        while let Some(id) = cursor {
            if !visited.insert(id) {
                return Err(error(format!("Stack cycle affects Record {id}")));
            }
            cursor = topology.get(&id).copied();
        }
        let ambiguous = same_spec(c, from, to)? && !visited.contains(&to);
        if ambiguous {
            if !supplied.remove(&(from, to)) {
                unresolved.push(format!("{from} -> {to}"));
                continue;
            }
            validate_decision(c, from, to)?;
        }
        waits.push((from, to));
    }
    if !unresolved.is_empty() {
        return Err(error(format!(
            "unresolved legacy prerequisite intent for Records: {}; supply --waits-for-decisions JSON to forgeup upgrade prepare, or resolve code ancestry through the source revision's public workflow before retrying",
            unresolved.join(", ")
        )));
    }
    if !supplied.is_empty() {
        return Err(error(format!(
            "Waits-For decisions do not identify unresolved dependencies: {supplied:?}"
        )));
    }
    Ok(ConversionPlan {
        legacy,
        before,
        waits,
    })
}

pub(super) fn apply(
    c: &Connection,
    decisions: &[WaitsForDecision],
) -> Result<(), RegistrationError> {
    let ConversionPlan {
        legacy,
        before,
        waits,
    } = plan(c, decisions)?;
    for (from, to) in legacy {
        c.execute(
            "INSERT INTO builds_on_dependencies(from_id,to_id) VALUES(?1,?2)",
            params![from.cast_signed(), to.cast_signed()],
        )
        .map_err(storage_error)?;
        c.execute(
            "UPDATE task_stacks SET predecessor=NULL,source_json='{}' WHERE record_id=?1",
            [from.cast_signed()],
        )
        .map_err(storage_error)?;
    }
    for (from, to) in waits {
        c.execute(
            "INSERT INTO ordering_only_dependencies(from_id,to_id,kind) VALUES(?1,?2,'depends-on')",
            params![from.cast_signed(), to.cast_signed()],
        )
        .map_err(storage_error)?;
    }
    for (id, old) in before {
        if super::incomplete_plans::load(c, id)? != old {
            return Err(error(format!(
                "conversion changed retained Stack evidence for Record {id}"
            )));
        }
    }
    validate(c)
}
