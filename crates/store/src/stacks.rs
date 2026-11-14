//! Transactional stack membership, preparation and publication gates.
use super::{SqliteProjectRepository, checks, insert_update, load_record, outcomes, storage_error};
use forge_app::{
    ExecutionMode, RecordStatus, RecordUpdateKind, RegistrationError,
    outcomes::Evidence,
    stacks::{Publication, StackSource, TaskStack, task_branch},
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

fn error(message: impl Into<String>) -> RegistrationError {
    RegistrationError::new("stack-blocked", message)
}
fn json_error(_: serde_json::Error) -> RegistrationError {
    error("invalid stored stack data")
}

pub(super) fn schema(c: &Connection) -> Result<(), RegistrationError> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_stacks (
        record_id INTEGER PRIMARY KEY REFERENCES records(id),
        predecessor INTEGER UNIQUE REFERENCES task_stacks(record_id),
        source_json TEXT NOT NULL,
        base_commit TEXT,
        publication_json TEXT,
        CHECK(predecessor IS NULL OR predecessor != record_id)
    );",
    )
    .map_err(storage_error)
}

type StoredStackRow = (Option<i64>, String, Option<String>, Option<String>);

pub(super) fn load(c: &Connection, record: u64) -> Result<Option<TaskStack>, RegistrationError> {
    let row: Option<StoredStackRow> = c.query_row(
        "SELECT predecessor, source_json, base_commit, publication_json FROM task_stacks WHERE record_id = ?1",
        [record.cast_signed()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).optional().map_err(storage_error)?;
    let Some((predecessor, source, base_commit, publication)) = row else {
        return Ok(None);
    };
    let mut source: StackSource = serde_json::from_str(&source).map_err(json_error)?;
    let mut visited = std::collections::HashSet::from([record]);
    let destination = loop {
        match source {
            StackSource::Root { destination } => break destination,
            StackSource::Predecessor { record_id } => {
                if !visited.insert(record_id) {
                    return Err(error("stack cycle"));
                }
                let json: String = c
                    .query_row(
                        "SELECT source_json FROM task_stacks WHERE record_id = ?1",
                        [record_id.cast_signed()],
                        |r| r.get(0),
                    )
                    .map_err(storage_error)?;
                source = serde_json::from_str(&json).map_err(json_error)?;
            }
        }
    };
    let base_branch = predecessor.map_or_else(
        || destination.branch.clone(),
        |id| task_branch(id.cast_unsigned()),
    );
    Ok(Some(TaskStack {
        record_id: record,
        predecessor: predecessor.map(i64::cast_unsigned),
        destination,
        base_branch,
        base_commit,
        publication: publication
            .map(|p| serde_json::from_str(&p).map_err(json_error))
            .transpose()?,
    }))
}

fn audit(tx: &Transaction<'_>, record: u64, message: &str) -> Result<(), RegistrationError> {
    tx.execute("INSERT INTO record_comments(record_id, author, body, created_at_millis) VALUES (?1, 'forge-stack', ?2, ?3)", params![record.cast_signed(), message, super::now_millis()?]).map_err(storage_error)?;
    insert_update(
        tx,
        record,
        RecordUpdateKind::CommentAdded {
            comment_id: forge_app::RecordCommentId(tx.last_insert_rowid().cast_unsigned()),
        },
    )
}

fn unprepared(c: &Connection, record: u64) -> Result<(), RegistrationError> {
    let affected: bool = c.query_row("WITH RECURSIVE suffix(id) AS (SELECT ?1 UNION SELECT s.record_id FROM task_stacks s JOIN suffix ON s.predecessor = suffix.id)
        SELECT EXISTS(SELECT 1 FROM suffix JOIN records r ON r.id = suffix.id
        WHERE r.status != 'open' OR EXISTS(SELECT 1 FROM task_stacks s WHERE s.record_id = r.id AND s.base_commit IS NOT NULL)
        OR EXISTS(SELECT 1 FROM worktree_claims w WHERE w.record_id = r.id)
        OR EXISTS(SELECT 1 FROM record_attempts a WHERE a.record_id = r.id)
        OR EXISTS(SELECT 1 FROM agent_assignments a WHERE a.record_id = r.id))", [record.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
    if affected {
        return Err(error("all affected Tasks must be Open and unprepared"));
    }
    Ok(())
}

fn compatible(c: &Connection, record: u64) -> Result<(), RegistrationError> {
    let task = load_record(c, record)?;
    let last: Option<i64> = c
        .query_row(
            "SELECT id FROM record_checks WHERE record_id = ?1 ORDER BY position DESC LIMIT 1",
            [record.cast_signed()],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let last = last
        .map(|id| checks::load(c, record, id.cast_unsigned()))
        .transpose()?;
    forge_app::stacks::validate_stack_member(&task, last.as_ref())?;
    let profile = last.and_then(|c| c.profile_revision);
    if task.execution_mode == ExecutionMode::Dispatched {
        let revision =
            profile.ok_or_else(|| error("Publication Check requires a pinned profile"))?;
        let profile = super::load_agent_profile(c, forge_app::RecordType::Task, revision)?;
        if profile.launch_policy.is_none() {
            return Err(error("Publication profile has no registered launch policy"));
        }
    }
    Ok(())
}

fn required(c: &Connection, record: u64) -> Result<TaskStack, RegistrationError> {
    load(c, record)?.ok_or_else(|| error("Task is not stacked"))
}

impl SqliteProjectRepository {
    pub(super) fn stack_show(&self, record: u64) -> Result<Option<TaskStack>, RegistrationError> {
        let c = self
            .connection
            .lock()
            .map_err(|_| error("storage lock poisoned"))?;
        self.ensure_record_workspace(&c, record)?;
        load(&c, record)
    }

    pub(super) fn stack_configure(
        &self,
        record: u64,
        source: &StackSource,
    ) -> Result<TaskStack, RegistrationError> {
        if let StackSource::Root { destination } = source {
            destination.validate()?;
        }
        let mut c = self
            .connection
            .lock()
            .map_err(|_| error("storage lock poisoned"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let task = self.check_task(&tx, record)?;
        unprepared(&tx, record)?;
        compatible(&tx, record)?;
        let predecessor = match source {
            StackSource::Root { .. } => None,
            StackSource::Predecessor { record_id } => {
                self.ensure_record_workspace(&tx, *record_id)?;
                let previous = load_record(&tx, *record_id)?;
                forge_app::stacks::validate_stack_predecessor(&task, &previous)?;
                required(&tx, *record_id)?;
                let cycle: bool = tx.query_row("WITH RECURSIVE suffix(id) AS (SELECT ?1 UNION SELECT record_id FROM task_stacks JOIN suffix ON predecessor = suffix.id) SELECT EXISTS(SELECT 1 FROM suffix WHERE id = ?2)", params![record.cast_signed(), record_id.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
                if cycle {
                    return Err(error("stack cannot form a cycle"));
                }
                let occupied: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM task_stacks WHERE predecessor = ?1 AND record_id != ?2)", params![record_id.cast_signed(), record.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
                if occupied {
                    return Err(error("predecessor already has a Stack Successor"));
                }
                let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM record_relationships WHERE from_id = ?1 AND to_id = ?2 AND kind = 'depends-on')", params![record.cast_signed(), record_id.cast_signed()], |r| r.get(0)).map_err(storage_error)?;
                if !exists {
                    super::insert_relationship(
                        &tx,
                        &task,
                        &previous,
                        forge_app::RecordRelationshipKind::DependsOn,
                    )?;
                    for (id, other) in [(record, *record_id), (*record_id, record)] {
                        insert_update(
                            &tx,
                            id,
                            RecordUpdateKind::RelationshipAdded {
                                related_record_id: other,
                                relationship_kind: Some(
                                    forge_app::RecordRelationshipKind::DependsOn,
                                ),
                            },
                        )?;
                    }
                }
                Some(record_id.cast_signed())
            }
        };
        tx.execute("INSERT INTO task_stacks(record_id, predecessor, source_json) VALUES (?1, ?2, ?3) ON CONFLICT(record_id) DO UPDATE SET predecessor = excluded.predecessor, source_json = excluded.source_json", params![record.cast_signed(), predecessor, serde_json::to_string(source).map_err(json_error)?]).map_err(storage_error)?;
        // Stacked Tasks resolve at preparation, never from opening-time Project HEAD.
        tx.execute(
            "DELETE FROM task_base_commits WHERE record_id = ?1",
            [record.cast_signed()],
        )
        .map_err(storage_error)?;
        audit(
            &tx,
            record,
            &format!(
                "Stack configured: {}",
                serde_json::to_string(source).map_err(json_error)?
            ),
        )?;
        let result = required(&tx, record)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }

    pub(super) fn stack_pin(
        &self,
        expected: &TaskStack,
        commit: &str,
    ) -> Result<TaskStack, RegistrationError> {
        if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(error("expected full Base Commit"));
        }
        let mut c = self
            .connection
            .lock()
            .map_err(|_| error("storage lock poisoned"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        self.ensure_record_workspace(&tx, expected.record_id)?;
        if &required(&tx, expected.record_id)? != expected {
            return Err(error("stack changed during preparation; reread"));
        }
        unprepared(&tx, expected.record_id)?;
        compatible(&tx, expected.record_id)?;
        if !super::unfinished_prerequisite_ids(&tx, expected.record_id)?.is_empty() {
            return Err(error("Task has unresolved prerequisites"));
        }
        if let Some(predecessor) = expected.predecessor {
            let previous = required(&tx, predecessor)?;
            if previous.publication.is_none_or(|p| p.head_commit != commit) {
                return Err(error("Base Commit differs from predecessor publication"));
            }
        }
        tx.execute(
            "UPDATE task_stacks SET base_commit = ?2 WHERE record_id = ?1",
            params![expected.record_id.cast_signed(), commit],
        )
        .map_err(storage_error)?;
        tx.execute("INSERT INTO task_base_commits(record_id, base_commit) VALUES (?1, ?2) ON CONFLICT(record_id) DO UPDATE SET base_commit = excluded.base_commit", params![expected.record_id.cast_signed(), commit]).map_err(storage_error)?;
        audit(
            &tx,
            expected.record_id,
            &format!("Stack Base Commit pinned: {commit}"),
        )?;
        let result = required(&tx, expected.record_id)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }

    pub(super) fn stack_publish(
        &self,
        expected: &TaskStack,
        publication: &Publication,
        assignment: Option<&str>,
    ) -> Result<TaskStack, RegistrationError> {
        let mut c = self
            .connection
            .lock()
            .map_err(|_| error("storage lock poisoned"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let record = expected.record_id;
        let task = self.check_task(&tx, record)?;
        if task.status != RecordStatus::InProgress || expected.base_commit.is_none() {
            return Err(error("publication requires a prepared In Progress Task"));
        }
        if &required(&tx, record)? != expected {
            return Err(error("stack changed during publication; reread"));
        }
        let work = outcomes::state(&tx, record)?;
        let Some(Evidence::Repository { commit, .. }) = work.output.as_ref() else {
            return Err(error("repository output missing"));
        };
        if work.generation != publication.generation
            || commit != &publication.head_commit
            || publication.repository != expected.destination.repository
            || publication.head_branch != task_branch(record)
            || publication.base_branch != expected.base_branch
        {
            return Err(error("publication evidence changed"));
        }
        let last: i64 = tx
            .query_row(
                "SELECT id FROM record_checks WHERE record_id = ?1 ORDER BY position DESC LIMIT 1",
                [record.cast_signed()],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        let pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM record_checks WHERE record_id = ?1 AND id != ?2 AND COALESCE((SELECT CASE WHEN EXISTS(SELECT 1 FROM check_invalidations i WHERE i.assessment_id = a.id) THEN 'pending' ELSE a.verdict END FROM check_assessments a WHERE a.check_id = record_checks.id ORDER BY a.id DESC LIMIT 1), 'pending') != 'pass')", params![record.cast_signed(), last], |r| r.get(0)).map_err(storage_error)?;
        if pending {
            return Err(error("all preceding Checks must pass before publication"));
        }
        if let Some(id) = assignment {
            let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_assignments a JOIN assignment_check_attempts b ON b.assignment_id = a.id JOIN record_attempts t ON t.id = b.attempt_id WHERE a.id = ?1 AND a.record_id = ?2 AND a.closed_at_millis IS NULL AND t.check_id = ?3 AND t.closed = 0)", params![id, record.cast_signed(), last], |r| r.get(0)).map_err(storage_error)?;
            let bound = super::task_attempts::binding(&tx, id)?;
            let current = bound.is_some_and(|attempt| {
                attempt.generation == work.generation
                    && work.output.as_ref() == Some(&attempt.evidence)
            });
            if !valid || !current {
                return Err(RegistrationError::new(
                    "assignment-capability-denied",
                    "publication requires the bound final Check attempt",
                ));
            }
        } else {
            if task.execution_mode != ExecutionMode::Manual {
                return Err(RegistrationError::new(
                    "dispatched-publication-forbidden",
                    "operator publication requires explicit Manual Execution Mode",
                ));
            }
            super::execution_modes::safe_ownership(&tx, record)?;
        }
        tx.execute(
            "UPDATE task_stacks SET publication_json = ?2 WHERE record_id = ?1",
            params![
                record.cast_signed(),
                serde_json::to_string(publication).map_err(json_error)?
            ],
        )
        .map_err(storage_error)?;
        audit(
            &tx,
            record,
            &format!(
                "Verified non-draft publication: {} at {} (generation {})",
                publication.url, publication.head_commit, publication.generation
            ),
        )?;
        let result = required(&tx, record)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }
}

pub(super) fn publication_gate(
    c: &Connection,
    record: u64,
    evidence: &Evidence,
    generation: u64,
) -> Result<(), RegistrationError> {
    let Some(stack) = load(c, record)? else {
        return Ok(());
    };
    let Some(p) = stack.publication else {
        return Err(error("verified non-draft publication required"));
    };
    if p.generation != generation
        || !matches!(evidence, Evidence::Repository { commit, .. } if commit == &p.head_commit)
    {
        return Err(error("publication evidence is stale"));
    }
    Ok(())
}
