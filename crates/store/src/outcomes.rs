//! One transactional boundary for Manual attempt identity, freshness, evidence and acceptance.
use super::{
    SqliteProjectRepository, checks, insert_update, load_record, now_millis, storage_error,
};
use forge_app::checks::Verdict;
use forge_app::outcomes::{
    AcceptedOutcome, Attempt, AttemptInput, Evidence, OutcomeInput, WorkInvalidation,
    WorkInvalidationInput, WorkState,
};
use forge_app::{RecordStatus, RecordUpdateKind, RegistrationError};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub(super) fn schema(c: &Connection) -> Result<(), RegistrationError> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS record_work (
        record_id INTEGER PRIMARY KEY REFERENCES records(id), generation INTEGER NOT NULL DEFAULT 1 CHECK(generation > 0),
        input_json TEXT, output_json TEXT);
        CREATE TABLE IF NOT EXISTS record_attempts (
        id TEXT PRIMARY KEY, record_id INTEGER NOT NULL REFERENCES records(id), check_id INTEGER NOT NULL REFERENCES record_checks(id),
        attempt_json TEXT NOT NULL, closed INTEGER NOT NULL DEFAULT 0 CHECK(closed IN (0,1)), outcome_json TEXT, result_json TEXT,
        CHECK((outcome_json IS NULL) = (result_json IS NULL)), CHECK(outcome_json IS NULL OR closed = 1));
        CREATE UNIQUE INDEX IF NOT EXISTS one_open_record_attempt ON record_attempts(record_id) WHERE closed = 0;
        CREATE TABLE IF NOT EXISTS record_work_invalidations (
        id INTEGER PRIMARY KEY, record_id INTEGER NOT NULL REFERENCES records(id), generation INTEGER NOT NULL,
        actor TEXT NOT NULL, message TEXT NOT NULL, created_at_millis INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS work_invalidations_by_record ON record_work_invalidations(record_id, id);")
        .map_err(storage_error)
}
fn json_error(e: impl std::fmt::Display) -> RegistrationError {
    RegistrationError::new("storage", format!("invalid persisted outcome data: {e}"))
}
fn error(code: &'static str, message: &str) -> RegistrationError {
    RegistrationError::new(code, message)
}
fn attribution(actor: &str, message: &str) -> Result<(), RegistrationError> {
    if actor.trim().is_empty() || message.trim().is_empty() {
        return Err(error(
            "invalid-outcome",
            "actor and explanation/identity must be nonblank",
        ));
    }
    Ok(())
}
pub(super) fn state(c: &Connection, record: u64) -> Result<WorkState, RegistrationError> {
    let row: Option<(u64, Option<String>, Option<String>)> = c
        .query_row(
            "SELECT generation, input_json, output_json FROM record_work WHERE record_id = ?1",
            [record.cast_signed()],
            |r| Ok((r.get::<_, i64>(0)?.cast_unsigned(), r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let (generation, input, output) = row.unwrap_or((1, None, None));
    let latest_invalidation = c.query_row("SELECT generation, actor, message, created_at_millis FROM record_work_invalidations WHERE record_id = ?1 ORDER BY id DESC LIMIT 1", [record.cast_signed()], |r| Ok(WorkInvalidation { generation: r.get::<_, i64>(0)?.cast_unsigned(), actor: r.get(1)?, message: r.get(2)?, created_at_millis: r.get::<_, i64>(3)?.cast_unsigned() })).optional().map_err(storage_error)?;
    Ok(WorkState {
        generation,
        latest_invalidation,
        input: input
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(json_error)?,
        output: output
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(json_error)?,
    })
}
fn generation(current: u64, expected: u64) -> Result<(), RegistrationError> {
    if current != expected {
        return Err(error(
            "stale-generation",
            "work freshness changed; begin a new attempt",
        ));
    }
    Ok(())
}
fn mutable(c: &Connection, record: u64) -> Result<(), RegistrationError> {
    if load_record(c, record)?.status != RecordStatus::InProgress {
        return Err(error(
            "invalid-transition",
            "Manual attempts require an In Progress Task",
        ));
    }
    // Ownership/resource reconciliation and mode gates are extended by the mode slice.
    let assigned: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM agent_assignments WHERE record_id = ?1 AND closed_at_millis IS NULL)", [record.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
    if assigned {
        return Err(error(
            "assignment-active",
            "retire existing Assignment ownership before Manual work",
        ));
    }
    Ok(())
}
fn ordered(c: &Connection, record: u64, check: u64) -> Result<(), RegistrationError> {
    let current = checks::load(c, record, check)?;
    let ids = c
        .prepare(
            "SELECT id FROM record_checks WHERE record_id = ?1 AND position < ?2 ORDER BY position",
        )
        .map_err(storage_error)?
        .query_map(
            params![record.cast_signed(), current.position.cast_signed()],
            |r| r.get::<_, i64>(0).map(i64::cast_unsigned),
        )
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    for id in ids {
        if checks::load(c, record, id)?.current_verdict != Some(Verdict::Pass) {
            return Err(error(
                "check-order",
                "all earlier Checks must currently pass before this Check",
            ));
        }
    }
    if current.current_verdict == Some(Verdict::Pass) {
        return Err(error(
            "check-already-passed",
            "invalidate work before reassessing a passing Check",
        ));
    }
    Ok(())
}
/// Invalidate every current Assessment and every outstanding attempt, without reviving history.
/// Kept connection-local so body edits, explicit invalidation and later handback are atomic.
pub(super) fn invalidate(
    c: &Connection,
    record: u64,
    actor: &str,
    message: &str,
    assignment: Option<&str>,
) -> Result<(), RegistrationError> {
    let before = state(c, record)?;
    if before.generation >= i64::MAX.cast_unsigned() {
        return Err(error("generation-exhausted", "work freshness is exhausted"));
    }
    c.execute("INSERT INTO record_work(record_id, generation) VALUES (?1, 2) ON CONFLICT(record_id) DO UPDATE SET generation = generation + 1, output_json = NULL", [record.cast_signed()]).map_err(storage_error)?;
    c.execute(
        "UPDATE record_attempts SET closed = 1 WHERE record_id = ?1 AND closed = 0",
        [record.cast_signed()],
    )
    .map_err(storage_error)?;
    let latest = c.prepare("SELECT c.id, a.id FROM record_checks c JOIN check_assessments a ON a.id = (SELECT max(id) FROM check_assessments WHERE check_id = c.id) WHERE c.record_id = ?1 AND NOT EXISTS (SELECT 1 FROM check_invalidations WHERE assessment_id = a.id)").map_err(storage_error)?
        .query_map([record.cast_signed()], |r| Ok((r.get::<_, i64>(0)?.cast_unsigned(), r.get::<_, i64>(1)?.cast_unsigned()))).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
    for (check, assessment) in latest {
        c.execute("INSERT INTO check_invalidations(assessment_id, actor, message, created_at_millis, assignment_id) VALUES (?1, ?2, ?3, ?4, ?5)", params![assessment.cast_signed(), actor, message, now_millis()?, assignment]).map_err(storage_error)?;
        let id = c.last_insert_rowid();
        c.execute("INSERT INTO check_activities(check_id, assessment_id, invalidation_id) VALUES (?1, ?2, ?3)", params![check.cast_signed(), assessment.cast_signed(), id]).map_err(storage_error)?;
        insert_update(
            c,
            record,
            RecordUpdateKind::CheckInvalidated {
                assessment_id: assessment,
            },
        )?;
    }
    c.execute(
        "UPDATE record_checks SET version = version + 1 WHERE record_id = ?1",
        [record.cast_signed()],
    )
    .map_err(storage_error)?;
    c.execute("INSERT INTO record_work_invalidations(record_id, generation, actor, message, created_at_millis) VALUES (?1, ?2, ?3, ?4, ?5)", params![record.cast_signed(), (before.generation + 1).cast_signed(), actor, message, now_millis()?]).map_err(storage_error)?;
    Ok(())
}
impl SqliteProjectRepository {
    pub(super) fn work_state(&self, record: u64) -> Result<WorkState, RegistrationError> {
        let mut c = self
            .connection
            .lock()
            .map_err(|_| error("storage", "storage lock poisoned"))?;
        let tx = c.transaction().map_err(storage_error)?;
        self.check_task(&tx, record)?;
        state(&tx, record)
    }
    pub(super) fn begin_attempt(
        &self,
        record: u64,
        input: &AttemptInput,
    ) -> Result<Attempt, RegistrationError> {
        let mut c = self
            .connection
            .lock()
            .map_err(|_| error("storage", "storage lock poisoned"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let task = self.check_task(&tx, record)?;
        input.evidence.validate()?;
        attribution(&input.actor, &input.id)?;
        let attempt = Attempt {
            id: input.id.clone(),
            record_id: record,
            check_id: input.check_id,
            generation: input.generation,
            evidence: input.evidence.clone(),
            actor: input.actor.clone(),
            body: task.body,
        };
        let existing: Option<String> = tx
            .query_row(
                "SELECT attempt_json FROM record_attempts WHERE id = ?1",
                [&input.id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some(json) = existing {
            let old: Attempt = serde_json::from_str(&json).map_err(json_error)?;
            if old == attempt {
                return Ok(old);
            }
            return Err(error(
                "attempt-conflict",
                "attempt identity already binds different work",
            ));
        }
        mutable(&tx, record)?;
        let work = state(&tx, record)?;
        generation(work.generation, input.generation)?;
        ordered(&tx, record, input.check_id)?;
        let check = checks::load(&tx, record, input.check_id)?;
        let bound = if check.position == 0 {
            work.input
        } else {
            work.output
        };
        if bound.as_ref().is_some_and(|e| e != &input.evidence)
            || (check.position > 0 && bound.is_none())
        {
            return Err(error(
                "evidence-mismatch",
                "attempt must bind the current input (Implementation) or output (review) evidence",
            ));
        }
        let open: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM record_attempts WHERE record_id = ?1 AND closed = 0)",
                [record.cast_signed()],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        if open {
            return Err(error(
                "attempt-active",
                "close or invalidate the current attempt before beginning another",
            ));
        }
        if check.position == 0 {
            tx.execute("INSERT INTO record_work(record_id, input_json) VALUES (?1, ?2) ON CONFLICT(record_id) DO UPDATE SET input_json = excluded.input_json", params![record.cast_signed(), serde_json::to_string(&input.evidence).map_err(json_error)?]).map_err(storage_error)?;
        }
        tx.execute("INSERT INTO record_attempts(id, record_id, check_id, attempt_json) VALUES (?1, ?2, ?3, ?4)", params![input.id, record.cast_signed(), input.check_id.cast_signed(), serde_json::to_string(&attempt).map_err(json_error)?]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(attempt)
    }
    pub(super) fn submit_outcome(
        &self,
        record: u64,
        input: &OutcomeInput,
    ) -> Result<AcceptedOutcome, RegistrationError> {
        let mut c = self
            .connection
            .lock()
            .map_err(|_| error("storage", "storage lock poisoned"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        self.check_task(&tx, record)?;
        input.evidence.validate()?;
        attribution(&input.actor, &input.message)?;
        let row: Option<(String, bool, Option<String>, Option<String>)> = tx.query_row("SELECT attempt_json, closed, outcome_json, result_json FROM record_attempts WHERE id = ?1 AND record_id = ?2", params![input.attempt_id, record.cast_signed()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional().map_err(storage_error)?;
        let (json, closed, outcome, result) =
            row.ok_or_else(|| error("attempt-not-found", "attempt does not belong to this Task"))?;
        if let Some(outcome) = outcome {
            let accepted: OutcomeInput = serde_json::from_str(&outcome).map_err(json_error)?;
            if &accepted != input {
                return Err(error(
                    "outcome-conflict",
                    "attempt already accepted a different outcome",
                ));
            }
            return serde_json::from_str(
                &result
                    .ok_or_else(|| error("storage", "accepted outcome has no durable result"))?,
            )
            .map_err(json_error);
        }
        let attempt: Attempt = serde_json::from_str(&json).map_err(json_error)?;
        if attempt.actor != input.actor {
            return Err(error(
                "unauthorized-attempt",
                "outcome actor must own the Manual attempt",
            ));
        }
        let work = state(&tx, record)?;
        generation(work.generation, input.generation)?;
        generation(attempt.generation, input.generation)?;
        if closed {
            return Err(error(
                "attempt-closed",
                "attempt is closed without an accepted outcome",
            ));
        }
        mutable(&tx, record)?;
        if load_record(&tx, record)?.body != attempt.body {
            return Err(error("stale-generation", "Task inputs changed"));
        }
        ordered(&tx, record, attempt.check_id)?;
        let check = checks::load(&tx, record, attempt.check_id)?;
        if check.position > 0
            && (input.evidence != attempt.evidence || work.output.as_ref() != Some(&input.evidence))
        {
            return Err(error(
                "evidence-mismatch",
                "review outcome must assess its bound output evidence",
            ));
        }
        let revision = match &input.evidence {
            Evidence::Repository { commit, .. } => Some(commit.as_str()),
            Evidence::Artifact { .. } => None,
        };
        tx.execute("INSERT INTO check_assessments(check_id, verdict, actor, message, created_at_millis, revision, attempt_id, generation, evidence_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", params![attempt.check_id.cast_signed(), if input.verdict == Verdict::Pass { "pass" } else { "fail" }, input.actor, input.message, now_millis()?, revision, attempt.id, work.generation.cast_signed(), serde_json::to_string(&input.evidence).map_err(json_error)?]).map_err(storage_error)?;
        let assessment_id = tx.last_insert_rowid().cast_unsigned();
        tx.execute(
            "INSERT INTO check_activities(check_id, assessment_id) VALUES (?1, ?2)",
            params![attempt.check_id.cast_signed(), assessment_id.cast_signed()],
        )
        .map_err(storage_error)?;
        tx.execute(
            "UPDATE record_checks SET version = version + 1 WHERE id = ?1",
            [attempt.check_id.cast_signed()],
        )
        .map_err(storage_error)?;
        if check.position == 0 && input.verdict == Verdict::Pass {
            tx.execute(
                "UPDATE record_work SET output_json = ?2 WHERE record_id = ?1",
                params![
                    record.cast_signed(),
                    serde_json::to_string(&input.evidence).map_err(json_error)?
                ],
            )
            .map_err(storage_error)?;
        }
        insert_update(
            &tx,
            record,
            RecordUpdateKind::CheckAssessed { assessment_id },
        )?;
        let result = AcceptedOutcome {
            attempt,
            evidence: input.evidence.clone(),
            check: checks::load(&tx, record, check.id)?,
        };
        tx.execute("UPDATE record_attempts SET closed = 1, outcome_json = ?2, result_json = ?3 WHERE id = ?1", params![input.attempt_id, serde_json::to_string(input).map_err(json_error)?, serde_json::to_string(&result).map_err(json_error)?]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }
    pub(super) fn invalidate_work(
        &self,
        record: u64,
        input: &WorkInvalidationInput,
    ) -> Result<WorkState, RegistrationError> {
        let mut c = self
            .connection
            .lock()
            .map_err(|_| error("storage", "storage lock poisoned"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        self.check_task(&tx, record)?;
        mutable(&tx, record)?;
        input.evidence.validate()?;
        attribution(&input.actor, &input.message)?;
        generation(state(&tx, record)?.generation, input.generation)?;
        invalidate(&tx, record, &input.actor, &input.message, None)?;
        tx.execute(
            "UPDATE record_work SET input_json = ?2 WHERE record_id = ?1",
            params![
                record.cast_signed(),
                serde_json::to_string(&input.evidence).map_err(json_error)?
            ],
        )
        .map_err(storage_error)?;
        let result = state(&tx, record)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }
}
