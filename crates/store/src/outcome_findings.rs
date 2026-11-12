//! Connection-local Finding effects; the caller owns the outcome transaction.
use super::{findings, storage_error};
use forge_app::{
    RegistrationError,
    checks::{RecordCheck, Verdict},
    findings::{Finding, FindingState, Severity},
    outcomes::OutcomeInput,
};
use rusqlite::{Connection, params};

pub(super) fn apply(
    c: &Connection,
    record: u64,
    check: &RecordCheck,
    assessment: u64,
    input: &OutcomeInput,
    assignment: Option<&str>,
) -> Result<Vec<Finding>, RegistrationError> {
    let revision = match &input.evidence {
        forge_app::outcomes::Evidence::Repository { commit, .. } => Some(commit.as_str()),
        forge_app::outcomes::Evidence::Artifact { .. } => None,
    };
    let mut changed = Vec::new();
    for finding in &input.findings {
        if finding.message.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-finding",
                "Finding explanation must be nonblank",
            ));
        }
        let severity = match finding.severity {
            Severity::Minor => "minor",
            Severity::Major => "major",
            Severity::Critical => "critical",
        };
        c.execute("INSERT INTO record_findings(record_id, severity, blocking, check_id, assessment_id, verification_check_id) VALUES (?1, ?2, ?3, ?4, ?5, ?4)", params![record.cast_signed(), severity, finding.blocking.unwrap_or(finding.severity != Severity::Minor), check.id.cast_signed(), assessment.cast_signed()]).map_err(storage_error)?;
        let id = c.last_insert_rowid().cast_unsigned();
        findings::append(
            c,
            record,
            id,
            "raise",
            &input.actor,
            &finding.message,
            revision,
            None,
            assignment,
        )?;
        changed.push(id);
    }
    for verification in &input.verifications {
        let finding = findings::load(c, record, verification.finding_id, check.project.clone())?;
        if finding.verification_check_id != Some(check.id) {
            return Err(RegistrationError::new(
                "unauthorized-verification",
                "only the owning review Check can verify a Finding; Implementation may only propose fixes",
            ));
        }
        if finding.version != verification.expected_version {
            return Err(RegistrationError::new(
                "stale-version",
                "Finding version changed; read it again",
            ));
        }
        if verification.message.trim().is_empty()
            || matches!(
                finding.state,
                FindingState::Resolved | FindingState::Dismissed
            )
        {
            return Err(RegistrationError::new(
                "invalid-verification",
                "verification requires an unresolved Finding and explanation",
            ));
        }
        findings::append(
            c,
            record,
            finding.id,
            "resolve",
            &input.actor,
            &verification.message,
            revision,
            None,
            assignment,
        )?;
        changed.push(finding.id);
    }
    if check.position > 0 && input.verdict == Verdict::Pass {
        for id in findings::blockers(c, record)? {
            if findings::load(c, record, id, check.project.clone())?.verification_check_id
                == Some(check.id)
            {
                return Err(RegistrationError::new(
                    "unresolved-check-findings",
                    "passing review leaves its own blocking Finding unresolved",
                ));
            }
        }
    }
    changed
        .into_iter()
        .map(|id| findings::load(c, record, id, check.project.clone()))
        .collect()
}

pub(super) fn retain_blocker_hold(
    c: &Connection,
    record: u64,
) -> Result<Vec<u64>, RegistrationError> {
    let blockers = findings::blockers(c, record)?;
    if blockers.is_empty() {
        return Ok(blockers);
    }
    // In Dispatched mode, owned Findings are work for their original reviews, not
    // an operator stop. Unlinked blockers and residual blockers after every pass
    // still require intervention. Manual execution retains its explicit-clearance policy.
    let task = super::load_record(c, record)?;
    if task.execution_mode == forge_app::ExecutionMode::Dispatched {
        let mut unlinked = false;
        for id in &blockers {
            if findings::load(c, record, *id, task.project_slugs.first().cloned())?
                .verification_check_id
                .is_none()
            {
                unlinked = true;
                break;
            }
        }
        if !unlinked {
            let mut statement = c
                .prepare("SELECT id FROM record_checks WHERE record_id = ?1")
                .map_err(storage_error)?;
            let ids = statement
                .query_map([record.cast_signed()], |row| row.get::<_, i64>(0))
                .map_err(storage_error)?;
            for id in ids {
                if super::checks::load(c, record, id.map_err(storage_error)?.cast_unsigned())?
                    .current_verdict
                    != Some(Verdict::Pass)
                {
                    return Ok(blockers);
                }
            }
        }
    }
    let reason = format!(
        "Unresolved blocking Findings {blockers:?}; assign a verification owner to unlinked Findings and obtain Check verification before completion. Explicit operator clearance is required after remediation."
    );
    // Never replace or clear a prior ownership/resource safety diagnostic.
    c.execute(
        "INSERT OR IGNORE INTO dispatch_holds(record_id, reason) VALUES (?1, ?2)",
        params![record.cast_signed(), reason],
    )
    .map_err(storage_error)?;
    Ok(blockers)
}
