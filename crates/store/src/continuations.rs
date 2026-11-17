//! Check-scoped checkpoint accounting, in the outcome/claim writer transaction.
use super::storage_error;
use forge_app::{
    RegistrationError,
    outcomes::{
        Attempt, CheckContinuations, ContinuationCheckpoint, ContinuationHandoff, Evidence,
    },
};
use rusqlite::{Connection, params};

fn json_error(e: impl std::fmt::Display) -> RegistrationError {
    RegistrationError::new("storage", format!("invalid continuation data: {e}"))
}

pub(super) fn state(
    c: &Connection,
    record: u64,
) -> Result<Vec<CheckContinuations>, RegistrationError> {
    // Offline upgrade validation decodes historical Stack work before expanding DDL.
    // Ordinary opening rejects these revisions before any application reads/writes.
    let revision: u32 = c
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(storage_error)?;
    if (7..15).contains(&revision) {
        return Ok(vec![]);
    }
    let rows = c.prepare("SELECT k.check_id, k.total, k.consecutive_unchanged, k.checkpoint_json FROM check_continuations k JOIN record_checks r ON r.id = k.check_id WHERE r.record_id = ?1 ORDER BY r.position").map_err(storage_error)?
        .query_map([record.cast_signed()], |r| Ok((r.get::<_, i64>(0)?.cast_unsigned(), r.get(1)?, r.get(2)?, r.get::<_, String>(3)?))).map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
    rows.into_iter()
        .map(|(check_id, total, consecutive_unchanged, json)| {
            Ok(CheckContinuations {
                check_id,
                total,
                consecutive_unchanged,
                checkpoint: serde_json::from_str(&json).map_err(json_error)?,
                resets: if revision < 16 { vec![] } else {
                    c.prepare("SELECT actor, message, created_at_millis FROM check_continuation_resets WHERE check_id = ?1 ORDER BY id")
                        .map_err(storage_error)?
                        .query_map([check_id.cast_signed()], |r| Ok(forge_app::outcomes::ProgressReset {
                            actor: r.get(0)?, message: r.get(1)?, created_at_millis: r.get::<_, i64>(2)?.cast_unsigned(),
                        })).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?
                },
            })
        })
        .collect()
}

/// Called only after Task/Workspace authorization, within the reset writer transaction.
pub(super) fn reset(
    c: &Connection,
    record: u64,
    check: u64,
    actor: &str,
    message: &str,
) -> Result<(), RegistrationError> {
    let selected: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM record_checks WHERE id = ?1 AND record_id = ?2)",
            params![check.cast_signed(), record.cast_signed()],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if !selected {
        return Err(RegistrationError::new(
            "invalid-check",
            "Check does not belong to selected Task",
        ));
    }
    c.execute(
        "UPDATE check_continuations SET total = 0, consecutive_unchanged = 0 WHERE check_id = ?1",
        [check.cast_signed()],
    )
    .map_err(storage_error)?;
    if c.changes() == 0 {
        return Err(RegistrationError::new(
            "invalid-check",
            "Check has no continuation accounting to reset",
        ));
    }
    c.execute("INSERT INTO check_continuation_resets(check_id, actor, message, created_at_millis) VALUES (?1, ?2, ?3, ?4)", params![check.cast_signed(), actor, message, super::now_millis()?]).map_err(storage_error)?;
    Ok(())
}

pub(super) fn retain_hold(c: &Connection, record: u64) -> Result<(), RegistrationError> {
    for count in state(c, record)? {
        if count.exhausted() {
            super::task_progress::hold(
                c,
                record,
                &format!(
                    "Implementation Continuation limit reached for Check {} ({} total, {} consecutive unchanged evidence). Checkpoint retained; ordinary resume or remediation reset cannot renew continuation allowance. Use dispatcher reset-continuations with the Task and Check IDs, actor and explanation; then independently reconcile safety state before explicit resume.",
                    count.check_id, count.total, count.consecutive_unchanged
                ),
            )?;
        }
    }
    Ok(())
}

pub(super) fn accept(
    c: &Connection,
    attempt: &Attempt,
    evidence: &Evidence,
    handoff: &ContinuationHandoff,
) -> Result<(), RegistrationError> {
    if state(c, attempt.record_id)?
        .iter()
        .any(|s| s.check_id == attempt.check_id && s.exhausted())
    {
        return Err(RegistrationError::new(
            "continuation-exhausted",
            "Check continuation budget is exhausted; checkpoint not accepted",
        ));
    }
    let checkpoint = ContinuationCheckpoint {
        attempt_id: attempt.id.clone(),
        evidence: evidence.clone(),
        handoff: handoff.clone(),
    };
    let unchanged = u32::from(evidence == &attempt.evidence);
    c.execute("INSERT INTO check_continuations(check_id, total, consecutive_unchanged, checkpoint_json) VALUES (?1, 1, ?2, ?3) ON CONFLICT(check_id) DO UPDATE SET total = total + 1, consecutive_unchanged = CASE WHEN ?2 = 1 THEN consecutive_unchanged + 1 ELSE 0 END, checkpoint_json = excluded.checkpoint_json", params![attempt.check_id.cast_signed(), unchanged, serde_json::to_string(&checkpoint).map_err(json_error)?]).map_err(storage_error)?;
    c.execute(
        "UPDATE record_work SET input_json = ?2, output_json = NULL WHERE record_id = ?1",
        params![
            attempt.record_id.cast_signed(),
            serde_json::to_string(evidence).map_err(json_error)?
        ],
    )
    .map_err(storage_error)?;
    retain_hold(c, attempt.record_id)
}
