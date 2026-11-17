//! Writer-transaction enforcement for initial Spec plans. No external observations.
use super::{load_record, stacks, storage_error};
use forge_app::{RecordStatus, RecordType, RegistrationError};
use rusqlite::{Connection, OptionalExtension};

pub(super) fn editable_spec(c: &Connection, spec: u64) -> Result<(), RegistrationError> {
    let record = load_record(c, spec)?;
    if record.record_type == RecordType::Spec && record.status != RecordStatus::Open {
        return Err(RegistrationError::new(
            "spec-plan-started",
            format!(
                "Spec {spec} has started; ordinary structural edits are disabled; preserve prepared work and use explicit supported recovery"
            ),
        ));
    }
    Ok(())
}

pub(super) fn editable_task(c: &Connection, task: u64) -> Result<(), RegistrationError> {
    let spec: Option<i64> = c.query_row(
        "SELECT r.id FROM record_relationships p JOIN records r ON r.id = p.parent_id WHERE p.child_id = ?1 AND p.kind = 'parent-child' AND r.record_type = 'spec'",
        [task.cast_signed()], |r| r.get(0),
    ).optional().map_err(storage_error)?;
    if let Some(spec) = spec {
        editable_spec(c, spec.cast_unsigned())?;
    }
    Ok(())
}

pub(super) fn editable_stack_suffix(c: &Connection, task: u64) -> Result<(), RegistrationError> {
    let mut statement = c.prepare(
        "WITH RECURSIVE links(record_id, predecessor) AS (SELECT record_id, predecessor FROM task_stacks UNION SELECT from_id, to_id FROM builds_on_dependencies), suffix(id) AS (SELECT ?1 UNION SELECT record_id FROM links JOIN suffix ON predecessor = suffix.id) SELECT id FROM suffix"
    ).map_err(storage_error)?;
    let ids = statement
        .query_map([task.cast_signed()], |r| r.get::<_, i64>(0))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    for id in ids {
        editable_task(c, id.cast_unsigned())?;
    }
    Ok(())
}

/// Parent/plan preflight is also used before external preparation. It never mutates state.
pub(super) fn execution_plan(c: &Connection, task: u64) -> Result<(), RegistrationError> {
    if load_record(c, task)?.record_type != RecordType::Task {
        return Ok(());
    }
    let spec: Option<i64> = c.query_row(
        "SELECT r.id FROM record_relationships p JOIN records r ON r.id = p.parent_id WHERE p.child_id = ?1 AND p.kind = 'parent-child' AND r.record_type = 'spec'",
        [task.cast_signed()], |r| r.get(0),
    ).optional().map_err(storage_error)?;
    if let Some(spec) = spec {
        let spec = spec.cast_unsigned();
        forge_app::spec_plans::validate_execution_parent(task, &load_record(c, spec)?)?;
        // Legacy In Progress is not proof that its plan was validated at start.
        validate(c, spec)?;
    }
    // Standalone chains have the same execution contract as Spec children.
    // Planning may omit root configuration or compatible Checks; authorization may not.
    let mut cursor = Some(task);
    let mut visited = std::collections::HashSet::new();
    while let Some(id) = cursor {
        if !visited.insert(id) {
            return Err(RegistrationError::new("stack-blocked", "Stack cycle"));
        }
        let Some(stack) = stacks::load(c, id)? else {
            break;
        };
        stacks::compatible(c, id)?;
        if id != task && load_record(c, id)?.status == RecordStatus::Done {
            stack.verified_commit(&super::outcomes::state(c, id)?)?;
        }
        cursor = stack.predecessor;
    }
    Ok(())
}

/// Call in the writer transaction that authorizes work, not just at client preflight.
pub(super) fn authorize(
    c: &Connection,
    task: u64,
    evidence: Option<&forge_app::outcomes::Evidence>,
) -> Result<(), RegistrationError> {
    execution_plan(c, task)?;
    stacks::execution_gate(c, task, evidence)
}

/// A retained Provisioning Assignment is not permission to launch stale work after restart.
pub(super) fn assignment_execution(
    c: &Connection,
    assignment: &str,
) -> Result<(), RegistrationError> {
    let record: Option<i64> = c
        .query_row(
            "SELECT record_id FROM agent_assignments WHERE id = ?1 AND closed_at_millis IS NULL",
            [assignment],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(record) = record {
        let record = record.cast_unsigned();
        let attempt = super::task_attempts::binding(c, assignment)?;
        authorize(c, record, attempt.as_ref().map(|a| &a.evidence))?;
        if let Some(attempt) = attempt
            && super::outcomes::state(c, record)?.generation != attempt.generation
        {
            return Err(RegistrationError::new(
                "stale-generation",
                "retire the stale Assignment before another launch",
            ));
        }
        let unfinished = super::unfinished_prerequisite_ids(c, record)?;
        if !unfinished.is_empty() {
            return Err(super::dependencies_not_done_error(&unfinished));
        }
    }
    Ok(())
}

pub(super) fn validate(c: &Connection, spec: u64) -> Result<(), RegistrationError> {
    let mut statement = c.prepare(
        "SELECT child_id FROM record_relationships WHERE parent_id = ?1 AND kind = 'parent-child' ORDER BY child_id"
    ).map_err(storage_error)?;
    let ids = statement
        .query_map([spec.cast_signed()], |r| r.get::<_, i64>(0))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let parent = load_record(c, spec)?;
    let tasks = ids
        .into_iter()
        .map(|id| load_record(c, id.cast_unsigned()))
        .collect::<Result<Vec<_>, _>>()?;
    for task in &tasks {
        if task.record_type != RecordType::Task
            || task.project_slugs != parent.project_slugs
            || task.workspace_id != parent.workspace_id
        {
            return Err(RegistrationError::new(
                "invalid-spec-plan",
                format!(
                    "Spec {spec} has invalid child {} parentage or scope",
                    task.id
                ),
            ));
        }
        let mut lineage = Vec::new();
        let mut cursor = Some(task.id);
        while let Some(id) = cursor {
            if lineage
                .iter()
                .any(|s: &forge_app::stacks::TaskStack| s.record_id == id)
            {
                return Err(RegistrationError::new("invalid-spec-plan", "Stack cycle"));
            }
            let Some(stack) = stacks::load(c, id)? else {
                break;
            };
            stacks::compatible(c, id).map_err(|error| {
                RegistrationError::new(
                    "invalid-spec-plan",
                    format!("Spec {spec}, Task {id}: {}", error.message),
                )
            })?;
            if let Some(previous) = stack.predecessor {
                let previous_record = load_record(c, previous)?;
                forge_app::stacks::validate_stack_predecessor(
                    &load_record(c, id)?,
                    &previous_record,
                )?;
                let member = load_record(c, id)?;
                if !member.relationships.iter().any(|r| {
                    matches!(
                        r.prerequisite_kind(),
                        Some(
                            forge_app::PrerequisiteKind::BuildsOn
                                | forge_app::PrerequisiteKind::LegacyDependsOn
                        )
                    ) && r.from_id == id
                        && r.to_id == previous
                }) {
                    return Err(RegistrationError::new(
                        "invalid-spec-plan",
                        format!(
                            "Task {id} must depend on Stack Predecessor {previous} without ordering-only intent"
                        ),
                    ));
                }
            }
            cursor = stack.predecessor;
            lineage.push(stack);
        }
        forge_app::spec_plans::validate_task_inheritance(task, &tasks, &lineage)?;
    }
    Ok(())
}
