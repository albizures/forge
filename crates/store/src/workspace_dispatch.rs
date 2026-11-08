//! Durable Workspace selections and exclusive provider identities.
use super::{SqliteProjectRepository, load_agent_profile, storage_error, workspaces};
use forge_app::{
    DispatchWorkspace, RecordType, RegistrationError, WorkspaceDispatchConfiguration,
    WorkspaceDispatchRepository,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub(super) fn initialize(connection: &Connection) -> Result<(), RegistrationError> {
    let columns = connection
        .prepare("PRAGMA table_info(agent_assignments)")
        .map_err(storage_error)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns
        .iter()
        .any(|column| column == "dispatch_workspace_id")
    {
        connection
            .execute_batch("ALTER TABLE agent_assignments ADD COLUMN dispatch_workspace_id TEXT;")
            .map_err(storage_error)?;
    }
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS dispatch_provider_generation (
            singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
            generation INTEGER NOT NULL CHECK(generation >= 0)
         );
         INSERT OR IGNORE INTO dispatch_provider_generation VALUES (1, 0);
         CREATE TABLE IF NOT EXISTS workspace_dispatch_configuration (
            workspace_id TEXT PRIMARY KEY NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            task_profile_type TEXT NOT NULL DEFAULT 'task' CHECK(task_profile_type = 'task'),
            task_profile_revision INTEGER CHECK(task_profile_revision > 0),
            grilling_profile_type TEXT NOT NULL DEFAULT 'grilling' CHECK(grilling_profile_type = 'grilling'),
            grilling_profile_revision INTEGER CHECK(grilling_profile_revision > 0),
            herdr_workspace_id TEXT UNIQUE CHECK(herdr_workspace_id IS NULL OR length(trim(herdr_workspace_id)) > 0),
            verified_generation INTEGER CHECK(verified_generation >= 0),
            FOREIGN KEY(task_profile_type, task_profile_revision) REFERENCES agent_profiles(record_type, revision),
            FOREIGN KEY(grilling_profile_type, grilling_profile_revision) REFERENCES agent_profiles(record_type, revision)
         );
         CREATE TRIGGER IF NOT EXISTS assignment_workspace_binding
         AFTER INSERT ON agent_assignments
         BEGIN UPDATE agent_assignments SET workspace_id = (SELECT workspace_id FROM records WHERE id = NEW.record_id),
           dispatch_workspace_id = (SELECT herdr_workspace_id FROM workspace_dispatch_configuration WHERE workspace_id = (SELECT workspace_id FROM records WHERE id = NEW.record_id))
           WHERE id = NEW.id; END;
         CREATE TRIGGER IF NOT EXISTS assignment_workspace_binding_immutable
         BEFORE UPDATE OF workspace_id, dispatch_workspace_id ON agent_assignments
         WHEN OLD.workspace_id IS NOT NULL
         BEGIN SELECT RAISE(ABORT, 'Assignment Workspace and provider binding are immutable'); END;
         CREATE TRIGGER IF NOT EXISTS dispatch_socket_insert_generation
         AFTER INSERT ON agent_dispatch_configuration WHEN NEW.herdr_socket_path IS NOT NULL
         BEGIN UPDATE dispatch_provider_generation SET generation = generation + 1 WHERE singleton = 1; END;
         CREATE TRIGGER IF NOT EXISTS dispatch_socket_update_generation
         AFTER UPDATE OF herdr_socket_path ON agent_dispatch_configuration
         WHEN OLD.herdr_socket_path IS NOT NEW.herdr_socket_path
         BEGIN UPDATE dispatch_provider_generation SET generation = generation + 1 WHERE singleton = 1; END;"
    ).map_err(storage_error)
}

