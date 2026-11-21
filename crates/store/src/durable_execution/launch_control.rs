//! The single sticky durable-launch rollback. No process, runner or provider calls.
use crate::{SqliteProjectRepository, now_millis, storage_error};
use forge_app::{
    RegistrationError,
    durable_execution::launch_control::{DisableRequest, Repository, Rollback},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

fn read(c: &Connection) -> Result<Option<Rollback>, RegistrationError> {
    let row: Option<(String, String, i64)> = c
        .query_row(
            "SELECT operation_id, reason, disabled_at_millis FROM durable_launch_rollbacks WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    row.map(|(operation_id, reason, at)| {
        Ok(Rollback {
            operation_id,
            reason,
            disabled_at_millis: u64::try_from(at).map_err(|_| {
                RegistrationError::new(
                    "durable-storage-invalid",
                    "durable launch rollback time is invalid",
                )
            })?,
        })
    })
    .transpose()
}

impl Repository for SqliteProjectRepository {
    fn disable_durable_launches(
        &self,
        request: &DisableRequest,
    ) -> Result<(Rollback, bool), RegistrationError> {
        request.validate()?;
        let mut c = self.connection.lock().map_err(|_| {
            RegistrationError::new("storage", "repository connection lock is poisoned")
        })?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        if let Some(existing) = read(&tx)? {
            tx.commit().map_err(storage_error)?;
            return Ok((existing, true));
        }
        tx.execute(
            "INSERT INTO durable_launch_rollbacks(id, operation_id, reason, disabled_at_millis) VALUES (1, ?1, ?2, ?3)",
            params![request.operation_id, request.reason, now_millis()?],
        )
        .map_err(storage_error)?;
        let rollback = read(&tx)?.ok_or_else(|| {
            RegistrationError::new("durable-storage-invalid", "rollback was not retained")
        })?;
        tx.commit().map_err(storage_error)?;
        Ok((rollback, false))
    }

    fn durable_launch_rollback(&self) -> Result<Option<Rollback>, RegistrationError> {
        let c = self.connection.lock().map_err(|_| {
            RegistrationError::new("storage", "repository connection lock is poisoned")
        })?;
        read(&c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(operation_id: &str) -> DisableRequest {
        DisableRequest {
            operation_id: operation_id.into(),
            reason: "incident".into(),
        }
    }

    #[test]
    fn rollback_is_sticky_idempotent_immutable_and_survives_reopen() {
        let path = std::env::temp_dir().join(format!(
            "forge-launch-rollback-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteProjectRepository::open(&path).unwrap();
        assert!(store.durable_launch_rollback().unwrap().is_none());
        let (first, replayed) = store.disable_durable_launches(&request("op-1")).unwrap();
        assert!(!replayed);
        let (again, replayed) = store.disable_durable_launches(&request("op-1")).unwrap();
        assert!(replayed);
        assert_eq!(again, first);
        // A later, different request never replaces the retained rollback.
        let (other, replayed) = store.disable_durable_launches(&request("op-2")).unwrap();
        assert!(replayed);
        assert_eq!(other, first);
        {
            let c = store.connection.lock().unwrap();
            assert!(
                c.execute("DELETE FROM durable_launch_rollbacks", [])
                    .is_err()
            );
            assert!(
                c.execute("UPDATE durable_launch_rollbacks SET reason = 'x'", [])
                    .is_err()
            );
            assert!(
                c.execute(
                    "INSERT INTO durable_launch_rollbacks(id, operation_id, reason, disabled_at_millis) VALUES (2, 'x', 'y', 1)",
                    [],
                )
                .is_err()
            );
        }
        drop(store);
        let reopened = SqliteProjectRepository::open(&path).unwrap();
        assert_eq!(reopened.durable_launch_rollback().unwrap(), Some(first));
        assert!(
            reopened
                .disable_durable_launches(&DisableRequest {
                    operation_id: "bad id".into(),
                    reason: "r".into(),
                })
                .is_err()
        );
        drop(reopened);
        let _ = std::fs::remove_file(&path);
    }
}
