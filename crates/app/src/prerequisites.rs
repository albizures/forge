//! Shared prerequisite endpoint policy; adapters do not reinterpret intent.
use crate::{Record, RecordType, RegistrationError};

/// Validate completion-prerequisite endpoints within an already authorized Workspace.
/// Both legacy dependencies and Waits-For preserve this eligibility matrix.
///
/// # Errors
/// Rejects incompatible Record Types or Task-to-Grilling Project Scope.
pub fn validate_endpoints(
    blocked: &Record,
    prerequisite: &Record,
) -> Result<(), RegistrationError> {
    let eligible = blocked.record_type == prerequisite.record_type
        && matches!(blocked.record_type, RecordType::Task | RecordType::Spec)
        || blocked.record_type == RecordType::Task
            && prerequisite.record_type == RecordType::Grilling
            && (blocked.project_slugs.is_empty() && prerequisite.project_slugs.is_empty()
                || blocked.project_slugs.len() == 1
                    && prerequisite
                        .project_slugs
                        .contains(&blocked.project_slugs[0]));
    if eligible {
        Ok(())
    } else {
        Err(RegistrationError::new(
            "invalid-relationship",
            "completion prerequisites require same-type Task or Spec Records, or a Task scoped within a Grilling",
        ))
    }
}
