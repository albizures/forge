//! Durable unattended-work limits. All mutations share the outcome/claim transaction.
use super::storage_error;
use forge_app::RegistrationError;
use rusqlite::{Connection, params};

pub(super) fn schema(c: &Connection) -> Result<(), RegistrationError> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS task_retry_counts (
        check_id INTEGER PRIMARY KEY REFERENCES record_checks(id), interruptions INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS task_progress (
        record_id INTEGER PRIMARY KEY REFERENCES records(id), cycles INTEGER NOT NULL DEFAULT 0,
        baseline TEXT, verified INTEGER NOT NULL DEFAULT 0, baseline_verified INTEGER NOT NULL DEFAULT 0,
        stop_reason TEXT);
        CREATE TABLE IF NOT EXISTS task_progress_resets (
        id INTEGER PRIMARY KEY, record_id INTEGER NOT NULL REFERENCES records(id),
        actor TEXT NOT NULL, message TEXT NOT NULL, created_at_millis INTEGER NOT NULL);") .map_err(storage_error)
}

pub(super) fn state(
    c: &Connection,
    record: u64,
) -> Result<forge_app::outcomes::TaskProgress, RegistrationError> {
    use forge_app::outcomes::{CheckRetries, ProgressReset, TaskProgress};
    use rusqlite::OptionalExtension;
    let mut state = c.query_row("SELECT cycles, baseline, verified, stop_reason FROM task_progress WHERE record_id = ?1", [record.cast_signed()], |r| {
        Ok((r.get::<_, u32>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?.cast_unsigned(), r.get::<_, Option<String>>(3)?))
    }).optional().map_err(storage_error)?.map(|(cycles, baseline, verified, stop)| -> Result<TaskProgress, RegistrationError> {
        Ok(TaskProgress { remediation_cycles: cycles, baseline: baseline.map(|s| serde_json::from_str(&s)).transpose().map_err(|e| RegistrationError::new("storage", e.to_string()))?, verified_resolutions: verified, stop_reason: stop, ..TaskProgress::default() })
    }).transpose()?.unwrap_or_default();
    state.interrupted_checks = c.prepare("SELECT r.check_id, r.interruptions FROM task_retry_counts r JOIN record_checks c ON c.id = r.check_id WHERE c.record_id = ?1 ORDER BY c.position").map_err(storage_error)?.query_map([record.cast_signed()], |r| Ok(CheckRetries { check_id: r.get::<_, i64>(0)?.cast_unsigned(), interruptions: r.get(1)? })).map_err(storage_error)?.collect::<Result<_, _>>().map_err(storage_error)?;
    state.resets = c.prepare("SELECT actor, message, created_at_millis FROM task_progress_resets WHERE record_id = ?1 ORDER BY id DESC LIMIT 20").map_err(storage_error)?.query_map([record.cast_signed()], |r| Ok(ProgressReset { actor: r.get(0)?, message: r.get(1)?, created_at_millis: r.get::<_, i64>(2)?.cast_unsigned() })).map_err(storage_error)?.collect::<Result<_, _>>().map_err(storage_error)?;
    Ok(state)
}

