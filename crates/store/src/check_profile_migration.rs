//! Transactional effective-profile migration. Historical pins, Assignments and
//! Assessments are never rewritten; an effective selection and freshness
//! invalidation are committed together or not at all.
use super::{SqliteProjectRepository, load_agent_profile, load_record, outcomes, storage_error};
use forge_app::{
    RecordStatus, RecordType, RegistrationError,
    check_profile_migration::{Change, Receipt, Request},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

/// Effective revision of the Check aliased `c`: the latest migration, else the original pin.
pub(super) const EFFECTIVE_PROFILE: &str = "COALESCE((SELECT m.new_revision FROM check_profile_migration_changes m WHERE m.check_id = c.id ORDER BY m.migration_id DESC LIMIT 1), c.profile_revision)";

fn error(code: &'static str, message: &'static str) -> RegistrationError {
    RegistrationError::new(code, message)
}

fn receipt(
    c: &Connection,
    task: u64,
    id: i64,
    replayed: bool,
) -> Result<Receipt, RegistrationError> {
    let mut receipt = c.query_row(
        "SELECT operation_id, actor, message, generation_before, generation_after, created_at_millis FROM check_profile_migrations WHERE id = ?1",
        [id],
        |r| Ok(Receipt {
            task_id: task,
            operation_id: r.get(0)?,
            actor: r.get(1)?,
            message: r.get(2)?,
            generation_before: r.get::<_, i64>(3)?.cast_unsigned(),
            generation_after: r.get::<_, i64>(4)?.cast_unsigned(),
            created_at_millis: r.get::<_, i64>(5)?.cast_unsigned(),
            changes: Vec::new(),
            replayed,
        }),
    ).map_err(storage_error)?;
    receipt.changes = c
        .prepare("SELECT check_id, prior_revision, new_revision FROM check_profile_migration_changes WHERE migration_id = ?1 ORDER BY check_id")
        .map_err(storage_error)?
        .query_map([id], |r| Ok(Change {
            check_id: r.get::<_, i64>(0)?.cast_unsigned(),
            prior_revision: r.get(1)?,
            new_revision: r.get(2)?,
        }))
        .map_err(storage_error)?
        .collect::<Result<_, _>>()
        .map_err(storage_error)?;
    Ok(receipt)
}

/// Validate every selection against the effective pin, Task scope and durable
/// compatibility, returning prior/new revisions ordered by Check.
fn validated_changes(tx: &Connection, request: &Request) -> Result<Vec<Change>, RegistrationError> {
    let mut changes = Vec::new();
    for selection in &request.selections {
        let current: Option<Option<u32>> = tx
                .query_row(
                    &format!("SELECT {EFFECTIVE_PROFILE} FROM record_checks c WHERE c.id = ?1 AND c.record_id = ?2"),
                    params![selection.check_id.cast_signed(), request.task_id.cast_signed()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage_error)?;
        let current = current
            .ok_or_else(|| error("check-not-found", "Check does not belong to this Task"))?
            .ok_or_else(|| {
                error(
                    "invalid-check-profile",
                    "Check has no pinned Agent Profile to migrate from",
                )
            })?;
        if current == selection.profile_revision {
            return Err(error(
                "check-profile-unchanged",
                "Check already uses the selected profile revision",
            ));
        }
        // Task-type revisions only; revisions of other Record Types never match.
        let profile = load_agent_profile(tx, RecordType::Task, selection.profile_revision)
            .map_err(|e| RegistrationError::new("invalid-check-profile", e.message))?;
        let durable = profile.provider == "pi-durable"
            && profile
                .launch_policy
                .as_ref()
                .is_some_and(|p| p.runtime == "pi-durable");
        if !durable {
            return Err(error(
                "invalid-check-profile",
                "the selected revision is not a registered durable-compatible Task profile",
            ));
        }
        changes.push(Change {
            check_id: selection.check_id,
            prior_revision: current,
            new_revision: selection.profile_revision,
        });
    }
    changes.sort_by_key(|c| c.check_id);
    Ok(changes)
}

impl SqliteProjectRepository {
    pub(super) fn migrate_profiles(&self, request: &Request) -> Result<Receipt, RegistrationError> {
        request.validate()?;
        let mut c = self
            .connection
            .lock()
            .map_err(|_| error("storage", "storage lock poisoned"))?;
        // The writer is reserved first so claim/retirement and migration serialize.
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let task = request.task_id;
        self.ensure_record_workspace(&tx, task)?;
        let digest = request.digest();
        let prior: Option<(i64, String)> = tx
            .query_row(
                "SELECT id, request_sha256 FROM check_profile_migrations WHERE record_id = ?1 AND operation_id = ?2",
                params![task.cast_signed(), request.operation_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some((id, sha)) = prior {
            // A lost reply after later changes still discovers the original result.
            if sha != digest {
                return Err(error(
                    "check-profile-migration-conflict",
                    "this operation ID already recorded a different migration; choose a new operation ID",
                ));
            }
            return receipt(&tx, task, id, true);
        }
        let record = load_record(&tx, task)?;
        if record.record_type != RecordType::Task {
            return Err(error(
                "check-profile-migration-scope",
                "only Task Checks can migrate profiles",
            ));
        }
        if record.status == RecordStatus::Done {
            return Err(error(
                "invalid-transition",
                "Done Tasks cannot migrate Check profiles",
            ));
        }
        super::execution_modes::safe_ownership(&tx, task)?;
        let open: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM record_attempts WHERE record_id = ?1 AND closed = 0)",
                [task.cast_signed()],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        if open {
            return Err(error(
                "attempt-active",
                "close or explicitly invalidate the current attempt before migrating Check profiles",
            ));
        }
        let before = outcomes::state(&tx, task)?;
        if before.generation != request.expected_generation {
            return Err(error(
                "stale-work-generation",
                "work freshness changed; inspect the Task before retrying",
            ));
        }
        let changes = validated_changes(&tx, request)?;
        let has_work: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM record_work WHERE record_id = ?1) OR EXISTS(SELECT 1 FROM check_assessments a JOIN record_checks c ON c.id = a.check_id WHERE c.record_id = ?1)",
                [task.cast_signed()],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        if has_work {
            outcomes::invalidate(&tx, task, &request.actor, &request.message, None)?;
        }
        let after = outcomes::state(&tx, task)?;
        tx.execute(
            "INSERT INTO check_profile_migrations(record_id, operation_id, request_sha256, actor, message, generation_before, generation_after, created_at_millis) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                task.cast_signed(),
                request.operation_id,
                digest,
                request.actor,
                request.message,
                before.generation.cast_signed(),
                after.generation.cast_signed(),
                super::now_millis()?
            ],
        )
        .map_err(storage_error)?;
        let id = tx.last_insert_rowid();
        for change in &changes {
            tx.execute(
                "INSERT INTO check_profile_migration_changes(migration_id, check_id, prior_revision, new_revision) VALUES (?1, ?2, ?3, ?4)",
                params![id, change.check_id.cast_signed(), change.prior_revision, change.new_revision],
            )
            .map_err(storage_error)?;
        }
        let result = receipt(&tx, task, id, false)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }

    pub(super) fn profile_migrations(&self, task: u64) -> Result<Vec<Receipt>, RegistrationError> {
        let c = self
            .connection
            .lock()
            .map_err(|_| error("storage", "storage lock poisoned"))?;
        self.ensure_record_workspace(&c, task)?;
        let ids = c
            .prepare("SELECT id FROM check_profile_migrations WHERE record_id = ?1 ORDER BY id")
            .map_err(storage_error)?
            .query_map([task.cast_signed()], |r| r.get::<_, i64>(0))
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        ids.into_iter()
            .map(|id| receipt(&c, task, id, false))
            .collect()
    }
}
