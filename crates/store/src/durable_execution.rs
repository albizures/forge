//! Durable admission identity and result journal; no process or network calls.
use crate::{SqliteProjectRepository, now_millis, storage_error, task_attempts};
use forge_app::{
    RegistrationError,
    durable_execution::{Admission, Backend, Launch, Repository, Runtime},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

mod completion;
mod effect_resolution;
mod effects;
mod followup;
mod purge;
mod recovery;
mod recovery_authorization;
mod retirement;
mod status;

pub(super) fn schema(c: &Connection) -> Result<(), RegistrationError> {
    c.execute_batch(include_str!("upgrade/durable_execution.sql"))
        .map_err(storage_error)?;
    c.execute_batch(include_str!("upgrade/authorization_leases.sql"))
        .map_err(storage_error)?;
    c.execute_batch(include_str!("upgrade/durable_retirement.sql"))
        .map_err(storage_error)?;
    c.execute_batch(include_str!("upgrade/durable_followups.sql"))
        .map_err(storage_error)?;
    c.execute_batch(include_str!("upgrade/check_profile_migrations.sql"))
        .map_err(storage_error)?;
    c.execute_batch(include_str!("upgrade/durable_purges.sql"))
        .map_err(storage_error)
}

fn error(code: &'static str) -> RegistrationError {
    RegistrationError::new(
        code,
        "durable admission scope, binding or operation is not valid",
    )
}

fn scope(c: &Connection, workspace: &str, assignment: &str) -> Result<(), RegistrationError> {
    let valid: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_assignments WHERE id = ?1 AND workspace_id = ?2)",
            params![assignment, workspace],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if valid {
        Ok(())
    } else {
        Err(error("durable-scope-denied"))
    }
}

fn authorize(
    c: &Connection,
    workspace: &str,
    launch: &Launch,
    live: bool,
) -> Result<(), RegistrationError> {
    scope(c, workspace, &launch.assignment_id)?;
    let valid: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_assignments a JOIN agent_profiles p ON p.record_type = a.profile_record_type AND p.revision = a.profile_revision WHERE a.id = ?1 AND p.provider = 'pi-durable' AND a.profile_record_type = 'task' AND a.profile_revision = ?2 AND (?3 = 0 OR (a.closed_at_millis IS NULL AND a.state = 'provisioning')))",
        params![launch.assignment_id, launch.profile_revision, live], |r| r.get(0),
    ).map_err(storage_error)?;
    if !valid {
        return Err(error("durable-binding-denied"));
    }
    let attempt = task_attempts::binding(c, &launch.assignment_id)?
        .ok_or_else(|| error("durable-binding-denied"))?;
    let matches: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_assignments a JOIN record_attempts r ON r.id = ?2 AND r.record_id = a.record_id WHERE a.id = ?1 AND (?3 = 0 OR r.closed = 0))",
        params![launch.assignment_id, launch.attempt_id, live], |r| r.get(0),
    ).map_err(storage_error)?;
    if attempt.id != launch.attempt_id || !matches {
        return Err(error("durable-binding-denied"));
    }
    if live && crate::outcomes::state(c, attempt.record_id)?.generation != attempt.generation {
        return Err(error("durable-binding-stale"));
    }
    Ok(())
}

fn load(c: &Connection, assignment: &str) -> Result<Option<Runtime>, RegistrationError> {
    let pair: Option<(String, String, String)> = c.query_row(
        "SELECT backend, launch_json, admission_json FROM durable_assignment_runtimes WHERE assignment_id = ?1",
        [assignment], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).optional().map_err(storage_error)?;
    pair.map(|(backend, launch, admission)| {
        Ok(Runtime {
            backend: match backend.as_str() {
                "pi-durable" => Backend::PiDurable,
                _ => return Err(error("durable-backend-incompatible")),
            },
            launch: serde_json::from_str(&launch).map_err(|_| error("durable-storage-invalid"))?,
            admission: serde_json::from_str(&admission)
                .map_err(|_| error("durable-storage-invalid"))?,
        })
    })
    .transpose()
}

