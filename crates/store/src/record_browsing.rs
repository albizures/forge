//! Small read projections over durable Record Updates; no invented historical dates.
use super::{SqliteProjectRepository, storage_error};
use forge_app::{RegistrationError, record_browsing::RecordDates};

fn timestamp(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|value| {
            u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
        })
        .transpose()
}

pub(super) fn record_dates(
    store: &SqliteProjectRepository,
    id: u64,
) -> Result<RecordDates, RegistrationError> {
    store.record_workspace(id)?;
    let connection = store
        .connection
        .lock()
        .map_err(|_| RegistrationError::new("storage", "store lock poisoned"))?;
    connection.query_row(
        "SELECT MIN(CASE WHEN kind = 'created' THEN created_at_millis END), MAX(created_at_millis) FROM record_updates WHERE record_id = ?1",
        [id.cast_signed()],
        |row| Ok(RecordDates {
            opened_at_millis: timestamp(row, 0)?,
            updated_at_millis: timestamp(row, 1)?,
        }),
    ).map_err(storage_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_app::{
        Application, NewRecord, Project, RecordRelationshipKind, RecordRepository,
        RepositoryInspector, WorkspaceRepository,
    };
    use std::path::Path;

    struct Inspector;
    impl RepositoryInspector for Inspector {
        fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
            Err(RegistrationError::new("unused", "unused"))
        }
    }

    #[test]
    fn durable_dates_include_comments_and_relationships_and_preserve_opening() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.create_workspace("a", "A", Path::new("/")).unwrap();
        let scoped = store.records_in_workspace("a").unwrap();
        let open = || {
            scoped
                .open_record(&NewRecord {
                    body: "body".into(),
                    project_slugs: vec![],
                    generated_by_ids: vec![],
                })
                .unwrap()
        };
        let first = open();
        let second = open();
        let third = open();
        store
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE record_updates SET created_at_millis = 1000", [])
            .unwrap();
        let comment = scoped
            .add_record_comment(first.id, "comment", None)
            .unwrap();
        scoped.delete_record_comment(first.id, comment.id).unwrap();
        scoped
            .relate_records(second.id, third.id, RecordRelationshipKind::GeneratedBy)
            .unwrap();
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE record_updates SET created_at_millis = 3000 WHERE kind = 'comment-added'",
                [],
            )
            .unwrap();
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE record_updates SET created_at_millis = 4000 WHERE kind = 'comment-deleted'",
                [],
            )
            .unwrap();
        store.connection.lock().unwrap().execute("UPDATE record_updates SET created_at_millis = 5000 WHERE kind = 'relationship-added'", []).unwrap();
        assert_eq!(
            scoped.record_dates(first.id).unwrap(),
            RecordDates {
                opened_at_millis: Some(1000),
                updated_at_millis: Some(4000)
            }
        );
        assert_eq!(
            scoped.record_dates(second.id).unwrap(),
            RecordDates {
                opened_at_millis: Some(1000),
                updated_at_millis: Some(5000)
            }
        );
        assert_eq!(
            scoped.record_dates(third.id).unwrap().updated_at_millis,
            Some(5000)
        );
        let app = Application::new(scoped, Box::new(Inspector));
        let inventory = app.recent_records("a").unwrap();
        assert_eq!(
            inventory
                .iter()
                .map(|item| item.record.id)
                .collect::<Vec<_>>(),
            [second.id, third.id, first.id]
        );
        assert_eq!(inventory[2].dates.opened_at_millis, Some(1000));
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM record_updates WHERE record_id = ?1 AND kind = 'created'",
                [first.id.cast_signed()],
            )
            .unwrap();
        assert_eq!(
            store.record_dates(first.id).unwrap(),
            RecordDates {
                opened_at_millis: None,
                updated_at_millis: Some(4000)
            }
        );
    }

    #[test]
    fn missing_historical_dates_and_projection_failures_are_not_fabricated() {
        let store = SqliteProjectRepository::in_memory().unwrap();
        store.create_workspace("a", "A", Path::new("/")).unwrap();
        let scoped = store.records_in_workspace("a").unwrap();
        let record = scoped
            .open_record(&NewRecord {
                body: "body".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM record_updates", [])
            .unwrap();
        assert_eq!(
            scoped.record_dates(record.id).unwrap(),
            RecordDates::default()
        );
        assert!(scoped.record_dates(999).is_err());
        store
            .connection
            .lock()
            .unwrap()
            .execute("DROP TABLE record_updates", [])
            .unwrap();
        assert!(scoped.record_dates(record.id).is_err());
        let app = Application::new(scoped, Box::new(Inspector));
        assert!(app.recent_records("a").is_err());
    }
}
