use super::{SqliteProjectRepository, now_millis, storage_error};
use forge_app::{
    RegistrationError,
    check_templates::{
        CheckCriterion, CheckTemplate, TemplateChange, TemplatePage, TemplateReplacement,
    },
};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn schema(connection: &Connection) -> Result<(), RegistrationError> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS project_check_templates (
        project_slug TEXT PRIMARY KEY NOT NULL REFERENCES projects(slug),
        version INTEGER NOT NULL, criteria_json TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS project_check_template_changes (
        id INTEGER PRIMARY KEY NOT NULL, project_slug TEXT NOT NULL REFERENCES projects(slug),
        version INTEGER NOT NULL, criteria_json TEXT NOT NULL, actor TEXT NOT NULL,
        message TEXT NOT NULL, created_at_millis INTEGER NOT NULL, UNIQUE(project_slug, version));
        CREATE INDEX IF NOT EXISTS template_changes_by_project ON project_check_template_changes(project_slug, id);")
        .map_err(storage_error)
}

fn template_storage_error(error: impl std::fmt::Display) -> RegistrationError {
    RegistrationError::new("storage", error.to_string())
}

fn current(connection: &Connection, project: &str) -> Result<CheckTemplate, RegistrationError> {
    let row: Option<(i64, String)> = connection
        .query_row(
            "SELECT version, criteria_json FROM project_check_templates WHERE project_slug = ?1",
            [project],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    match row {
        Some((version, json)) => Ok(CheckTemplate {
            version: version.cast_unsigned(),
            criteria: serde_json::from_str(&json).map_err(template_storage_error)?,
        }),
        None => Ok(CheckTemplate {
            version: 0,
            criteria: vec![],
        }),
    }
}

// The caller holds the Task-opening write transaction: replacement cannot change
// the selected template between this read, Check inserts and Task commit.
pub(super) fn provision(
    connection: &Connection,
    task: u64,
    project: Option<&str>,
) -> Result<(), RegistrationError> {
    let Some(project) = project else {
        return Ok(());
    };
    for (index, criterion) in current(connection, project)?.criteria.iter().enumerate() {
        connection.execute("INSERT INTO task_checks (task_id, name, description, position) VALUES (?1, ?2, ?3, ?4)",
            params![task.cast_signed(), criterion.name, criterion.description, i64::try_from(index + 1).map_err(template_storage_error)?]).map_err(storage_error)?;
    }
    Ok(())
}

impl SqliteProjectRepository {
    fn check_template_project(
        &self,
        connection: &Connection,
        project: &str,
    ) -> Result<(), RegistrationError> {
        let owner: Option<String> = connection
            .query_row(
                "SELECT workspace_id FROM projects WHERE slug = ?1",
                [project],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if owner.is_none()
            || self
                .record_workspace
                .as_ref()
                .is_some_and(|selected| Some(selected) != owner.as_ref())
        {
            return Err(RegistrationError::new(
                "project-not-found",
                "Project does not exist in the selected Workspace",
            ));
        }
        Ok(())
    }
    pub(super) fn template_page(
        &self,
        project: &str,
        after: Option<u64>,
        limit: u32,
    ) -> Result<TemplatePage, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock is poisoned"))?;
        let transaction = connection.transaction().map_err(storage_error)?;
        self.check_template_project(&transaction, project)?;
        if !(1..=100).contains(&limit) {
            return Err(RegistrationError::new(
                "invalid-page",
                "limit must be between 1 and 100",
            ));
        }
        let template = current(&transaction, project)?;
        let rows = transaction.prepare("SELECT id, version, criteria_json, actor, message, created_at_millis FROM project_check_template_changes WHERE project_slug = ?1 AND id > ?2 ORDER BY id LIMIT ?3").map_err(storage_error)?
            .query_map(params![project, after.unwrap_or(0).cast_signed(), limit + 1], |row| Ok((
                row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?,
                row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, i64>(5)?)))
            .map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
        let mut history = rows
            .into_iter()
            .map(|(id, version, json, actor, message, timestamp)| {
                Ok(TemplateChange {
                    id: id.cast_unsigned(),
                    version: version.cast_unsigned(),
                    criteria: serde_json::from_str::<Vec<CheckCriterion>>(&json)
                        .map_err(template_storage_error)?,
                    actor,
                    message,
                    created_at_millis: timestamp.cast_unsigned(),
                })
            })
            .collect::<Result<Vec<_>, RegistrationError>>()?;
        let more = history.len() > limit as usize;
        history.truncate(limit as usize);
        let next_cursor = if more {
            history.last().map(|change| change.id)
        } else {
            None
        };
        Ok(TemplatePage {
            version: template.version,
            criteria: template.criteria,
            history,
            next_cursor,
        })
    }
    pub(super) fn replace_template(
        &self,
        project: &str,
        input: &TemplateReplacement,
    ) -> Result<CheckTemplate, RegistrationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| RegistrationError::new("storage", "storage lock is poisoned"))?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        self.check_template_project(&transaction, project)?;
        let criteria = input.normalized_criteria()?;
        let old = current(&transaction, project)?;
        if old.version != input.expected_version {
            return Err(RegistrationError::new(
                "stale-version",
                "Check Template version changed; read it again before replacement",
            ));
        }
        let version = old.version + 1;
        let json = serde_json::to_string(&criteria).map_err(template_storage_error)?;
        transaction.execute("INSERT INTO project_check_templates (project_slug, version, criteria_json) VALUES (?1, ?2, ?3) ON CONFLICT(project_slug) DO UPDATE SET version = excluded.version, criteria_json = excluded.criteria_json", params![project, version.cast_signed(), json]).map_err(storage_error)?;
        transaction.execute("INSERT INTO project_check_template_changes (project_slug, version, criteria_json, actor, message, created_at_millis) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![project, version.cast_signed(), json, input.actor, input.message, now_millis()?]).map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(CheckTemplate { version, criteria })
    }
}
