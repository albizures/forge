//! Initial Spec plan policy; dependency ordering is distinct from repository inheritance.
use crate::{PrerequisiteKind, Record, RegistrationError, stacks::TaskStack};

/// A Spec authorizes child execution only while In Progress; status is not plan evidence.
/// # Errors
/// Returns an actionable parent lifecycle rejection. Other parent types have no Spec gate.
pub fn validate_execution_parent(task: u64, parent: &Record) -> Result<(), RegistrationError> {
    if parent.record_type == crate::RecordType::Spec
        && parent.status != crate::RecordStatus::InProgress
    {
        return Err(RegistrationError::new(
            "spec-execution-blocked",
            format!(
                "Task {task}: parent Spec {} must be In Progress; validate and start the Spec before execution",
                parent.id
            ),
        ));
    }
    Ok(())
}

/// Validate required same-Spec inheritance against explicitly configured Stack lineage.
/// Independent Tasks and explicit Waits-For Relationships do not require a Stack.
/// # Errors
/// Rejects missing inheritance, contradictory ordering, and ambiguous branches.
pub fn validate_task_inheritance(
    task: &Record,
    siblings: &[Record],
    lineage: &[TaskStack],
) -> Result<(), RegistrationError> {
    for dependency in &task.relationships {
        if dependency.prerequisite_kind().is_none()
            || dependency.from_id != task.id
            || !siblings.iter().any(|r| r.id == dependency.to_id)
        {
            continue;
        }
        let inherited = lineage
            .iter()
            .skip(1)
            .any(|s| s.record_id == dependency.to_id);
        if dependency.prerequisite_kind() == Some(PrerequisiteKind::WaitsFor) {
            if lineage
                .first()
                .is_some_and(|s| s.predecessor == Some(dependency.to_id))
            {
                return Err(blocked(
                    task.id,
                    dependency.to_id,
                    "Waits-For Relationship is a Stack Predecessor",
                ));
            }
        } else if !inherited {
            return Err(blocked(
                task.id,
                dependency.to_id,
                "missing required Stack inheritance or ambiguous code prerequisites; configure a cumulative Stack or explicitly replace this dependency with waits-for",
            ));
        }
    }
    Ok(())
}

fn blocked(task: u64, prerequisite: u64, reason: &str) -> RegistrationError {
    RegistrationError::new(
        "invalid-spec-plan",
        format!("Task {task}, prerequisite {prerequisite}: {reason}"),
    )
}