// Backend-specific admission evidence for accepted Check handoff. This only
// permits revocation/cleanup to begin; it never proves process absence or frees
// capacity. Missing Herdr agent-name metadata is not durable launch uncertainty.
pub(super) fn handoff_admitted(
    c: &Connection,
    assignment: &str,
    attempt: &forge_app::outcomes::Attempt,
) -> Result<bool, RegistrationError> {
    let Some(runtime) = load(c, assignment)? else {
        return Ok(false);
    };
    runtime.launch.validate()?;
    let (revision, record): (u32, i64) = c
        .query_row(
            "SELECT profile_revision, record_id FROM agent_assignments WHERE id = ?1",
            [assignment],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(storage_error)?;
    let record = u64::try_from(record).map_err(|_| error("durable-storage-invalid"))?;
    Ok(runtime.launch.assignment_id == assignment
        && runtime.launch.attempt_id == attempt.id
        && runtime.launch.profile_revision == revision
        && attempt.actor == assignment
        && attempt.record_id == record
        && matches!(runtime.admission, Admission::Accepted { ref admission_id } if !admission_id.trim().is_empty()))
}

fn exact(runtime: &Runtime, launch: &Launch) -> Result<(), RegistrationError> {
    if &runtime.launch == launch {
        Ok(())
    } else {
        Err(error("durable-operation-conflict"))
    }
}

fn json(value: &Admission) -> Result<String, RegistrationError> {
    serde_json::to_string(value).map_err(|_| error("durable-storage-invalid"))
}

impl Repository for SqliteProjectRepository {
    fn reserve(&self, workspace: &str, launch: &Launch) -> Result<Runtime, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        authorize(&tx, workspace, launch, false)?;
        if let Some(runtime) = load(&tx, &launch.assignment_id)? {
            exact(&runtime, launch)?;
            tx.commit().map_err(storage_error)?;
            return Ok(runtime);
        }
        authorize(&tx, workspace, launch, true)?;
        launch.validate()?;
        let used: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM durable_assignment_runtimes WHERE operation_id = ?1 OR runtime_id = ?2 OR conversation_id = ?3 OR store_id = ?4)",
            params![launch.operation_id, launch.runtime_id, launch.conversation_id, launch.store_id], |r| r.get(0),
        ).map_err(storage_error)?;
        if used {
            return Err(error("durable-operation-conflict"));
        }
        tx.execute("INSERT INTO durable_assignment_runtimes VALUES (?1, 'pi-durable', ?2, ?3, ?4, ?5, ?6, ?7)",
            params![launch.assignment_id, launch.operation_id, launch.runtime_id, launch.conversation_id, launch.store_id, serde_json::to_string(launch).map_err(|_| error("durable-storage-invalid"))?, json(&Admission::Pending)?],
        ).map_err(storage_error)?;
        // The existing capacity/readiness/worktree guards all account for this resource.
        tx.execute("INSERT INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) VALUES (?1, 'durable-runtime', ?2, ?3)",
            params![launch.assignment_id, launch.runtime_id, now_millis()?],
        ).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(Runtime {
            backend: Backend::PiDurable,
            launch: launch.clone(),
            admission: Admission::Pending,
        })
    }

    fn begin_send(&self, workspace: &str, launch: &Launch) -> Result<bool, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        authorize(&tx, workspace, launch, true)?;
        let runtime =
            load(&tx, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
        exact(&runtime, launch)?;
        let send = runtime.admission == Admission::Pending;
        if send {
            tx.execute("UPDATE durable_assignment_runtimes SET admission_json = ?2 WHERE assignment_id = ?1",
                params![launch.assignment_id, json(&Admission::Unknown)?],
            ).map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)?;
        Ok(send)
    }

    fn record_result(
        &self,
        workspace: &str,
        launch: &Launch,
        result: &Admission,
    ) -> Result<Runtime, RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        // A response may arrive after revocation: retain its result, never renew authority.
        authorize(&tx, workspace, launch, false)?;
        let mut runtime =
            load(&tx, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
        exact(&runtime, launch)?;
        let reconciled = runtime.admission.reconcile(result)?;
        if reconciled != runtime.admission {
            tx.execute("UPDATE durable_assignment_runtimes SET admission_json = ?2 WHERE assignment_id = ?1",
                params![launch.assignment_id, json(&reconciled)?],
            ).map_err(storage_error)?;
            runtime.admission = reconciled;
        }
        tx.commit().map_err(storage_error)?;
        Ok(runtime)
    }

    fn runtime(
        &self,
        workspace: &str,
        assignment: &str,
    ) -> Result<Option<Runtime>, RegistrationError> {
        let c = self.connection.lock().map_err(|_| error("storage"))?;
        scope(&c, workspace, assignment)?;
        load(&c, assignment)
    }
}