pub(super) fn selected_revision(
    connection: &Connection,
    record_id: u64,
    kind: RecordType,
) -> Result<u32, RegistrationError> {
    let selected: Option<(Selections, i64)> = connection.query_row(
        "SELECT c.task_profile_revision, c.grilling_profile_revision, c.herdr_workspace_id, c.verified_generation, g.generation FROM records r JOIN workspace_dispatch_configuration c ON c.workspace_id = r.workspace_id JOIN dispatch_provider_generation g ON g.singleton = 1 WHERE r.id = ?1",
        [record_id.cast_signed()], |row| Ok((Selections { task: row.get(0)?, grilling: row.get(1)?, binding: row.get(2)?, verified: row.get(3)? }, row.get(4)?)),
    ).optional().map_err(storage_error)?;
    let Some((
        Selections {
            task: Some(task),
            grilling: Some(grilling),
            binding: Some(_),
            verified: Some(verified),
        },
        generation,
    )) = selected
    else {
        return Err(RegistrationError::new(
            "workspace-dispatch-incomplete",
            "select valid Task and Grilling profiles and verify a Herdr binding in this Workspace before dispatch",
        ));
    };
    if verified != generation {
        return Err(RegistrationError::new(
            "provider-verification-stale",
            "Herdr socket changed; verify this Workspace binding again before dispatch",
        ));
    }
    load_agent_profile(connection, RecordType::Task, task)?;
    load_agent_profile(connection, RecordType::Grilling, grilling)?;
    Ok(if kind == RecordType::Task {
        task
    } else {
        grilling
    })
}

fn generation(connection: &Connection) -> Result<u64, RegistrationError> {
    connection
        .query_row(
            "SELECT generation FROM dispatch_provider_generation WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0).map(i64::cast_unsigned),
        )
        .map_err(storage_error)
}

#[derive(Default)]
struct Selections {
    task: Option<u32>,
    grilling: Option<u32>,
    binding: Option<String>,
    verified: Option<i64>,
}

impl WorkspaceDispatchRepository for SqliteProjectRepository {
    fn workspace_dispatch_configuration(
        &self,
        workspace: &str,
    ) -> Result<WorkspaceDispatchConfiguration, RegistrationError> {
        let mut connection = self.connection.lock().map_err(|_| {
            RegistrationError::new("storage", "Workspace dispatch storage lock is poisoned")
        })?;
        // All facts, including the endpoint generation, come from one read snapshot even
        // when another daemon/request uses a separate connection to this store.
        let tx = connection.transaction().map_err(storage_error)?;
        workspaces::get(&tx, workspace)?;
        let selected = tx.query_row(
            "SELECT task_profile_revision, grilling_profile_revision, herdr_workspace_id, verified_generation FROM workspace_dispatch_configuration WHERE workspace_id = ?1", [workspace],
            |row| Ok(Selections { task: row.get(0)?, grilling: row.get(1)?, binding: row.get(2)?, verified: row.get(3)? }),
        ).optional().map_err(storage_error)?;
        let Selections {
            task,
            grilling,
            binding,
            verified,
        } = selected.unwrap_or_default();
        let provider_generation = generation(&tx)?;
        Ok(WorkspaceDispatchConfiguration {
            workspace_id: workspace.into(),
            task_profile: task
                .map(|revision| load_agent_profile(&tx, RecordType::Task, revision))
                .transpose()?,
            grilling_profile: grilling
                .map(|revision| load_agent_profile(&tx, RecordType::Grilling, revision))
                .transpose()?,
            provider_verified: binding.is_some()
                && verified.map(i64::cast_unsigned) == Some(provider_generation),
            binding: binding.map(|id| DispatchWorkspace { id }),
            provider_generation,
        })
    }

