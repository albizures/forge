use super::{SqliteProjectRepository, insert_update, load_record, now_millis, storage_error};
use forge_app::checks::{
    Assessment, AssessmentInput, CheckActivity, CheckPage, IMPLEMENTATION_CRITERION, Invalidation,
    InvalidationInput, RevisionComparison, TaskCheck, Verdict,
};
use forge_app::{Record, RecordStatus, RecordType, RecordUpdateKind, RegistrationError};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn schema(connection: &Connection) -> Result<(), RegistrationError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS task_checks (
        id INTEGER PRIMARY KEY NOT NULL, task_id INTEGER NOT NULL REFERENCES records(id),
        name TEXT NOT NULL, description TEXT NOT NULL, position INTEGER NOT NULL,
        version INTEGER NOT NULL DEFAULT 1, UNIQUE(task_id, position), UNIQUE(task_id, name));
        CREATE TABLE IF NOT EXISTS check_assessments (
        id INTEGER PRIMARY KEY NOT NULL, check_id INTEGER NOT NULL REFERENCES task_checks(id),
        verdict TEXT NOT NULL CHECK(verdict IN ('pass','fail')), actor TEXT NOT NULL,
        message TEXT NOT NULL, created_at_millis INTEGER NOT NULL, revision TEXT, dirty INTEGER, assignment_id TEXT);
        CREATE INDEX IF NOT EXISTS assessments_by_check ON check_assessments(check_id, id);
        CREATE TABLE IF NOT EXISTS check_invalidations (
        id INTEGER PRIMARY KEY NOT NULL, assessment_id INTEGER NOT NULL UNIQUE REFERENCES check_assessments(id),
        actor TEXT NOT NULL, message TEXT NOT NULL, created_at_millis INTEGER NOT NULL, revision TEXT, dirty INTEGER, assignment_id TEXT);
        CREATE TABLE IF NOT EXISTS check_activities (
        id INTEGER PRIMARY KEY NOT NULL, check_id INTEGER NOT NULL REFERENCES task_checks(id),
        assessment_id INTEGER NOT NULL REFERENCES check_assessments(id),
        invalidation_id INTEGER REFERENCES check_invalidations(id));
        CREATE INDEX IF NOT EXISTS activities_by_check ON check_activities(check_id, id);",
        )
        .map_err(storage_error)
}

pub(super) fn provision(connection: &Connection, task: u64) -> Result<(), RegistrationError> {
    connection.execute("INSERT INTO task_checks (task_id, name, description, position) VALUES (?1, 'Implementation', ?2, 0)",
        params![task.cast_signed(), IMPLEMENTATION_CRITERION]).map_err(storage_error)?;
    Ok(())
}

fn assessment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Assessment> {
    Ok(Assessment {
        id: row.get::<_, i64>(0)?.cast_unsigned(),
        check_id: row.get::<_, i64>(1)?.cast_unsigned(),
        verdict: match row.get::<_, String>(2)?.as_str() {
            "pass" => Verdict::Pass,
            "fail" => Verdict::Fail,
            _ => return Err(rusqlite::Error::InvalidQuery),
        },
        assignment_id: row.get(8)?,
        actor: row.get(3)?,
        message: row.get(4)?,
        created_at_millis: row.get::<_, i64>(5)?.cast_unsigned(),
        revision: row.get(6)?,
        dirty: row.get(7)?,
        invalidation: None,
        comparison: RevisionComparison::Unknown,
    })
}
const ASSESSMENT_COLUMNS: &str =
    "id, check_id, verdict, actor, message, created_at_millis, revision, dirty, assignment_id";

fn invalidation(
    connection: &Connection,
    assessment_id: u64,
) -> Result<Option<Invalidation>, RegistrationError> {
    connection.query_row("SELECT id, assessment_id, actor, message, created_at_millis, revision, dirty, assignment_id FROM check_invalidations WHERE assessment_id = ?1", [assessment_id.cast_signed()], |row| Ok(Invalidation {
        id: row.get::<_, i64>(0)?.cast_unsigned(), assessment_id: row.get::<_, i64>(1)?.cast_unsigned(),
        actor: row.get(2)?, message: row.get(3)?, created_at_millis: row.get::<_, i64>(4)?.cast_unsigned(),
        revision: row.get(5)?, dirty: row.get(6)?, assignment_id: row.get(7)?, comparison: RevisionComparison::Unknown,
    })).optional().map_err(storage_error)
}