impl forge_app::authorization_lease::Repository for SqliteProjectRepository {
    fn grant_lease(
        &self,
        workspace: &str,
        lease: &forge_app::authorization_lease::Lease,
        now: u64,
    ) -> Result<(), RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        authorize(&tx, workspace, &lease.launch, false)?;
        let live: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_assignments a JOIN record_attempts r ON r.id = ?2 WHERE a.id = ?1 AND a.closed_at_millis IS NULL AND a.state IN ('provisioning', 'working', 'idle') AND r.closed = 0)",
            params![lease.launch.assignment_id, lease.launch.attempt_id], |r| r.get(0),
        ).map_err(storage_error)?;
        if !live {
            return Err(error("assignment-lease-denied"));
        }
        let runtime = load(&tx, &lease.launch.assignment_id)?
            .ok_or_else(|| error("durable-intent-missing"))?;
        exact(&runtime, &lease.launch)?;
        if matches!(runtime.admission, Admission::Rejected { .. }) {
            return Err(error("assignment-lease-denied"));
        }
        crate::spec_plans::assignment_execution(&tx, &lease.launch.assignment_id)?;
        let attempt = task_attempts::binding(&tx, &lease.launch.assignment_id)?
            .ok_or_else(|| error("durable-binding-denied"))?;
        if attempt.generation != lease.generation
            || crate::outcomes::state(&tx, attempt.record_id)?.generation != lease.generation
        {
            return Err(error("durable-binding-stale"));
        }
        forge_app::validate_task_dispatch_mode(&crate::load_record(&tx, attempt.record_id)?)?;
        let blocked: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_assignments a WHERE a.id = ?1 AND (EXISTS(SELECT 1 FROM dispatch_holds h WHERE h.record_id = a.record_id) OR EXISTS(SELECT 1 FROM agent_assignment_resources r WHERE r.assignment_id = a.id AND r.retired_at_millis IS NULL AND r.resource_kind NOT IN ('durable-runtime', 'provision-intent'))))",
            [&lease.launch.assignment_id], |r| r.get(0),
        ).map_err(storage_error)?;
        if blocked {
            return Err(error("assignment-lease-blocked"));
        }
        let previous: Option<String> = tx.query_row(
            "SELECT lease_json FROM assignment_authorization_leases WHERE assignment_id = ?1 ORDER BY sequence DESC LIMIT 1",
            [&lease.launch.assignment_id], |r| r.get(0),
        ).optional().map_err(storage_error)?;
        let previous: Option<forge_app::authorization_lease::Lease> = previous
            .map(|json| serde_json::from_str(&json).map_err(|_| error("durable-storage-invalid")))
            .transpose()?;
        // Retries return no new deadline and only while current authority is valid.
        if previous.as_ref() == Some(lease) {
            if now < lease.issued_boottime_millis || now >= lease.expires_boottime_millis {
                return Err(error("assignment-lease-denied"));
            }
        } else {
            lease.validate(previous.as_ref(), now)?;
            tx.execute("INSERT INTO assignment_authorization_leases(assignment_id, sequence, lease_json) VALUES (?1, ?2, ?3)",
                params![lease.launch.assignment_id, lease.sequence, serde_json::to_string(lease).map_err(|_| error("durable-storage-invalid"))?],
            ).map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)
    }
}

impl forge_app::authorization_lease::session::SessionRepository for SqliteProjectRepository {
    fn retain_failure(
        &self,
        workspace: &str,
        launch: &Launch,
        failure: forge_app::authorization_lease::session::Failure,
    ) -> Result<(), RegistrationError> {
        let mut c = self.connection.lock().map_err(|_| error("storage"))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        // Diagnostics must survive revocation/freshness changes, but cannot be
        // attached to a foreign Assignment or a different retained runtime.
        scope(&tx, workspace, &launch.assignment_id)?;
        let runtime =
            load(&tx, &launch.assignment_id)?.ok_or_else(|| error("durable-intent-missing"))?;
        exact(&runtime, launch)?;
        tx.execute(
            "INSERT OR IGNORE INTO agent_assignment_resources(assignment_id, resource_kind, resource_id, created_at_millis) VALUES (?1, 'authorization-uncertain', ?2, ?3)",
            params![launch.assignment_id, failure.code(), now_millis()?],
        ).map_err(storage_error)?;
        tx.execute(
            "INSERT OR IGNORE INTO dispatch_holds(record_id, reason) SELECT record_id, 'Durable execution authorization is uncertain; inspect retained Assignment, grant and runtime evidence before retirement or replacement.' FROM agent_assignments WHERE id = ?1",
            [&launch.assignment_id],
        ).map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }
}

#[cfg(test)]
#[path = "durable_execution_tests.rs"]
mod tests;
