use super::{SqliteProjectRepository, insert_update, now_millis, storage_error};
use forge_app::{
    RecordStatus, RecordUpdateKind, RegistrationError,
    checks::RevisionComparison,
    findings::{
        Finding, FindingAction, FindingActivity, FindingAmendment, FindingList, FindingMutation,
        FindingMutationInput, FindingPage, FindingRaiseInput, FindingResolveInput, FindingResponse,
        FindingState, Severity,
    },
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub(super) fn schema(c: &Connection) -> Result<(), RegistrationError> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS record_findings (
        id INTEGER PRIMARY KEY NOT NULL, record_id INTEGER NOT NULL REFERENCES records(id),
        severity TEXT NOT NULL CHECK(severity IN ('minor','major','critical')), blocking INTEGER NOT NULL,
        check_id INTEGER REFERENCES record_checks(id), assessment_id INTEGER REFERENCES check_assessments(id));
        CREATE INDEX IF NOT EXISTS findings_by_record ON record_findings(record_id, id);
        CREATE TABLE IF NOT EXISTS finding_activities (
        id INTEGER PRIMARY KEY NOT NULL, finding_id INTEGER NOT NULL REFERENCES record_findings(id),
        action TEXT NOT NULL CHECK(action IN ('raise','resolve','respond','dismiss','reopen','amend','note')), actor TEXT NOT NULL, assignment_id TEXT,
        response TEXT CHECK(response IN ('fixed','rejected')), amendment TEXT,
        message TEXT NOT NULL, revision TEXT, dirty INTEGER, created_at_millis INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS activities_by_finding ON finding_activities(finding_id, id);").map_err(storage_error)
}
fn page_args(after: Option<u64>, limit: u32) -> Result<(), RegistrationError> {
    if !(1..=100).contains(&limit) || after.is_some_and(|v| v > i64::MAX.cast_unsigned()) {
        return Err(RegistrationError::new(
            "invalid-page",
            "limit must be between 1 and 100 and cursor must be a valid ID",
        ));
    }
    Ok(())
}
const ACTIVITY_COLUMNS: &str = "id, finding_id, action, actor, assignment_id, message, revision, dirty, created_at_millis, response, amendment";
fn activity(row: &rusqlite::Row<'_>) -> rusqlite::Result<FindingActivity> {
    Ok(FindingActivity {
        id: row.get::<_, i64>(0)?.cast_unsigned(),
        finding_id: row.get::<_, i64>(1)?.cast_unsigned(),
        action: match row.get::<_, String>(2)?.as_str() {
            "raise" => FindingAction::Raise,
            "resolve" => FindingAction::Resolve,
            "respond" => FindingAction::Respond,
            "dismiss" => FindingAction::Dismiss,
            "reopen" => FindingAction::Reopen,
            "amend" => FindingAction::Amend,
            "note" => FindingAction::Note,
            _ => return Err(rusqlite::Error::InvalidQuery),
        },
        response: match row.get::<_, Option<String>>(9)?.as_deref() {
            Some("fixed") => Some(FindingResponse::Fixed),
            Some("rejected") => Some(FindingResponse::Rejected),
            None => None,
            _ => return Err(rusqlite::Error::InvalidQuery),
        },
        amendment: row
            .get::<_, Option<String>>(10)?
            .map(|s| serde_json::from_str(&s).map_err(|_| rusqlite::Error::InvalidQuery))
            .transpose()?,
        actor: row.get(3)?,
        assignment_id: row.get(4)?,
        message: row.get(5)?,
        revision: row.get(6)?,
        dirty: row.get(7)?,
        created_at_millis: row.get::<_, i64>(8)?.cast_unsigned(),
        comparison: RevisionComparison::Unknown,
    })
}
fn load(
    c: &Connection,
    task: u64,
    id: u64,
    project: Option<String>,
) -> Result<Finding, RegistrationError> {
    let (severity, blocking, check_id, assessment_id) = c.query_row("SELECT severity, blocking, check_id, assessment_id FROM record_findings WHERE record_id = ?1 AND id = ?2", params![task.cast_signed(), id.cast_signed()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?, r.get::<_, Option<i64>>(2)?, r.get::<_, Option<i64>>(3)?))).optional().map_err(storage_error)?.ok_or_else(|| RegistrationError::new("finding-not-found", "Finding does not belong to this Task"))?;
    let raised = c.query_row(&format!("SELECT {ACTIVITY_COLUMNS} FROM finding_activities WHERE finding_id = ?1 ORDER BY id LIMIT 1"), [id.cast_signed()], activity).map_err(storage_error)?;
    let latest_activity = c.query_row(&format!("SELECT {ACTIVITY_COLUMNS} FROM finding_activities WHERE finding_id = ?1 ORDER BY id DESC LIMIT 1"), [id.cast_signed()], activity).map_err(storage_error)?;
    let version = c
        .query_row(
            "SELECT count(*) FROM finding_activities WHERE finding_id = ?1",
            [id.cast_signed()],
            |r| r.get::<_, i64>(0),
        )
        .map_err(storage_error)?
        .cast_unsigned();
    Ok(Finding {
        id,
        record_id: task,
        version,
        severity: match severity.as_str() {
            "minor" => Severity::Minor,
            "major" => Severity::Major,
            "critical" => Severity::Critical,
            _ => {
                return Err(RegistrationError::new(
                    "storage",
                    "invalid Finding severity",
                ));
            }
        },
        blocking,
        check_id: check_id.map(i64::cast_unsigned),
        assessment_id: assessment_id.map(i64::cast_unsigned),
        // Notes and amendments retain the latest disposition, even on closed Findings.
        state: c.query_row("SELECT action FROM finding_activities WHERE finding_id = ?1 AND action NOT IN ('note','amend') ORDER BY id DESC LIMIT 1", [id.cast_signed()], |r| {
            match r.get::<_, String>(0)?.as_str() {
                "raise" | "reopen" => Ok(FindingState::Open),
                "respond" => Ok(FindingState::AwaitingReview),
                "resolve" => Ok(FindingState::Resolved),
                "dismiss" => Ok(FindingState::Dismissed),
                _ => Err(rusqlite::Error::InvalidQuery),
            }
        }).map_err(storage_error)?,
        raised,
        latest_activity,
        project,
    })
}
// Keep attribution/evidence explicit at the single transactional activity/log boundary.
#[allow(clippy::too_many_arguments)]
fn append(
    c: &Connection,
    task: u64,
    finding: u64,
    action: &str,
    actor: &str,
    message: &str,
    revision: Option<&str>,
    dirty: Option<bool>,
    assignment: Option<&str>,
) -> Result<u64, RegistrationError> {
    c.execute("INSERT INTO finding_activities (finding_id, action, actor, message, revision, dirty, created_at_millis, assignment_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", params![finding.cast_signed(), action, actor, message, revision, dirty, now_millis()?, assignment]).map_err(storage_error)?;
    let activity_id = c.last_insert_rowid().cast_unsigned();
    insert_update(c, task, RecordUpdateKind::FindingActivity { activity_id })?;
    Ok(activity_id)
}
pub(super) fn blockers(c: &Connection, task: u64) -> Result<Vec<u64>, RegistrationError> {
    c.prepare("SELECT f.id FROM record_findings f WHERE f.record_id = ?1 AND f.blocking = 1 AND (SELECT a.action FROM finding_activities a WHERE a.finding_id = f.id AND a.action NOT IN ('note','amend') ORDER BY a.id DESC LIMIT 1) NOT IN ('resolve','dismiss') ORDER BY f.id").map_err(storage_error)?.query_map([task.cast_signed()], |r| r.get::<_, i64>(0).map(i64::cast_unsigned)).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)
}
impl SqliteProjectRepository {
    pub(super) fn raise_finding(
        &self,
        task: u64,
        input: &FindingRaiseInput,
        assignment: Option<&str>,
    ) -> Result<Finding, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock is poisoned"))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let record = self.check_task(&tx, task)?;
        input.validate()?;
        if record.status == RecordStatus::Done {
            return Err(RegistrationError::new(
                "record-done",
                "cannot raise Findings on a Done Task",
            ));
        }
        let mut check = input.check_id;
        if let Some(assessment) = input.assessment_id {
            let origin = tx.query_row("SELECT c.id FROM check_assessments a JOIN record_checks c ON c.id = a.check_id WHERE a.id = ?1 AND c.record_id = ?2", params![assessment.cast_signed(), task.cast_signed()], |r| r.get::<_, i64>(0)).optional().map_err(storage_error)?.map(i64::cast_unsigned).ok_or_else(|| RegistrationError::new("invalid-finding-origin", "Assessment must belong to this Task"))?;
            if check.is_some_and(|id| id != origin) {
                return Err(RegistrationError::new(
                    "invalid-finding-origin",
                    "Assessment does not match the supplied Check",
                ));
            }
            check = Some(origin);
        }
        if let Some(check) = check {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM record_checks WHERE id = ?1 AND record_id = ?2)",
                    params![check.cast_signed(), task.cast_signed()],
                    |r| r.get(0),
                )
                .map_err(storage_error)?;
            if !exists {
                return Err(RegistrationError::new(
                    "invalid-finding-origin",
                    "Check must belong to this Task",
                ));
            }
        }
        let blocking = input.blocking.unwrap_or(input.severity != Severity::Minor);
        let severity = match input.severity {
            Severity::Minor => "minor",
            Severity::Major => "major",
            Severity::Critical => "critical",
        };
        tx.execute("INSERT INTO record_findings (record_id, severity, blocking, check_id, assessment_id) VALUES (?1, ?2, ?3, ?4, ?5)", params![task.cast_signed(), severity, blocking, check.map(u64::cast_signed), input.assessment_id.map(u64::cast_signed)]).map_err(storage_error)?;
        let id = tx.last_insert_rowid().cast_unsigned();
        append(
            &tx,
            task,
            id,
            "raise",
            &input.actor,
            &input.message,
            input.revision.as_deref(),
            input.dirty,
            assignment,
        )?;
        let finding = load(&tx, task, id, record.project_slugs.into_iter().next())?;
        tx.commit().map_err(storage_error)?;
        Ok(finding)
    }
    pub(super) fn resolve_finding(
        &self,
        task: u64,
        id: u64,
        input: &FindingResolveInput,
    ) -> Result<Finding, RegistrationError> {
        self.mutate_finding(
            task,
            id,
            &FindingMutationInput {
                change: FindingMutation::Resolve,
                explanation: input.clone(),
            },
            None,
        )
    }
    pub(super) fn mutate_finding(
        &self,
        task: u64,
        id: u64,
        mutation: &FindingMutationInput,
        assignment: Option<&str>,
    ) -> Result<Finding, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock is poisoned"))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let record = self.check_task(&tx, task)?;
        mutation.validate()?;
        let input = &mutation.explanation;
        let project = record.project_slugs.into_iter().next();
        let current = load(&tx, task, id, project.clone())?;
        if current.version != input.expected_version {
            return Err(RegistrationError::new(
                "stale-version",
                "Finding version changed; read it again",
            ));
        }
        let closed = matches!(
            current.state,
            FindingState::Resolved | FindingState::Dismissed
        );
        if record.status == RecordStatus::Done
            && (matches!(mutation.change, FindingMutation::Amend { .. })
                || (current.blocking && matches!(mutation.change, FindingMutation::Reopen)))
        {
            return Err(RegistrationError::new(
                "record-done",
                "cannot amend Findings or reopen blocking Findings on a Done Task",
            ));
        }
        let (action, response, amendment) = match mutation.change {
            FindingMutation::Respond { response } if !closed => ("respond", Some(response), None),
            FindingMutation::Resolve if !closed => ("resolve", None, None),
            FindingMutation::Dismiss if !closed => ("dismiss", None, None),
            FindingMutation::Reopen if closed => ("reopen", None, None),
            FindingMutation::Note => ("note", None, None),
            FindingMutation::Amend { severity, blocking } => {
                let new_severity = severity.unwrap_or(current.severity);
                let new_blocking = blocking.unwrap_or(current.blocking);
                let severity_text = match new_severity {
                    Severity::Minor => "minor",
                    Severity::Major => "major",
                    Severity::Critical => "critical",
                };
                tx.execute(
                    "UPDATE record_findings SET severity = ?1, blocking = ?2 WHERE id = ?3",
                    params![severity_text, new_blocking, id.cast_signed()],
                )
                .map_err(storage_error)?;
                (
                    "amend",
                    None,
                    Some(FindingAmendment {
                        old_severity: current.severity,
                        new_severity,
                        old_blocking: current.blocking,
                        new_blocking,
                    }),
                )
            }
            _ => {
                return Err(RegistrationError::new(
                    "invalid-transition",
                    "responses and dispositions require an unresolved Finding; reopening requires a closed Finding",
                ));
            }
        };
        let activity_id = append(
            &tx,
            task,
            id,
            action,
            &input.actor,
            &input.message,
            input.revision.as_deref(),
            input.dirty,
            assignment,
        )?;
        let response = response.map(|r| match r {
            FindingResponse::Fixed => "fixed",
            FindingResponse::Rejected => "rejected",
        });
        let amendment = amendment
            .map(|a| serde_json::to_string(&a))
            .transpose()
            .map_err(|e| RegistrationError::new("storage", e.to_string()))?;
        tx.execute(
            "UPDATE finding_activities SET response = ?1, amendment = ?2 WHERE id = ?3",
            params![response, amendment, activity_id.cast_signed()],
        )
        .map_err(storage_error)?;
        let finding = load(&tx, task, id, project)?;
        tx.commit().map_err(storage_error)?;
        Ok(finding)
    }
    pub(super) fn list_findings(
        &self,
        task: u64,
        after: Option<u64>,
        limit: u32,
    ) -> Result<FindingList, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock is poisoned"))?;
        let tx = connection.transaction().map_err(storage_error)?;
        let record = self.check_task(&tx, task)?;
        page_args(after, limit)?;
        let mut ids = tx
            .prepare(
                "SELECT id FROM record_findings WHERE record_id = ?1 AND id > ?2 ORDER BY id LIMIT ?3",
            )
            .map_err(storage_error)?
            .query_map(
                params![
                    task.cast_signed(),
                    after.unwrap_or(0).cast_signed(),
                    limit + 1
                ],
                |r| r.get::<_, i64>(0).map(i64::cast_unsigned),
            )
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        let more = ids.len() > limit as usize;
        ids.truncate(limit as usize);
        let next_cursor = if more { ids.last().copied() } else { None };
        let project = record.project_slugs.into_iter().next();
        let findings = ids
            .into_iter()
            .map(|id| load(&tx, task, id, project.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(FindingList {
            findings,
            next_cursor,
        })
    }
    pub(super) fn show_finding(
        &self,
        task: u64,
        id: u64,
        after: Option<u64>,
        limit: u32,
    ) -> Result<FindingPage, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock is poisoned"))?;
        let tx = connection.transaction().map_err(storage_error)?;
        let record = self.check_task(&tx, task)?;
        page_args(after, limit)?;
        let finding = load(&tx, task, id, record.project_slugs.into_iter().next())?;
        let mut activities = tx.prepare(&format!("SELECT {ACTIVITY_COLUMNS} FROM finding_activities WHERE finding_id = ?1 AND id > ?2 ORDER BY id LIMIT ?3")).map_err(storage_error)?.query_map(params![id.cast_signed(), after.unwrap_or(0).cast_signed(), limit + 1], activity).map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
        let more = activities.len() > limit as usize;
        activities.truncate(limit as usize);
        let next_cursor = if more {
            activities.last().map(|a| a.id)
        } else {
            None
        };
        Ok(FindingPage {
            finding,
            activities,
            next_cursor,
        })
    }
}
