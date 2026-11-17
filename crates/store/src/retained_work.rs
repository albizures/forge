//! Atomic recovery authority; external Git observations belong to the application boundary.
use super::{SqliteProjectRepository, checks, execution_modes, outcomes, storage_error};
use forge_app::{
    RecordStatus, RegistrationError, WorktreeClaim, WorktreeClaimState,
    checks::Verdict,
    outcomes::{OutcomeDisposition, OutcomeInput},
    retained_work::{RecoveryInput, RetainedWorkRecovery},
};
use rusqlite::{Connection, TransactionBehavior, params};

fn error(code: &'static str, message: &str) -> RegistrationError {
    RegistrationError::new(code, message)
}
pub(super) fn state(
    c: &Connection,
    record: u64,
) -> Result<Vec<RetainedWorkRecovery>, RegistrationError> {
    // Offline upgrade inspection evaluates historical work without inventing recovery events.
    if c.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
        .map_err(storage_error)?
        < 17
    {
        return Ok(Vec::new());
    }
    c.prepare(
        "SELECT recovery_json FROM retained_work_recoveries WHERE record_id = ?1 ORDER BY rowid",
    )
    .map_err(storage_error)?
    .query_map([record.cast_signed()], |r| r.get::<_, String>(0))
    .map_err(storage_error)?
    .map(|r| {
        serde_json::from_str(&r.map_err(storage_error)?)
            .map_err(|_| error("storage", "invalid retained-work recovery"))
    })
    .collect()
}

impl SqliteProjectRepository {
    #[allow(clippy::too_many_lines)]
    pub(super) fn recover_retained_work(
        &self,
        record: u64,
        input: &RecoveryInput,
        claim: &WorktreeClaim,
        retained_state: Option<&forge_app::retained_work::RetainedState>,
    ) -> Result<RetainedWorkRecovery, RegistrationError> {
        input.validate()?;
        if retained_state.is_some_and(|s| {
            s.sha256.len() != 64 || !s.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        }) {
            return Err(error(
                "invalid-recovery",
                "Retained state requires a full SHA-256 digest",
            ));
        }
        let mut c = self
            .connection
            .lock()
            .map_err(|_| error("storage", "storage lock poisoned"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let task = self.check_task(&tx, record)?;
        if let Some(previous) = state(&tx, record)?
            .into_iter()
            .find(|r| r.request.source_assignment == input.source_assignment)
        {
            if previous.request == *input {
                return Ok(previous);
            }
            return Err(error(
                "recovery-conflict",
                "Source Assignment already has a different recovery authorization",
            ));
        }
        forge_app::validate_task_dispatch_mode(&task)?;
        if task.status != RecordStatus::InProgress || task.project_slugs.len() != 1 {
            return Err(error(
                "record-not-ready",
                "Recovery requires an In Progress single-Project Task",
            ));
        }
        execution_modes::safe_ownership(&tx, record)?;
        let work = outcomes::state(&tx, record)?;
        if work.generation != input.generation {
            return Err(error(
                "stale-generation",
                "Work freshness changed; inspect before authorizing salvage",
            ));
        }
        let source: String = tx.query_row("SELECT id FROM agent_assignments WHERE record_id = ?1 ORDER BY attempt_number DESC LIMIT 1", [record.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
        if source != input.source_assignment {
            return Err(error(
                "recovery-source-stale",
                "Recovery must name this Task's latest Check-bound Assignment",
            ));
        }
        let attempt = super::task_attempts::binding(&tx, &source)?.ok_or_else(|| {
            error(
                "recovery-unbound",
                "Source Assignment has no bound Check attempt",
            )
        })?;
        let retired: bool = tx.query_row("SELECT closed_at_millis IS NOT NULL AND closure_reason = 'retired' AND EXISTS(SELECT 1 FROM agent_assignment_resources WHERE assignment_id = ?1 AND resource_kind IN ('retirement-pending','check-handoff') AND retired_at_millis IS NOT NULL) FROM agent_assignments WHERE id = ?1", [&source], |r| r.get(0)).map_err(storage_error)?;
        if !retired {
            return Err(error(
                "assignment-recovery-blocked",
                "Explicit verified retirement is required before salvage authorization",
            ));
        }
        let check = checks::load(&tx, record, attempt.check_id)?;
        if check.position != 0
            || check.current_verdict == Some(Verdict::Pass)
            || attempt.record_id != record
        {
            return Err(error(
                "recovery-check-invalid",
                "Recovery is only for the same unpassed Implementation Check",
            ));
        }
        let accepted: Option<String> = tx
            .query_row(
                "SELECT outcome_json FROM record_attempts WHERE id = ?1",
                [&attempt.id],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        if let Some(json) = accepted {
            let outcome: OutcomeInput = serde_json::from_str(&json)
                .map_err(|_| error("storage", "invalid source outcome"))?;
            if outcome.disposition != OutcomeDisposition::Interrupted {
                return Err(error(
                    "recovery-outcome-invalid",
                    "Source has an accepted non-interruption outcome; recovery cannot replace accepted history",
                ));
            }
        }
        let project_root: String = tx
            .query_row(
                "SELECT registered_folder FROM projects WHERE slug = ?1",
                [&task.project_slugs[0]],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        let expected_path = super::workspace_worktree_path(&tx, record)?;
        let persisted: (String, String, String) = tx
            .query_row(
                "SELECT project_slug, path, state FROM worktree_claims WHERE record_id = ?1",
                [record.cast_signed()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(storage_error)?;
        if claim.record_id != record
            || claim.state != WorktreeClaimState::Active
            || claim.project_slug != task.project_slugs[0]
            || claim.path != expected_path
            || persisted
                != (
                    claim.project_slug.clone(),
                    claim.path.display().to_string(),
                    "active".into(),
                )
            || !matches!(&input.evidence, forge_app::outcomes::Evidence::Repository { repository, .. } if repository == &project_root)
        {
            return Err(error(
                "worktree-recovery-invalid",
                "Repository, canonical Task placement or active claim changed",
            ));
        }
        // Invalidate current passes without erasing Assessments/Findings or changing any budget.
        outcomes::invalidate(&tx, record, &input.actor, &input.message, None)?;
        tx.execute(
            "UPDATE record_work SET input_json = ?2, output_json = NULL WHERE record_id = ?1",
            params![
                record.cast_signed(),
                serde_json::to_string(&input.evidence)
                    .map_err(|_| error("storage", "invalid recovery evidence"))?
            ],
        )
        .map_err(storage_error)?;
        let recovery = RetainedWorkRecovery {
            record_id: record,
            check_id: check.id,
            request: input.clone(),
            worktree_path: claim.path.clone(),
            generation: work.generation + 1,
            created_at_millis: super::now_millis()?.cast_unsigned(),
            retained_state: retained_state.cloned(),
        };
        tx.execute(
            "INSERT INTO retained_work_recoveries VALUES (?1, ?2, ?3)",
            params![
                source,
                record.cast_signed(),
                serde_json::to_string(&recovery)
                    .map_err(|_| error("storage", "invalid recovery audit"))?
            ],
        )
        .map_err(storage_error)?;
        tx.execute("INSERT OR IGNORE INTO dispatch_holds(record_id, reason) VALUES (?1, 'Operator authorized retained work for inspection/salvage; verify safety and explicitly resume')", [record.cast_signed()]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(recovery)
    }
}