fn load(connection: &Connection, task: u64, check: u64) -> Result<TaskCheck, RegistrationError> {
    let project = load_record(connection, task)?
        .project_slugs
        .into_iter()
        .next();
    let mut check = connection.query_row("SELECT id, task_id, name, description, position, version FROM task_checks WHERE task_id = ?1 AND id = ?2",
        params![task.cast_signed(), check.cast_signed()], |row| Ok(TaskCheck {
            id: row.get::<_, i64>(0)?.cast_unsigned(), task_id: row.get::<_, i64>(1)?.cast_unsigned(),
            name: row.get(2)?, description: row.get(3)?, position: row.get::<_, i64>(4)?.cast_unsigned(),
            version: row.get::<_, i64>(5)?.cast_unsigned(), latest_assessment: None,
            project, current_verdict: None,
        })).optional().map_err(storage_error)?.ok_or_else(|| RegistrationError::new("check-not-found", "Check does not belong to this Task"))?;
    check.latest_assessment = connection.query_row(&format!("SELECT {ASSESSMENT_COLUMNS} FROM check_assessments WHERE check_id = ?1 ORDER BY id DESC LIMIT 1"),
        [check.id.cast_signed()], assessment).optional().map_err(storage_error)?;
    if let Some(a) = &mut check.latest_assessment {
        a.invalidation = invalidation(connection, a.id)?;
    }
    check.current_verdict = check.verdict();
    Ok(check)
}

