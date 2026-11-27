//! Per-Assignment Nudge accounting and durable prompt intents.
use super::{SqliteProjectRepository, now_millis, storage_error};
use forge_app::RegistrationError;
use forge_app::nudges::{NudgeIntent, NudgeSettlement, NudgeState};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub(super) fn schema(c: &Connection) -> Result<(), RegistrationError> {
    c.execute_batch(include_str!("upgrade/assignment_nudges.sql"))
        .map_err(storage_error)
}

fn lock(
    repository: &SqliteProjectRepository,
) -> Result<std::sync::MutexGuard<'_, Connection>, RegistrationError> {
    repository
        .connection
        .lock()
        .map_err(|_| RegistrationError::new("storage", "storage lock poisoned"))
}

pub(super) fn state(
    repository: &SqliteProjectRepository,
    assignment: &str,
) -> Result<NudgeState, RegistrationError> {
    let c = lock(repository)?;
    let mut state = c
        .query_row(
            "SELECT last_completion_seq, total, consecutive_no_commit, last_head, pending_explanation FROM assignment_nudges WHERE assignment_id = ?1",
            [assignment],
            |r| {
                Ok(NudgeState {
                    last_completion_seq: r.get::<_, i64>(0)?.cast_unsigned(),
                    total: r.get::<_, i64>(1)?.try_into().unwrap_or(u32::MAX),
                    consecutive_no_commit: r.get::<_, i64>(2)?.try_into().unwrap_or(u32::MAX),
                    last_head: r.get(3)?,
                    pending_explanation: r.get(4)?,
                    prompt_sequence: 0,
                })
            },
        )
        .optional()
        .map_err(storage_error)?
        .unwrap_or_default();
    state.prompt_sequence = c
        .query_row(
            "SELECT COALESCE(MAX(sequence), 0) FROM assignment_prompt_intents WHERE assignment_id = ?1",
            [assignment],
            |r| r.get::<_, i64>(0),
        )
        .map_err(storage_error)?
        .try_into()
        .unwrap_or(u32::MAX);
    Ok(state)
}

/// Restart the counters of every open Assignment of a Task at an operator resume, keeping the
/// prompt sequence, finished-turn marker and HEAD baseline. The explanation, if any, is held
/// until the next Nudge carries it. The caller owns the transaction.
pub(super) fn reset_for_resume(
    c: &rusqlite::Connection,
    record: u64,
    explanation: Option<&str>,
) -> Result<(), RegistrationError> {
    c.execute(
        "INSERT INTO assignment_nudges (assignment_id, last_completion_seq, total, consecutive_no_commit, last_head, pending_explanation)
         SELECT id, 0, 0, 0, NULL, ?2 FROM agent_assignments WHERE record_id = ?1 AND closed_at_millis IS NULL
         ON CONFLICT(assignment_id) DO UPDATE SET total = 0, consecutive_no_commit = 0, pending_explanation = excluded.pending_explanation",
        params![record.cast_signed(), explanation],
    )
    .map(|_| ())
    .map_err(storage_error)
}

pub(super) fn begin(
    repository: &SqliteProjectRepository,
    assignment: &str,
    intent: &NudgeIntent,
) -> Result<bool, RegistrationError> {
    let c = lock(repository)?;
    // The primary key makes the (Assignment, sequence) intent exclusive across writers. The
    // sequence is the Assignment's own prompt count, independent of the resettable counters.
    let inserted = c
        .execute(
            "INSERT INTO assignment_prompt_intents (assignment_id, sequence, completion_seq, consecutive_no_commit, head, created_at_millis)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6
             WHERE COALESCE((SELECT MAX(sequence) FROM assignment_prompt_intents WHERE assignment_id = ?1), 0) + 1 = ?2
             ON CONFLICT DO NOTHING",
            params![
                assignment,
                intent.sequence,
                intent.completion_seq.cast_signed(),
                intent.consecutive_no_commit,
                intent.head,
                now_millis()?
            ],
        )
        .map_err(storage_error)?;
    Ok(inserted == 1)
}

pub(super) fn unsettled(
    repository: &SqliteProjectRepository,
    assignment: &str,
) -> Result<Option<NudgeIntent>, RegistrationError> {
    let c = lock(repository)?;
    c.query_row(
        "SELECT sequence, completion_seq, consecutive_no_commit, head FROM assignment_prompt_intents WHERE assignment_id = ?1 AND settlement IS NULL ORDER BY sequence LIMIT 1",
        [assignment],
        |r| {
            Ok(NudgeIntent {
                sequence: r.get(0)?,
                completion_seq: r.get::<_, i64>(1)?.cast_unsigned(),
                consecutive_no_commit: r.get(2)?,
                head: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(storage_error)
}

pub(super) fn settle(
    repository: &SqliteProjectRepository,
    assignment: &str,
    sequence: u32,
    settlement: NudgeSettlement,
) -> Result<(), RegistrationError> {
    let mut c = lock(repository)?;
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage_error)?;
    let label = match settlement {
        NudgeSettlement::Confirmed => "confirmed",
        NudgeSettlement::Uncertain => "uncertain",
    };
    let settled = tx
        .execute(
            "UPDATE assignment_prompt_intents SET settled_at_millis = ?3, settlement = ?4 WHERE assignment_id = ?1 AND sequence = ?2 AND settlement IS NULL",
            params![assignment, sequence, now_millis()?, label],
        )
        .map_err(storage_error)?;
    if settled == 1 {
        tx.execute(
            "INSERT INTO assignment_nudges (assignment_id, last_completion_seq, total, consecutive_no_commit, last_head, pending_explanation)
             SELECT assignment_id, completion_seq, 1, consecutive_no_commit, head, NULL FROM assignment_prompt_intents WHERE assignment_id = ?1 AND sequence = ?2
             ON CONFLICT(assignment_id) DO UPDATE SET last_completion_seq = excluded.last_completion_seq,
                total = assignment_nudges.total + 1, consecutive_no_commit = excluded.consecutive_no_commit,
                last_head = excluded.last_head, pending_explanation = NULL",
            params![assignment, sequence],
        )
        .map_err(storage_error)?;
    }
    tx.commit().map_err(storage_error)
}

pub(super) fn cancel(
    repository: &SqliteProjectRepository,
    assignment: &str,
    sequence: u32,
) -> Result<(), RegistrationError> {
    lock(repository)?
        .execute(
            "DELETE FROM assignment_prompt_intents WHERE assignment_id = ?1 AND sequence = ?2 AND settlement IS NULL",
            params![assignment, sequence],
        )
        .map(|_| ())
        .map_err(storage_error)
}

pub(super) fn attempt_outcome_accepted(
    repository: &SqliteProjectRepository,
    assignment: &str,
) -> Result<bool, RegistrationError> {
    lock(repository)?
        .query_row(
            "SELECT COALESCE((SELECT a.closed FROM assignment_check_attempts b JOIN record_attempts a ON a.id = b.attempt_id WHERE b.assignment_id = ?1), 0)",
            [assignment],
            |r| r.get(0),
        )
        .map_err(storage_error)
}
