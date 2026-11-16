//! Read-only Workspace inventory and durable Record Update date projections.
use crate::{Application, Record, RecordStatus, RecordType, RegistrationError};
use std::cmp::Ordering;

pub const PAGE_SIZE: usize = 50;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RecordSort {
    #[default]
    RecentlyUpdated,
    LeastRecentlyUpdated,
    NewestCreated,
    OldestCreated,
}

impl RecordSort {
    #[must_use]
    pub fn is_creation(self) -> bool {
        matches!(self, Self::NewestCreated | Self::OldestCreated)
    }

    #[must_use]
    pub fn date(self, dates: RecordDates) -> Option<u64> {
        if self.is_creation() {
            dates.opened_at_millis
        } else {
            dates.updated_at_millis
        }
    }

    fn compare(self, a: &DatedRecord, b: &DatedRecord) -> Ordering {
        let order = match (self.date(a.dates), self.date(b.dates)) {
            (Some(a), Some(b)) => {
                if matches!(self, Self::RecentlyUpdated | Self::NewestCreated) {
                    b.cmp(&a)
                } else {
                    a.cmp(&b)
                }
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        order.then_with(|| a.record.id.cmp(&b.record.id))
    }
}

#[derive(Clone, Debug, Default)]
pub struct RecordFilters {
    pub types: Vec<RecordType>,
    pub statuses: Vec<RecordStatus>,
}

#[derive(Debug)]
pub struct RecordPage {
    pub records: Vec<DatedRecord>,
    pub total: usize,
    pub page: usize,
    /// Empty results still have a valid first page.
    pub pages: usize,
}

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
        let mut records = self.dated_records(workspace)?;
        records.sort_by(|a, b| RecordSort::RecentlyUpdated.compare(a, b));
        Ok(records)
    }

    /// Filter and sort the complete Workspace inventory before selecting a page.
    /// Unknown dates sort last in every order; identifier ties always ascend.
    /// # Errors
    /// Returns invalid-page errors (including out-of-range pages) or read failures.
    pub fn browse_records(
        &self,
        workspace: &str,
        filters: &RecordFilters,
        sort: RecordSort,
        page: usize,
    ) -> Result<RecordPage, RegistrationError> {
        if page == 0 {
            return Err(RegistrationError::new(
                "invalid-page",
                "Page must be positive",
            ));
        }
        let mut records = self.dated_records(workspace)?;
        records.retain(|item| {
            (filters.types.is_empty() || filters.types.contains(&item.record.record_type))
                && (filters.statuses.is_empty() || filters.statuses.contains(&item.record.status))
        });
        records.sort_by(|a, b| sort.compare(a, b));
        let total = records.len();
        let pages = total.div_ceil(PAGE_SIZE).max(1);
        if page > pages {
            return Err(RegistrationError::new(
                "invalid-page",
                "Page exceeds matching results; apply filters to return to page one",
            ));
        }
        Ok(RecordPage {
            records: records
                .into_iter()
                .skip((page - 1) * PAGE_SIZE)
                .take(PAGE_SIZE)
                .collect(),
            total,
            page,
            pages,
        })
    }

    fn dated_records(&self, workspace: &str) -> Result<Vec<DatedRecord>, RegistrationError> {
        let scoped = self.records_in_workspace(workspace)?;
        scoped
            .list_records(None, None, None)?
            .into_iter()
            .map(|record| {
                let dates = scoped.records.record_dates(record.id)?;
                Ok(DatedRecord { record, dates })
            })
            .collect()
    }
}
