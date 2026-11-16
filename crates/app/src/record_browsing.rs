//! Read-only Workspace inventory and durable Record Update date projections.
use crate::{Application, Record, RegistrationError};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RecordDates {
    /// Opening Update timestamp, absent for historical Records without one.
    pub opened_at_millis: Option<u64>,
    /// Greatest Update timestamp, including Comments and Relationships.
    pub updated_at_millis: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatedRecord {
    pub record: Record,
    pub dates: RecordDates,
}

impl Application {
    /// All Records in exactly one Workspace, most recently updated first.
    /// Unknown dates sort last; equal dates use ascending Record Identifier.
    /// # Errors
    /// Returns Workspace selection, inventory, or date-projection failures.
    pub fn recent_records(&self, workspace: &str) -> Result<Vec<DatedRecord>, RegistrationError> {
        let scoped = self.records_in_workspace(workspace)?;
        let mut records = scoped
            .list_records(None, None, None)?
            .into_iter()
            .map(|record| {
                let dates = scoped.records.record_dates(record.id)?;
                Ok(DatedRecord { record, dates })
            })
            .collect::<Result<Vec<_>, RegistrationError>>()?;
        records.sort_by(|a, b| {
            b.dates
                .updated_at_millis
                .cmp(&a.dates.updated_at_millis)
                .then_with(|| a.record.id.cmp(&b.record.id))
        });
        Ok(records)
    }
}
