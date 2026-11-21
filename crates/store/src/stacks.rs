//! Transactional stack membership, preparation and publication gates.
use super::{SqliteProjectRepository, checks, insert_update, load_record, outcomes, storage_error};
use forge_app::{
    ExecutionMode, RecordStatus, RecordUpdateKind, RegistrationError,
    outcomes::Evidence,
    stacks::{Publication, StackSource, TaskStack, UnchangedResult, task_branch},
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
    .map_err(storage_error)?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_unchanged_results (
    record_id INTEGER PRIMARY KEY REFERENCES task_stacks(record_id),
    evidence_json TEXT NOT NULL
);",
    )
    .map_err(storage_error)
}

type StoredStackRow = (Option<i64>, String, Option<String>, Option<String>);

/// Canonical inheritance is a prerequisite designation, never a mutable Stack link.
pub(super) fn builds_on(c: &Connection, record: u64) -> Result<Option<u64>, RegistrationError> {
    let exists: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'builds_on_dependencies')",
            [],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if !exists {
        return Ok(None);
    }
    c.query_row(
        "SELECT to_id FROM builds_on_dependencies WHERE from_id = ?1",
        [record.cast_signed()],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .map(|v| v.map(i64::cast_unsigned))
    .map_err(storage_error)
}

/// Immediate topology can be inspected even while the root is not configured.
pub(super) fn predecessor(c: &Connection, record: u64) -> Result<Option<u64>, RegistrationError> {
    if let Some(previous) = builds_on(c, record)? {
        return Ok(Some(previous));
    }
    let legacy = c
        .query_row(
            "SELECT predecessor FROM task_stacks WHERE record_id = ?1",
            [record.cast_signed()],
            |r| r.get::<_, Option<i64>>(0),
        )
        .optional()
        .map_err(storage_error)?
        .flatten();
    validate_historical_topology(c, legacy)?;
    Ok(legacy.map(i64::cast_unsigned))
}

// Historical topology is readable only while inspecting an explicit offline
// upgrade source. Current Revision State must use canonical designations.
fn validate_historical_topology(
    c: &Connection,
    predecessor: Option<i64>,
) -> Result<(), RegistrationError> {
    if predecessor.is_some()
        && c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .map_err(storage_error)?
            >= 13
    {
        return Err(error(
            "legacy Stack topology requires explicit validated state upgrade; declare new plans with record relate --kind builds-on",
        ));
    }
    Ok(())
}

pub(super) fn editable_topology(c: &Connection, record: u64) -> Result<(), RegistrationError> {
    super::spec_plans::editable_stack_suffix(c, record)?;
    unprepared(c, record)
}

#[derive(Clone, Copy)]
enum ObservedResult<'a> {
    Published(&'a Publication),
    Unchanged(&'a UnchangedResult),
}

pub(super) fn load(c: &Connection, record: u64) -> Result<Option<TaskStack>, RegistrationError> {
    let mut lineage = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut cursor = Some(record);
    while let Some(id) = cursor {
        if !visited.insert(id) {
            return Err(error("stack cycle"));
        }
        let predecessor: Option<Option<i64>> = c
            .query_row(
                "SELECT predecessor FROM task_stacks WHERE record_id = ?1",
                [id.cast_signed()],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let Some(predecessor) = predecessor else {
            let canonical: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'builds_on_dependencies')",
                [], |r| r.get(0),
            ).map_err(storage_error)?;
            let participates = canonical && c.query_row(
                "SELECT EXISTS(SELECT 1 FROM builds_on_dependencies WHERE from_id = ?1 OR to_id = ?1)",
                [id.cast_signed()], |r| r.get::<_, bool>(0),
            ).map_err(storage_error)?;
            return if id == record && !participates {
                Ok(None)
            } else {
                Err(error(format!(
                    "Stack root Task {id} has no root execution configuration; configure its repository, remote and starting branch"
                )))
            };
        };
        validate_historical_topology(c, predecessor)?;
        lineage.push(id);
        cursor = builds_on(c, id)?.or_else(|| predecessor.map(i64::cast_unsigned));
    }
    // Resolve from the root iteratively: long linear plans must not consume call stack.
    let mut previous = None;
    for id in lineage.into_iter().rev() {
        previous = load_member(c, id, previous.as_ref())?;
    }
    Ok(previous)
}

