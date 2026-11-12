//! Controlled Check claims. Assignment, attempt and capacity are reserved in one transaction.
use super::{
    SqliteProjectRepository, checks, execution_modes, load_agent_profile, load_record, now_millis,
    outcomes, storage_error, workspace_dispatch,
};
use forge_app::{
    AgentAssignment, AgentDispatchRepository, DispatchPreflightSelection, RecordStatus, RecordType,
    RegistrationError,
    checks::Verdict,
    outcomes::{Attempt, Evidence},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

fn error(code: &'static str, message: &str) -> RegistrationError {
    RegistrationError::new(code, message)
}

pub(super) fn binding(
    c: &Connection,
    assignment: &str,
) -> Result<Option<Attempt>, RegistrationError> {
    let json: Option<String> = c.query_row("SELECT a.attempt_json FROM assignment_check_attempts b JOIN record_attempts a ON a.id = b.attempt_id WHERE b.assignment_id = ?1", [assignment], |r| r.get(0)).optional().map_err(storage_error)?;
    json.map(|json| {
        serde_json::from_str(&json).map_err(|_| error("storage", "invalid Assignment attempt"))
    })
    .transpose()
}

impl SqliteProjectRepository {
    /// Reserve capacity before revocation; restart must retry cleanup, never infer retirement.
    pub(super) fn begin_check_handoff(&self, id: &str) -> Result<bool, RegistrationError> {
        let mut c = self
            .connection
            .lock()
            .map_err(|_| error("storage", "storage lock poisoned"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let Some(attempt) = binding(&tx, id)? else {
            return Ok(false);
        };
        self.ensure_record_workspace(&tx, attempt.record_id)?;
        let check = checks::load(&tx, attempt.record_id, attempt.check_id)?;
        if check.current_verdict != Some(Verdict::Pass)
            || !check.latest_assessment.as_ref().is_some_and(|a| {
                a.attempt_id == attempt.id && a.assignment_id.as_deref() == Some(id)
            })
        {
            return Ok(false);
        }
        let uncertain_launch: bool = tx.query_row(
            "SELECT launch_started_at_millis IS NOT NULL AND NOT EXISTS(SELECT 1 FROM agent_assignment_resources WHERE assignment_id = ?1 AND resource_kind = 'agent-name') FROM agent_assignments WHERE id = ?1",
            [id], |r| r.get(0)).map_err(storage_error)?;
        if uncertain_launch {
            return Ok(false);
        }
        // Only the first caller creates the blocker. Replays must not resurrect retired resources.
        tx.execute("INSERT OR IGNORE INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) SELECT id, 'check-handoff', 'acceptance', ?2 FROM agent_assignments WHERE id = ?1 AND closed_at_millis IS NULL", params![id, now_millis()?]).map_err(storage_error)?;
        tx.execute("UPDATE agent_assignments SET closed_at_millis = ?2, closure_reason = 'retired' WHERE id = ?1 AND closed_at_millis IS NULL", params![id, now_millis()?]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(true)
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub(super) fn claim_check_attempt(
        &self,
        record: u64,
        id: &str,
        selection: &DispatchPreflightSelection,
        evidence: &Evidence,
        expected_check: u64,
        expected_profile: u32,
    ) -> Result<(AgentAssignment, bool), RegistrationError> {
        if id.trim().is_empty() {
            return Err(error(
                "invalid-assignment-id",
                "Assignment identity must be nonblank",
            ));
        }
        evidence.validate()?;
        if !matches!(evidence, Evidence::Repository { .. }) {
            return Err(error(
                "evidence-mismatch",
                "dispatched Tasks require Managed Worktree Repository evidence",
            ));
        }
        let mut c = self
            .connection
            .lock()
            .map_err(|_| error("storage", "storage lock poisoned"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        self.ensure_record_workspace(&tx, record)?;
        let task = load_record(&tx, record)?;
        if task.record_type != RecordType::Task {
            return Err(error(
                "unsupported-record-type",
                "controlled Check execution supports only Tasks",
            ));
        }
        forge_app::validate_task_dispatch_mode(&task)?;
        let existing: Option<String> = tx.query_row("SELECT id FROM agent_assignments WHERE record_id = ?1 AND closed_at_millis IS NULL", [record.cast_signed()], |r| r.get(0)).optional().map_err(storage_error)?;
        if let Some(existing) = existing {
            if binding(&tx, &existing)?.is_none() {
                return Err(error(
                    "assignment-recovery-blocked",
                    "unbound Assignment owns this Task; reconcile ownership first",
                ));
            }
            drop(tx);
            drop(c);
            return Ok((self.agent_assignment(&existing)?, true));
        }
        if !matches!(task.status, RecordStatus::Open | RecordStatus::InProgress) {
            return Err(error(
                "record-not-ready",
                "Task must be Open or Forge-managed In Progress",
            ));
        }
        if task.status == RecordStatus::InProgress {
            let managed: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_assignments a JOIN assignment_check_attempts b ON b.assignment_id = a.id WHERE a.record_id = ?1)",
                [record.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
            if !managed {
                return Err(error(
                    "record-not-ready",
                    "In Progress continuation requires durable Check-bound Assignment history; lifecycle status is not ownership evidence",
                ));
            }
        }
        execution_modes::safe_ownership(&tx, record)?;
        let held: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM dispatch_holds WHERE record_id = ?1)",
                [record.cast_signed()],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        if held {
            return Err(error(
                "dispatch-held",
                "investigate the Dispatch Hold and explicitly resume",
            ));
        }
        if !super::unfinished_prerequisite_ids(&tx, record)?.is_empty() {
            return Err(error(
                "record-not-ready",
                "Task has unresolved prerequisites",
            ));
        }
        let worktree: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM worktree_claims WHERE record_id = ?1 AND state = 'active')", [record.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
        if !worktree {
            return Err(error(
                "managed-worktree-required",
                "prepare a valid Managed Worktree before controlled acceptance",
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
            return Err(error("attempt-active", "an attempt already owns this Task"));
        }
        let slug = &task.project_slugs[0];
        let used: i64 = tx.query_row("SELECT COUNT(*) FROM agent_assignments a JOIN record_projects p ON p.record_id = a.record_id WHERE p.project_slug = ?1 AND (a.closed_at_millis IS NULL OR EXISTS(SELECT 1 FROM agent_assignment_resources r WHERE r.assignment_id = a.id AND r.retired_at_millis IS NULL))", [slug], |r| r.get(0)).map_err(storage_error)?;
        if used >= i64::from(super::effective_limit(&tx, slug)?) {
            return Err(error(
                "project-capacity",
                "Project dispatch capacity is reserved",
            ));
        }
        workspace_dispatch::verify_preflight_selection(&tx, record, selection)?;
        let ids = tx
            .prepare("SELECT id FROM record_checks WHERE record_id = ?1 ORDER BY position")
            .map_err(storage_error)?
            .query_map([record.cast_signed()], |r| {
                r.get::<_, i64>(0).map(i64::cast_unsigned)
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        let mut next = None;
        for check in ids {
            let check = checks::load(&tx, record, check)?;
            if check.current_verdict != Some(Verdict::Pass) {
                next = Some(check);
                break;
            }
        }
        let check = next.ok_or_else(|| {
            error(
                "checks-passed",
                "all Checks currently pass; no attempt is authorized",
            )
        })?;
        let revision = check.profile_revision.ok_or_else(|| {
            error(
                "invalid-check-profile",
                "next Check has no pinned Agent Profile",
            )
        })?;
        if check.id != expected_check || revision != expected_profile {
            return Err(error(
                "check-selection-stale",
                "next Check changed after preflight; retry acceptance",
            ));
        }
        let profile = load_agent_profile(&tx, RecordType::Task, revision)
            .map_err(|e| RegistrationError::new("invalid-check-profile", e.message))?;
        if profile.launch_policy.is_none() {
            return Err(error(
                "invalid-check-profile",
                "pinned revision requires a registered launch policy",
            ));
        }
        let work = outcomes::state(&tx, record)?;
        let bound = if check.position == 0 {
            work.input.as_ref()
        } else {
            work.output.as_ref()
        };
        if bound.is_some_and(|bound| bound != evidence) || (check.position > 0 && bound.is_none()) {
            return Err(error(
                "evidence-mismatch",
                "attempt must bind current work input/output evidence",
            ));
        }
        let attempt = Attempt {
            id: id.into(),
            record_id: record,
            check_id: check.id,
            generation: work.generation,
            evidence: evidence.clone(),
            actor: id.into(),
            body: task.body,
        };
        let attempt_number: u32 = tx.query_row("SELECT COALESCE(MAX(attempt_number), 0) + 1 FROM agent_assignments WHERE record_id = ?1", [record.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
        tx.execute("INSERT INTO agent_assignments(id, record_id, project_slug, profile_record_type, profile_revision, attempt_number, claimed_at_millis, state_changed_at_millis, safe_dispatch_generation) VALUES (?1, ?2, ?3, 'task', ?4, ?5, ?6, ?6, 1)", params![id, record.cast_signed(), slug, revision, attempt_number, now_millis()?]).map_err(storage_error)?;
        tx.execute("INSERT INTO record_attempts(id, record_id, check_id, attempt_json) VALUES (?1, ?2, ?3, ?4)", params![id, record.cast_signed(), check.id.cast_signed(), serde_json::to_string(&attempt).map_err(|_| error("storage", "invalid attempt"))?]).map_err(storage_error)?;
        tx.execute(
            "INSERT INTO assignment_check_attempts VALUES (?1, ?1)",
            [id],
        )
        .map_err(storage_error)?;
        if check.position == 0 {
            tx.execute("INSERT INTO record_work(record_id, input_json) VALUES (?1, ?2) ON CONFLICT(record_id) DO UPDATE SET input_json = excluded.input_json", params![record.cast_signed(), serde_json::to_string(evidence).map_err(|_| error("storage", "invalid evidence"))?]).map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)?;
        drop(c);
        Ok((self.agent_assignment(id)?, false))
    }
}