pub(super) fn hold(c: &Connection, record: u64, reason: &str) -> Result<(), RegistrationError> {
    c.execute(
        "INSERT OR IGNORE INTO dispatch_holds(record_id, reason) VALUES (?1, ?2)",
        params![record.cast_signed(), reason],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(super) fn retain_hold(c: &Connection, record: u64) -> Result<(), RegistrationError> {
    let reason: Option<String> = c
        .query_row(
            "SELECT (SELECT stop_reason FROM task_progress WHERE record_id = ?1)",
            [record.cast_signed()],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if let Some(reason) = reason {
        hold(c, record, &reason)?;
    }
    let exhausted: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM task_retry_counts r JOIN record_checks c ON c.id = r.check_id WHERE c.record_id = ?1 AND r.interruptions > 2)", [record.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
    if exhausted {
        hold(
            c,
            record,
            "Interrupted Check exhausted two automatic retries; complete this Check successfully through safe Manual handoff before resuming. Remediation reset does not reset interrupted retries.",
        )?;
    }
    Ok(())
}

pub(super) fn interrupted(
    c: &Connection,
    record: u64,
    check: u64,
) -> Result<(), RegistrationError> {
    c.execute("INSERT INTO task_retry_counts(check_id, interruptions) VALUES (?1, 1) ON CONFLICT(check_id) DO UPDATE SET interruptions = interruptions + 1", [check.cast_signed()]).map_err(storage_error)?;
    retain_hold(c, record)
}

pub(super) fn accepted(
    c: &Connection,
    record: u64,
    check: &forge_app::checks::RecordCheck,
    input: &forge_app::outcomes::OutcomeInput,
) -> Result<(), RegistrationError> {
    use forge_app::{checks::Verdict, outcomes::OutcomeDisposition};
    c.execute(
        "INSERT OR IGNORE INTO task_progress(record_id) VALUES (?1)",
        [record.cast_signed()],
    )
    .map_err(storage_error)?;
    c.execute(
        "UPDATE task_progress SET verified = verified + ?2 WHERE record_id = ?1",
        params![
            record.cast_signed(),
            i64::try_from(input.verifications.len())
                .map_err(|e| RegistrationError::new("storage", e.to_string()))?
        ],
    )
    .map_err(storage_error)?;
    let failed = input.disposition == OutcomeDisposition::Handback
        || (check.position == 0 && input.verdict == Verdict::Fail);
    let all_pass: bool = c
        .query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM record_checks WHERE record_id = ?1 AND position > ?2)",
            params![record.cast_signed(), check.position.cast_signed()],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if !(failed || all_pass && input.verdict == Verdict::Pass) {
        return Ok(());
    }
    let (cycles, baseline, verified, baseline_verified): (i64, Option<String>, i64, i64) = c.query_row("SELECT cycles, baseline, verified, baseline_verified FROM task_progress WHERE record_id = ?1", [record.cast_signed()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(storage_error)?;
    let evidence = serde_json::to_string(&input.evidence)
        .map_err(|e| RegistrationError::new("storage", e.to_string()))?;
    let stop = if baseline.as_ref() == Some(&evidence) && verified == baseline_verified {
        Some(
            "No progress at the end of a remediation-and-review cycle: neither deliverable evidence changed nor a Finding resolution was verified. Investigate and explicitly reset remediation with an operator explanation.",
        )
    } else if failed && cycles >= 3 {
        Some(
            "Task exhausted three remediation cycles; investigate and explicitly reset remediation with an operator explanation.",
        )
    } else {
        None
    };
    if let Some(reason) = stop {
        c.execute(
            "UPDATE task_progress SET stop_reason = COALESCE(stop_reason, ?2) WHERE record_id = ?1",
            params![record.cast_signed(), reason],
        )
        .map_err(storage_error)?;
    } else if failed {
        c.execute("UPDATE task_progress SET cycles = cycles + 1, baseline = ?2, baseline_verified = verified WHERE record_id = ?1", params![record.cast_signed(), evidence]).map_err(storage_error)?;
    } else {
        // The cycle ended successfully; retain the spent budget until explicit reset.
        c.execute(
            "UPDATE task_progress SET baseline = NULL WHERE record_id = ?1",
            [record.cast_signed()],
        )
        .map_err(storage_error)?;
    }
    retain_hold(c, record)
}

pub(super) fn passed(c: &Connection, check: u64) -> Result<(), RegistrationError> {
    c.execute(
        "UPDATE task_retry_counts SET interruptions = 0 WHERE check_id = ?1",
        [check.cast_signed()],
    )
    .map_err(storage_error)?;
    Ok(())
}