fn load_member(
    c: &Connection,
    record: u64,
    previous: Option<&TaskStack>,
) -> Result<Option<TaskStack>, RegistrationError> {
    let row: Option<StoredStackRow> = c.query_row(
        "SELECT predecessor, source_json, base_commit, publication_json FROM task_stacks WHERE record_id = ?1",
        [record.cast_signed()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).optional().map_err(storage_error)?;
    let Some((predecessor, source, base_commit, publication)) = row else {
        return Ok(None);
    };
    let canonical = builds_on(c, record)?;
    if canonical.is_some() && (predecessor.is_some() || source != "{}") {
        return Err(error(
            "canonical Builds-On successor has competing stored starting-code configuration",
        ));
    }
    let predecessor = canonical.map(u64::cast_signed).or(predecessor);
    let source: StackSource = if let Some(record_id) = canonical {
        StackSource::Predecessor { record_id }
    } else {
        serde_json::from_str(&source).map_err(json_error)?
    };
    let (destination, base_branch) = match source {
        StackSource::Root { destination } if predecessor.is_none() => {
            let branch = destination.branch.clone();
            (destination, branch)
        }
        StackSource::Predecessor { record_id } if predecessor == Some(record_id.cast_signed()) => {
            let previous = previous
                .filter(|p| p.record_id == record_id)
                .ok_or_else(|| error("predecessor is not stacked"))?;
            let branch = if previous.unchanged_result.is_some() {
                previous.base_branch.clone()
            } else {
                task_branch(record_id)
            };
            (previous.destination.clone(), branch)
        }
        _ => return Err(error("contradictory stored Stack Predecessor")),
    };
    // Older schemas are inspected before their explicit copy-and-upgrade.
    let has_unchanged: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'task_unchanged_results')",
            [],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    let unchanged: Option<String> = if has_unchanged {
        c.query_row(
            "SELECT evidence_json FROM task_unchanged_results WHERE record_id = ?1",
            [record.cast_signed()],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage_error)?
    } else {
        None
    };
    if unchanged.is_some() && publication.is_some() {
        return Err(error("conflicting stored publication alternatives"));
    }
    Ok(Some(TaskStack {
        record_id: record,
        predecessor: predecessor.map(i64::cast_unsigned),
        destination,
        base_branch,
        base_commit,
        unchanged_result: unchanged
            .map(|s| serde_json::from_str(&s).map_err(json_error))
            .transpose()?,
        publication: publication
            .map(|p| serde_json::from_str(&p).map_err(json_error))
            .transpose()?,
    }))
}

