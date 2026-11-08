//! Record readiness is workflow state, not provider or dispatch configuration readiness.
use crate::{
    Application, Record, RecordRelationshipKind, RecordStatus, RecordType, RegistrationError,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordReadiness {
    pub record: Record,
    pub blocked_by: Vec<u64>,
}
impl Application {
    /// Lists the selected Workspace's executable frontier, optionally including planning,
    /// human-in-the-loop work, or blocked candidates. No dispatch configuration is required.
    ///
    /// # Errors
    /// Returns a storage or relationship lookup error.
    pub fn ready_records(
        &self,
        planning: bool,
        include_hitl: bool,
        blocked: bool,
    ) -> Result<Vec<RecordReadiness>, RegistrationError> {
        let records = self.list_records(None, Some(RecordStatus::Open), None, None)?;
        let mut frontier = Vec::new();
        for record in records {
            let included = match record.record_type {
                RecordType::Task => true,
                RecordType::Grilling => include_hitl,
                RecordType::Spec | RecordType::Wayfinder | RecordType::Initiative => planning,
                RecordType::Generic => false,
            };
            if !included {
                continue;
            }
            let mut blocked_by = Vec::new();
            for edge in &record.relationships {
                if edge.kind == RecordRelationshipKind::DependsOn
                    && edge.from_id == record.id
                    && self.show_record(edge.to_id)?.status != RecordStatus::Done
                {
                    blocked_by.push(edge.to_id);
                }
            }
            blocked_by.sort_unstable();
            if blocked || blocked_by.is_empty() {
                frontier.push(RecordReadiness { record, blocked_by });
            }
        }
        Ok(frontier)
    }
}