    fn select_workspace_profile(
        &self,
        workspace: &str,
        record_type: RecordType,
        revision: Option<u32>,
    ) -> Result<(), RegistrationError> {
        let column = match record_type {
            RecordType::Task => "task_profile_revision",
            RecordType::Grilling => "grilling_profile_revision",
            _ => {
                return Err(RegistrationError::new(
                    "unsupported-profile-selection",
                    "Workspace dispatch selects Task and Grilling profiles only",
                ));
            }
        };
        let mut connection = self.connection.lock().map_err(|_| {
            RegistrationError::new("storage", "Workspace dispatch storage lock is poisoned")
        })?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        workspaces::get(&tx, workspace)?;
        if let Some(revision) = revision {
            load_agent_profile(&tx, record_type, revision)?;
        }
        tx.execute(&format!("INSERT INTO workspace_dispatch_configuration (workspace_id, {column}) VALUES (?1, ?2) ON CONFLICT(workspace_id) DO UPDATE SET {column} = excluded.{column}"), params![workspace, revision]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }

    fn configure_workspace_acknowledgement_timeout(
        &self,
        workspace: &str,
        timeout_millis: Option<u64>,
    ) -> Result<forge_app::AgentProfile, RegistrationError> {
        let timeout = i64::try_from(timeout_millis.unwrap_or(300_000))
            .ok()
            .filter(|timeout| *timeout > 0 && *timeout <= i64::MAX / 2)
            .ok_or_else(|| {
                RegistrationError::new(
                    "invalid-acknowledgement-timeout",
                    "acknowledgement timeout must be positive and in range",
                )
            })?;
        let mut connection = self.connection.lock().map_err(|_| {
            RegistrationError::new("storage", "Workspace dispatch storage lock is poisoned")
        })?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        workspaces::get(&tx, workspace)?;
        let selected: Option<Option<u32>> = tx.query_row("SELECT task_profile_revision FROM workspace_dispatch_configuration WHERE workspace_id = ?1", [workspace], |row| row.get(0)).optional().map_err(storage_error)?;
        let selected = selected.flatten().ok_or_else(|| {
            RegistrationError::new(
                "task-profile-required",
                "select a valid Task profile before changing its acknowledgement timeout",
            )
        })?;
        load_agent_profile(&tx, RecordType::Task, selected)?;
        let revision: u32 = tx
            .query_row(
                "SELECT MAX(revision) + 1 FROM agent_profiles WHERE record_type = 'task'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        tx.execute("INSERT INTO agent_profiles (record_type, revision, provider, agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis, acknowledgement_timeout_millis, acknowledgement_timeout_overridden)
            SELECT record_type, ?1, provider, agent_kind, model, prompt_template, retry_limit, retry_backoff_millis, startup_timeout_millis, unknown_state_grace_millis, ?2, ?3 FROM agent_profiles WHERE record_type = 'task' AND revision = ?4", params![revision, timeout, timeout_millis.is_some(), selected]).map_err(storage_error)?;
        tx.execute("UPDATE workspace_dispatch_configuration SET task_profile_revision = ?2 WHERE workspace_id = ?1", params![workspace, revision]).map_err(storage_error)?;
        let profile = load_agent_profile(&tx, RecordType::Task, revision)?;
        tx.commit().map_err(storage_error)?;
        Ok(profile)
    }

    fn retain_verified_workspace_binding(
        &self,
        workspace: &str,
        binding: Option<&DispatchWorkspace>,
        provider_generation: u64,
    ) -> Result<(), RegistrationError> {
        if binding.is_some_and(|binding| binding.id.trim().is_empty()) {
            return Err(RegistrationError::new(
                "invalid-dispatch-workspace",
                "Herdr binding requires an exact nonblank identity",
            ));
        }
        let mut connection = self.connection.lock().map_err(|_| {
            RegistrationError::new("storage", "Workspace dispatch storage lock is poisoned")
        })?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        workspaces::get(&tx, workspace)?;
        if binding.is_some() && provider_generation != generation(&tx)? {
            return Err(RegistrationError::new(
                "provider-verification-stale",
                "Herdr socket changed during verification; verify the bound identity again",
            ));
        }
        let current: Option<Option<String>> = tx.query_row("SELECT herdr_workspace_id FROM workspace_dispatch_configuration WHERE workspace_id = ?1", [workspace], |row| row.get(0)).optional().map_err(storage_error)?;
        let identity = binding.map(|binding| binding.id.as_str());
        if current.flatten().as_deref() != identity {
            let busy: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_assignments a JOIN records record ON record.id = a.record_id
                  WHERE record.workspace_id = ?1 AND (a.closed_at_millis IS NULL OR EXISTS(
                    SELECT 1 FROM agent_assignment_resources resource WHERE resource.assignment_id = a.id AND resource.retired_at_millis IS NULL)))",
                [workspace], |row| row.get(0),
            ).map_err(storage_error)?;
            if busy {
                return Err(RegistrationError::new(
                    "dispatch-workspace-busy",
                    "Workspace has live Assignments or unresolved provider resources; inspect and safely retire/resolve them before changing its binding",
                ));
            }
        }
        if let Some(identity) = identity {
            let duplicate: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM workspace_dispatch_configuration WHERE herdr_workspace_id = ?1 AND workspace_id != ?2)", params![identity, workspace], |row| row.get(0)).map_err(storage_error)?;
            if duplicate {
                return Err(RegistrationError::new(
                    "dispatch-workspace-conflict",
                    "Herdr identity is already bound to another Forge Workspace",
                ));
            }
        }
        tx.execute("INSERT INTO workspace_dispatch_configuration (workspace_id, herdr_workspace_id, verified_generation) VALUES (?1, ?2, ?3) ON CONFLICT(workspace_id) DO UPDATE SET herdr_workspace_id = excluded.herdr_workspace_id, verified_generation = excluded.verified_generation", params![workspace, identity, binding.map(|_| provider_generation.cast_signed())]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }
}