fn predecessor_gate(
    c: &Connection,
    stack: &TaskStack,
    commit: &str,
) -> Result<(), RegistrationError> {
    let mut cursor = stack.predecessor;
    while let Some(id) = cursor {
        if load_record(c, id)?.status != RecordStatus::Done {
            return Err(error(format!("Stack Predecessor {id} is not Done")));
        }
        let previous = required(c, id)?;
        let work = outcomes::state(c, id)?;
        if previous.verified_commit(&work)? != commit {
            return Err(error(format!(
                "Base Commit differs from Stack Predecessor {id} verified result"
            )));
        }
        let branch = if previous.unchanged_result.is_some() {
            previous.base_branch.clone()
        } else {
            task_branch(id)
        };
        if stack.base_branch != branch {
            return Err(error("Base Branch differs from predecessor lineage"));
        }
        cursor = if previous.unchanged_result.is_some() {
            previous.predecessor
        } else {
            None
        };
    }
    Ok(())
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
    let affected: bool = c.query_row("WITH RECURSIVE links(record_id, predecessor) AS (SELECT record_id, predecessor FROM task_stacks UNION SELECT from_id, to_id FROM builds_on_dependencies), suffix(id) AS (SELECT ?1 UNION SELECT s.record_id FROM links s JOIN suffix ON s.predecessor = suffix.id)
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

pub(super) fn compatible(c: &Connection, record: u64) -> Result<(), RegistrationError> {
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
    if task.execution_mode == ExecutionMode::Dispatched {
        let mut statement = c.prepare(
            "SELECT c.id, c.name, COALESCE((SELECT m.new_revision FROM check_profile_migration_changes m WHERE m.check_id = c.id ORDER BY m.migration_id DESC LIMIT 1), c.profile_revision) FROM record_checks c WHERE c.record_id = ?1 ORDER BY c.position",
        ).map_err(storage_error)?;
        let checks = statement
            .query_map([record.cast_signed()], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<u32>>(2)?,
                ))
            })
            .map_err(storage_error)?;
        for check in checks {
            let (id, name, revision) = check.map_err(storage_error)?;
            let revision = revision.ok_or_else(|| {
                error(format!(
                    "Task {record}, Check {id} ({name}) requires a pinned profile"
                ))
            })?;
            let profile = super::load_agent_profile(c, forge_app::RecordType::Task, revision)
                .map_err(|failure| {
                    error(format!(
                        "Task {record}, Check {id} ({name}): {}",
                        failure.message
                    ))
                })?;
            if profile.launch_policy.is_none() {
                return Err(error(format!(
                    "Task {record}, Check {id} ({name}) profile has no registered launch policy"
                )));
            }
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
        forge_app::stacks::validate_stack_task(&task)?;
        super::spec_plans::editable_stack_suffix(&tx, record)?;
        unprepared(&tx, record)?;
        if builds_on(&tx, record)?.is_some() {
            return Err(error(
                "a Builds-On successor inherits its starting code; remove the Relationship before configuring a root",
            ));
        }
        if matches!(source, StackSource::Predecessor { .. }) {
            return Err(error(
                "Stack predecessor configuration is no longer supported; use record relate --kind builds-on --to <predecessor>",
            ));
        }
        tx.execute("INSERT INTO task_stacks(record_id, predecessor, source_json) VALUES (?1, NULL, ?2) ON CONFLICT(record_id) DO UPDATE SET predecessor = NULL, source_json = excluded.source_json", params![record.cast_signed(), serde_json::to_string(source).map_err(json_error)?]).map_err(storage_error)?;
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
        predecessor_gate(&tx, expected, commit)?;
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
        self.stack_accept_result(expected, ObservedResult::Published(publication), assignment)
    }

    pub(super) fn stack_unchanged(
        &self,
        expected: &TaskStack,
        result: &UnchangedResult,
        assignment: Option<&str>,
    ) -> Result<TaskStack, RegistrationError> {
        self.stack_accept_result(expected, ObservedResult::Unchanged(result), assignment)
    }

    // Keep authorization, freshness, evidence and required audit in one writer transaction.
    #[allow(clippy::too_many_lines)]
    fn stack_accept_result(
        &self,
        expected: &TaskStack,
        observed: ObservedResult<'_>,
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
        predecessor_gate(
            &tx,
            expected,
            expected.base_commit.as_deref().unwrap_or_default(),
        )?;
        let work = outcomes::state(&tx, record)?;
        let Some(Evidence::Repository { commit, .. }) = work.output.as_ref() else {
            return Err(error("repository output missing"));
        };
        match observed {
            ObservedResult::Published(p)
                if work.generation == p.generation
                    && commit == &p.head_commit
                    && p.repository == expected.destination.repository
                    && p.head_branch == task_branch(record)
                    && p.base_branch == expected.base_branch => {}
            ObservedResult::Unchanged(u)
                if work.generation == u.generation
                    && work.output.as_ref() == Some(&u.evidence)
                    && expected.base_commit.as_ref() == Some(commit) => {}
            _ => return Err(error("publication evidence changed")),
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
        // Exact retries do not add history; conflicting evidence requires explicit invalidation.
        if let Some(existing) = &expected.unchanged_result
            && existing.generation == work.generation
        {
            if !matches!(observed, ObservedResult::Unchanged(u) if u == existing) {
                return Err(error("conflicting publication retry"));
            }
            return Ok(expected.clone());
        }
        if let Some(existing) = &expected.publication
            && existing.generation == work.generation
        {
            if !matches!(observed, ObservedResult::Published(p) if p == existing) {
                return Err(error("conflicting publication retry"));
            }
            return Ok(expected.clone());
        }
        // Keep historical verification in audit; only the current generation authorizes outcomes.
        tx.execute(
            "DELETE FROM task_unchanged_results WHERE record_id = ?1",
            [record.cast_signed()],
        )
        .map_err(storage_error)?;
        tx.execute(
            "UPDATE task_stacks SET publication_json = NULL WHERE record_id = ?1",
            [record.cast_signed()],
        )
        .map_err(storage_error)?;
        match observed {
            ObservedResult::Published(p) => {
                tx.execute(
                    "UPDATE task_stacks SET publication_json = ?2 WHERE record_id = ?1",
                    params![
                        record.cast_signed(),
                        serde_json::to_string(p).map_err(json_error)?
                    ],
                )
                .map_err(storage_error)?;
                audit(
                    &tx,
                    record,
                    &format!(
                        "Verified non-draft publication: {} at {} (generation {})",
                        p.url, p.head_commit, p.generation
                    ),
                )?;
            }
            ObservedResult::Unchanged(u) => {
                tx.execute(
                    "INSERT INTO task_unchanged_results(record_id, evidence_json) VALUES (?1, ?2)",
                    params![
                        record.cast_signed(),
                        serde_json::to_string(u).map_err(json_error)?
                    ],
                )
                .map_err(storage_error)?;
                audit(
                    &tx,
                    record,
                    &format!(
                        "Verified unchanged result at {commit} (generation {})",
                        u.generation
                    ),
                )?;
            }
        }
        let result = required(&tx, record)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }
}

/// Persisted inheritance is rechecked at authorization, including legacy prepared Tasks.
pub(super) fn execution_gate(
    c: &Connection,
    record: u64,
    evidence: Option<&Evidence>,
) -> Result<(), RegistrationError> {
    let Some(stack) = load(c, record)? else {
        return Ok(());
    };
    compatible(c, record)?;
    let base = stack
        .base_commit
        .as_deref()
        .ok_or_else(|| error(format!("Task {record}: prepare the Stack before execution")))?;
    let pinned: Option<String> = c
        .query_row(
            "SELECT base_commit FROM task_base_commits WHERE record_id = ?1",
            [record.cast_signed()],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if pinned.as_deref() != Some(base) {
        return Err(error(format!(
            "Task {record}: retained Base Commit differs from Stack; preserve checkout and use explicit operator recovery"
        )));
    }
    predecessor_gate(c, &stack, base)?;
    let work = outcomes::state(c, record)?;
    if work.input.is_none()
        && work.output.is_none()
        && evidence
            .is_some_and(|e| !matches!(e, Evidence::Repository { commit, .. } if commit == base))
    {
        return Err(error(format!(
            "Task {record}: initial attempt must use exact Stack Base Commit {base}; preserve the checkout and investigate, do not reset or repin"
        )));
    }
    Ok(())
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
    let base = stack
        .base_commit
        .as_deref()
        .ok_or_else(|| error("Stack is not prepared"))?;
    predecessor_gate(c, &stack, base)?;
    if let Some(u) = stack.unchanged_result {
        if u.generation == generation
            && &u.evidence == evidence
            && matches!(evidence, Evidence::Repository { commit, .. } if Some(commit) == stack.base_commit.as_ref())
        {
            return Ok(());
        }
        return Err(error("unchanged publication evidence is stale"));
    }
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
