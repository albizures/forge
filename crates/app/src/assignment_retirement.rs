//! Operator retirement finalization is distinct from a provider cancellation reply.
use crate::RegistrationError;

/// Provider adapters cannot finalize the application's retirement marker.
/// # Errors
/// Requires the atomic finalization operation instead of generic resource cleanup.
pub fn validate_resource_retirement(kind: &str) -> Result<(), RegistrationError> {
    if kind == "retirement-pending" {
        return Err(RegistrationError::new(
            "assignment-retirement-reconciliation-required",
            "retirement markers require revoked authority and atomic resource reconciliation",
        ));
    }
    Ok(())
}

/// The store supplies facts from the same transaction that finalizes retirement.
/// Retained unknown launches, effects, ownership and cancellation all block it.
/// # Errors
/// Rejects live authority or any unresolved resource other than our own marker.
pub fn validate_finalization(
    revoked: bool,
    unresolved_resources: bool,
) -> Result<(), RegistrationError> {
    if !revoked {
        return Err(RegistrationError::new(
            "assignment-not-revoked",
            "retirement finalization requires revoked Assignment authority",
        ));
    }
    if unresolved_resources {
        return Err(RegistrationError::new(
            "assignment-retirement-unreconciled",
            "retained resources remain unresolved; cancellation success does not establish safe cleanup",
        ));
    }
    Ok(())
}