pub(super) fn gate(connection: &Connection, task: u64) -> Result<(), RegistrationError> {
    let mut statement = connection.prepare("SELECT c.id FROM task_checks c WHERE c.task_id = ?1 AND COALESCE((SELECT CASE WHEN EXISTS (SELECT 1 FROM check_invalidations i WHERE i.assessment_id = a.id) THEN 'pending' ELSE a.verdict END FROM check_assessments a WHERE a.check_id = c.id ORDER BY a.id DESC LIMIT 1), 'pending') != 'pass' ORDER BY c.id").map_err(storage_error)?;
    let blocked = statement
        .query_map([task.cast_signed()], |row| {
            row.get::<_, i64>(0).map(i64::cast_unsigned)
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    // Fresh stores provision every Task; missing criteria are a corrupt invariant, never a bypass.
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM task_checks WHERE task_id = ?1",
            [task.cast_signed()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if count == 0 {
        return Err(RegistrationError::new(
            "checks-not-passed",
            "Task has no provisioned Checks",
        ));
    }
    let findings = super::findings::blockers(connection, task)?;
    if !blocked.is_empty() || !findings.is_empty() {
        return Err(RegistrationError::new(
            if blocked.is_empty() {
                "completion-blocked"
            } else {
                "checks-not-passed"
            },
            format!(
                "pending or failed Check IDs: {blocked:?}; unresolved blocking Finding IDs: {findings:?}"
            ),
        ));
    }
    Ok(())
}

impl SqliteProjectRepository {
    pub(super) fn check_task(
        &self,
        connection: &Connection,
        task: u64,
    ) -> Result<Record, RegistrationError> {
        self.ensure_record_workspace(connection, task)?;
        let record = load_record(connection, task)?;
        if record.record_type != RecordType::Task {
            return Err(RegistrationError::new(
                "invalid-record-type",
                "Checks belong only to Tasks",
            ));
        }
        Ok(record)
    }
    pub(super) fn list_checks(&self, task: u64) -> Result<Vec<TaskCheck>, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        self.check_task(&transaction, task)?;
        let ids = transaction
            .prepare("SELECT id FROM task_checks WHERE task_id = ?1 ORDER BY position, id")
            .map_err(storage_error)?
            .query_map([task.cast_signed()], |row| {
                row.get::<_, i64>(0).map(i64::cast_unsigned)
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        ids.into_iter()
            .map(|id| load(&transaction, task, id))
            .collect()
    }
    pub(super) fn show_check(
        &self,
        task: u64,
        check: u64,
        after: Option<u64>,
        limit: u32,
    ) -> Result<CheckPage, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        self.check_task(&transaction, task)?;
        if !(1..=100).contains(&limit) || after.is_some_and(|id| id > i64::MAX.cast_unsigned()) {
            return Err(RegistrationError::new(
                "invalid-page",
                "limit must be between 1 and 100 and cursor must be a valid activity ID",
            ));
        }
        let check = load(&transaction, task, check)?;
        let mut activities = transaction.prepare("SELECT id, assessment_id, invalidation_id FROM check_activities WHERE check_id = ?1 AND id > ?2 ORDER BY id LIMIT ?3").map_err(storage_error)?
            .query_map(params![check.id.cast_signed(), after.unwrap_or(0).cast_signed(), limit + 1], |row| Ok((row.get::<_, i64>(0)?.cast_unsigned(), row.get::<_, i64>(1)?.cast_unsigned(), row.get::<_, Option<i64>>(2)?))).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
        let more = activities.len() > limit as usize;
        activities.truncate(limit as usize);
        let next_cursor = if more {
            activities.last().map(|a| a.0)
        } else {
            None
        };
        let mut assessments = Vec::new();
        let activities = activities
            .into_iter()
            .map(|(id, assessment_id, invalidation_id)| {
                let invalidation = if invalidation_id.is_some() {
                    invalidation(&transaction, assessment_id)?
                } else {
                    let mut a = transaction
                        .query_row(
                            &format!(
                                "SELECT {ASSESSMENT_COLUMNS} FROM check_assessments WHERE id = ?1"
                            ),
                            [assessment_id.cast_signed()],
                            assessment,
                        )
                        .map_err(storage_error)?;
                    a.invalidation = invalidation(&transaction, assessment_id)?;
                    assessments.push(a);
                    None
                };
                Ok(CheckActivity {
                    id,
                    assessment_id,
                    invalidation,
                })
            })
            .collect::<Result<Vec<_>, RegistrationError>>()?;
        Ok(CheckPage {
            check,
            assessments,
            next_cursor,
            activities,
        })
    }
    pub(super) fn invalidate_check(
        &self,
        task: u64,
        check: u64,
        input: &InvalidationInput,
        assignment: Option<&str>,
    ) -> Result<TaskCheck, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock is poisoned"))?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let record = self.check_task(&transaction, task)?;
        input.validate()?;
        let current = load(&transaction, task, check)?;
        if record.status == RecordStatus::Done {
            return Err(RegistrationError::new(
                "record-done",
                "Done Task Assessments are frozen",
            ));
        }
        if current.version != input.expected_version {
            return Err(RegistrationError::new(
                "stale-version",
                "Check version changed; read it again before invalidating",
            ));
        }
        if !current
            .latest_assessment
            .as_ref()
            .is_some_and(|a| a.id == input.assessment_id && a.invalidation.is_none())
        {
            return Err(RegistrationError::new(
                "invalid-transition",
                "only the latest, not-yet-invalidated Assessment may be invalidated",
            ));
        }
        transaction.execute("INSERT INTO check_invalidations (assessment_id, actor, message, created_at_millis, revision, dirty, assignment_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![input.assessment_id.cast_signed(), input.actor, input.message, now_millis()?, input.revision, input.dirty, assignment]).map_err(storage_error)?;
        let invalidation_id = transaction.last_insert_rowid();
        transaction.execute("INSERT INTO check_activities (check_id, assessment_id, invalidation_id) VALUES (?1, ?2, ?3)", params![check.cast_signed(), input.assessment_id.cast_signed(), invalidation_id]).map_err(storage_error)?;
        transaction
            .execute(
                "UPDATE task_checks SET version = version + 1 WHERE id = ?1",
                [check.cast_signed()],
            )
            .map_err(storage_error)?;
        insert_update(
            &transaction,
            task,
            RecordUpdateKind::CheckInvalidated {
                assessment_id: input.assessment_id,
            },
        )?;
        let updated = load(&transaction, task, check)?;
        transaction.commit().map_err(storage_error)?;
        Ok(updated)
    }
    pub(super) fn assess_check(
        &self,
        task: u64,
        check: u64,
        input: &AssessmentInput,
        assignment: Option<&str>,
    ) -> Result<TaskCheck, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock is poisoned"))?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let record = self.check_task(&transaction, task)?;
        input.validate()?;
        let current = load(&transaction, task, check)?;
        if record.status == RecordStatus::Done {
            return Err(RegistrationError::new(
                "record-done",
                "Done Task Assessments are frozen",
            ));
        }
        if current.version != input.expected_version {
            return Err(RegistrationError::new(
                "stale-version",
                "Check version changed; read it again before assessing",
            ));
        }
        transaction.execute("INSERT INTO check_assessments (check_id, verdict, actor, message, created_at_millis, revision, dirty, assignment_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![check.cast_signed(), match input.verdict { Verdict::Pass => "pass", Verdict::Fail => "fail" }, input.actor, input.message, now_millis()?, input.revision, input.dirty, assignment]).map_err(storage_error)?;
        let assessment_id = transaction.last_insert_rowid().cast_unsigned();
        transaction
            .execute(
                "INSERT INTO check_activities (check_id, assessment_id) VALUES (?1, ?2)",
                params![check.cast_signed(), assessment_id.cast_signed()],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "UPDATE task_checks SET version = version + 1 WHERE id = ?1",
                [check.cast_signed()],
            )
            .map_err(storage_error)?;
        insert_update(
            &transaction,
            task,
            RecordUpdateKind::CheckAssessed { assessment_id },
        )?;
        let updated = load(&transaction, task, check)?;
        transaction.commit().map_err(storage_error)?;
        Ok(updated)
    }
}
