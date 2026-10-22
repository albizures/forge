//! Application-owned operations and persistence contracts.

use std::path::{Path, PathBuf};

/// A Project registered with Forge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    /// Immutable human-referenceable identity.
    pub slug: String,
    /// Canonical path of the Repository's main Worktree.
    pub registered_folder: PathBuf,
    /// URL configured for the Repository's `origin` remote.
    pub origin_url: String,
}

/// The initial lifecycle status of a Record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordStatus {
    Open,
    InProgress,
    Done,
}

/// The currently supported kind of typed Record Relationship.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordRelationshipKind {
    ParentChild,
}

/// An endpoint-neutral typed Relationship between two Records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordRelationship {
    pub kind: RecordRelationshipKind,
    pub parent_id: u64,
    pub child_id: u64,
}

/// The immutable kind selected when a Record opens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordType {
    Generic,
    Spec,
    Task,
}

/// A unit of work whose immutable type governs its scope and hierarchy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Opaque, positive identity assigned by persistence.
    pub id: u64,
    /// Immutable type selected at opening.
    pub record_type: RecordType,
    /// Required Markdown content, separate from its structured metadata.
    pub body: String,
    /// Immutable, lexically ordered Project Scope selected at opening.
    pub project_slugs: Vec<String>,
    /// The Record's lifecycle status.
    pub status: RecordStatus,
    /// Immutable optional explanation recorded when the Record was completed.
    pub completion_resolution: Option<String>,
    /// All Relationships incident to this Record.
    pub relationships: Vec<RecordRelationship>,
}

/// A Record before persistence assigns its identity and initial status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewRecord {
    pub body: String,
    pub project_slugs: Vec<String>,
}

/// A non-generic Record before persistence assigns its identity and initial status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewTypedRecord {
    pub record_type: RecordType,
    pub body: String,
    pub project_slugs: Vec<String>,
}

/// Optional discovery constraints for Records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordListFilter {
    pub project_slug: Option<String>,
    pub status: Option<RecordStatus>,
}

/// Opaque stable identity assigned to a Forge-authored Record Update.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RecordUpdateId(pub u64);

/// The minimal fact recorded for a successful Record mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordUpdate {
    pub id: RecordUpdateId,
    /// Unix milliseconds when Forge committed this update.
    pub created_at_millis: u64,
    pub kind: RecordUpdateKind,
}

/// Opaque stable identity assigned to a Record Comment.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RecordCommentId(pub u64);

/// Immutable free-text discussion attached to a Record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordComment {
    pub id: RecordCommentId,
    pub created_at_millis: u64,
    pub author: Option<String>,
    pub body: String,
}

/// The change represented by a Record Update. Deliberately excludes content snapshots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordUpdateKind {
    Created,
    BodyEdited,
    StatusTransition {
        from: RecordStatus,
        to: RecordStatus,
    },
    RelationshipAdded {
        related_record_id: u64,
    },
    RelationshipRemoved {
        related_record_id: u64,
    },
    CommentAdded {
        comment_id: RecordCommentId,
    },
    CommentDeleted {
        comment_id: RecordCommentId,
    },
}

/// A domain error which can safely be shown to a user.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistrationError {
    /// Stable, transport-friendly category.
    pub code: &'static str,
    /// Actionable explanation.
    pub message: String,
}

impl RegistrationError {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Inspects a folder without coupling application policy to a Git implementation.
pub trait RepositoryInspector: Send + Sync {
    /// Returns the eligible main-worktree registration facts for `folder`.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the folder is not eligible.
    fn inspect(&self, folder: &Path) -> Result<Project, RegistrationError>;
}

/// Persistence boundary owned by the application.
pub trait ProjectRepository: Send + Sync {
    /// Stores `project`, or returns the matching existing Project idempotently.
    ///
    /// # Errors
    ///
    /// Returns a user-facing validation or storage failure.
    fn register(&self, project: &Project) -> Result<Project, RegistrationError>;
    /// Finds the Project whose Registered Folder contains `folder`.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when no Project contains the folder.
    fn find_containing(&self, folder: &Path) -> Result<Project, RegistrationError>;
}

/// Persistence operations for generic Records.
pub trait RecordRepository: ProjectRepository {
    /// Opens a Record after validating that all selected Projects exist.
    ///
    /// # Errors
    ///
    /// Returns a user-facing validation or storage failure.
    fn open_record(&self, record: &NewRecord) -> Result<Record, RegistrationError>;
    /// Opens a non-generic Record after validating its type-specific invariants.
    ///
    /// # Errors
    ///
    /// Returns a user-facing validation or storage failure.
    fn open_typed_record(&self, record: &NewTypedRecord) -> Result<Record, RegistrationError>;
    /// Opens a Task beneath a Spec, assigning the Spec's scope and membership atomically.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the parent is not a Spec or storage fails.
    fn open_task_under_spec(&self, body: &str, spec_id: u64) -> Result<Record, RegistrationError>;
    /// Lists Records matching Project Scope membership and lifecycle status.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when storage fails.
    fn list_records(&self, filter: &RecordListFilter) -> Result<Vec<Record>, RegistrationError>;
    /// Returns the Record identified by `id`.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the Record is absent or storage fails.
    fn show_record(&self, id: u64) -> Result<Record, RegistrationError>;
    /// Persists a non-Done Record Body.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the Record is Done, absent, or storage fails.
    fn edit_record(&self, id: u64, body: &str) -> Result<Record, RegistrationError>;
    /// Advances a Record from Open to In Progress.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for an invalid transition, absent Record, or storage failure.
    fn start_record(&self, id: u64) -> Result<Record, RegistrationError>;
    /// Advances a Record from In Progress to Done.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for an invalid transition, absent Record, or storage failure.
    fn done_record(
        &self,
        id: u64,
        completion_resolution: Option<&str>,
    ) -> Result<Record, RegistrationError>;
    /// Links `child_id` beneath `parent_id` as a parent/child Relationship.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for invalid hierarchy changes.
    fn relate_records(&self, child_id: u64, parent_id: u64) -> Result<Record, RegistrationError>;
    /// Removes the parent/child Relationship from `child_id` to `parent_id`.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the requested Relationship is absent.
    fn unrelate_records(&self, child_id: u64, parent_id: u64) -> Result<Record, RegistrationError>;
    /// Lists the Record's updates newest first, using identity to break timestamp ties.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error if the Record is absent or storage fails.
    fn list_record_updates(&self, id: u64) -> Result<Vec<RecordUpdate>, RegistrationError>;
    /// Adds an immutable Comment, including to a Done Record.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the Record is absent or storage fails.
    fn add_record_comment(
        &self,
        id: u64,
        body: &str,
        author: Option<&str>,
    ) -> Result<RecordComment, RegistrationError>;
    /// Lists current (not deleted) Comments newest first.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the Record is absent or storage fails.
    fn list_record_comments(&self, id: u64) -> Result<Vec<RecordComment>, RegistrationError>;
    /// Deletes a Comment without retaining its content.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the Comment is absent or storage fails.
    fn delete_record_comment(
        &self,
        id: u64,
        comment_id: RecordCommentId,
    ) -> Result<(), RegistrationError>;
}

/// The application boundary for operations shared by all transports.
pub struct Application {
    records: Box<dyn RecordRepository>,
    inspector: Box<dyn RepositoryInspector>,
}

impl Application {
    /// Creates an application from its infrastructure boundaries.
    #[must_use]
    pub fn new(
        records: Box<dyn RecordRepository>,
        inspector: Box<dyn RepositoryInspector>,
    ) -> Self {
        Self { records, inspector }
    }

    /// Registers an eligible Git main Worktree as a Project.
    ///
    /// # Errors
    ///
    /// Returns the validation or persistence failure suitable for the calling adapter.
    pub fn register_project(
        &self,
        folder: &Path,
        slug: &str,
    ) -> Result<Project, RegistrationError> {
        if slug.is_empty() {
            return Err(RegistrationError::new(
                "invalid-slug",
                "project slug must not be empty",
            ));
        }
        let mut project = self.inspector.inspect(folder)?;
        slug.clone_into(&mut project.slug);
        self.records.register(&project)
    }

    /// Finds the Project containing the supplied current folder.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the folder is outside every Project.
    pub fn project_here(&self, folder: &Path) -> Result<Project, RegistrationError> {
        let folder = std::fs::canonicalize(folder).map_err(|_| {
            RegistrationError::new(
                "invalid-folder",
                format!("`{}` is not an existing folder", folder.display()),
            )
        })?;
        self.records.find_containing(&folder)
    }

    /// Opens a generic Record with a required body and explicit Project Scope.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for blank bodies, duplicate or unknown Projects, or storage failures.
    pub fn open_record(
        &self,
        body: &str,
        project_slugs: Vec<String>,
    ) -> Result<Record, RegistrationError> {
        if body.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-body",
                "record body must not be blank",
            ));
        }
        let mut project_slugs = project_slugs;
        project_slugs.sort();
        if project_slugs.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(RegistrationError::new(
                "duplicate-project",
                "record Project Scope must not contain duplicate Project slugs",
            ));
        }
        self.records.open_record(&NewRecord {
            body: body.to_owned(),
            project_slugs,
        })
    }

    /// Opens a Spec for exactly one Project.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for a blank body or storage failure.
    pub fn open_spec(&self, body: &str, project_slug: String) -> Result<Record, RegistrationError> {
        self.open_typed_record(RecordType::Spec, body, vec![project_slug])
    }

    /// Opens a standalone Task with zero or one Project Scope.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for a blank body or storage failure.
    pub fn open_task(
        &self,
        body: &str,
        project_slug: Option<String>,
    ) -> Result<Record, RegistrationError> {
        self.open_typed_record(RecordType::Task, body, project_slug.into_iter().collect())
    }

    /// Opens a Task under `spec_id`, inheriting its scope and membership atomically.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for a blank body, invalid parent, or storage failure.
    pub fn open_task_under_spec(
        &self,
        body: &str,
        spec_id: u64,
    ) -> Result<Record, RegistrationError> {
        self.require_body(body)?;
        self.records.open_task_under_spec(body, spec_id)
    }

    /// Lists Records matching the supplied optional discovery filters.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when storage fails.
    pub fn list_records(
        &self,
        project_slug: Option<String>,
        status: Option<RecordStatus>,
    ) -> Result<Vec<Record>, RegistrationError> {
        self.records.list_records(&RecordListFilter {
            project_slug,
            status,
        })
    }

    /// Returns a Record by its opaque identity.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error if the Record is absent.
    pub fn show_record(&self, id: u64) -> Result<Record, RegistrationError> {
        self.records.show_record(id)
    }

    /// Edits a Record Body before it is Done.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for blank Bodies or completed Records.
    pub fn edit_record(&self, id: u64, body: &str) -> Result<Record, RegistrationError> {
        if body.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-body",
                "record body must not be blank",
            ));
        }
        self.records.edit_record(id, body)
    }

    /// Advances a Record from Open to In Progress.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the transition is invalid.
    pub fn start_record(&self, id: u64) -> Result<Record, RegistrationError> {
        self.records.start_record(id)
    }

    /// Completes an in-progress Record with an optional Resolution.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for an invalid transition or blank Resolution.
    pub fn done_record(
        &self,
        id: u64,
        completion_resolution: Option<&str>,
    ) -> Result<Record, RegistrationError> {
        if completion_resolution.is_some_and(|resolution| resolution.trim().is_empty()) {
            return Err(RegistrationError::new(
                "invalid-resolution",
                "completion resolution must not be blank",
            ));
        }
        self.records.done_record(id, completion_resolution)
    }

    /// Links a child Record to a parent Record.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error for invalid hierarchy changes.
    pub fn relate_records(
        &self,
        child_id: u64,
        parent_id: u64,
    ) -> Result<Record, RegistrationError> {
        let child = self.records.show_record(child_id)?;
        let parent = self.records.show_record(parent_id)?;
        if child.record_type != RecordType::Generic || parent.record_type != RecordType::Generic {
            return Err(RegistrationError::new(
                "invalid-relationship",
                "only Generic Records may be related after opening",
            ));
        }
        self.records.relate_records(child_id, parent_id)
    }

    /// Removes a child Record from a parent Record.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error when the requested Relationship is absent.
    pub fn unrelate_records(
        &self,
        child_id: u64,
        parent_id: u64,
    ) -> Result<Record, RegistrationError> {
        let child = self.records.show_record(child_id)?;
        let parent = self.records.show_record(parent_id)?;
        if child.record_type != RecordType::Generic || parent.record_type != RecordType::Generic {
            return Err(RegistrationError::new(
                "invalid-relationship",
                "only Generic Record Relationships may be removed",
            ));
        }
        self.records.unrelate_records(child_id, parent_id)
    }

    fn require_body(&self, body: &str) -> Result<(), RegistrationError> {
        if body.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-body",
                "record body must not be blank",
            ));
        }
        Ok(())
    }

    fn open_typed_record(
        &self,
        record_type: RecordType,
        body: &str,
        project_slugs: Vec<String>,
    ) -> Result<Record, RegistrationError> {
        self.require_body(body)?;
        self.records.open_typed_record(&NewTypedRecord {
            record_type,
            body: body.to_owned(),
            project_slugs,
        })
    }

    /// Lists a Record's Forge-authored Update Log newest first.
    ///
    /// # Errors
    ///
    /// Returns a user-facing error if the Record is absent or storage fails.
    pub fn list_record_updates(&self, id: u64) -> Result<Vec<RecordUpdate>, RegistrationError> {
        self.records.list_record_updates(id)
    }

    /// Adds a non-blank immutable Comment to a Record.
    ///
    /// # Errors
    ///
    /// Returns a validation, Record lookup, or storage error.
    pub fn add_record_comment(
        &self,
        id: u64,
        body: &str,
        author: Option<&str>,
    ) -> Result<RecordComment, RegistrationError> {
        if body.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-comment",
                "comment must not be blank",
            ));
        }
        if author.is_some_and(|author| author.trim().is_empty()) {
            return Err(RegistrationError::new(
                "invalid-comment-author",
                "comment author must not be blank",
            ));
        }
        self.records.add_record_comment(id, body, author)
    }

    /// Lists current Comments newest first.
    ///
    /// # Errors
    ///
    /// Returns a Record lookup or storage error.
    pub fn list_record_comments(&self, id: u64) -> Result<Vec<RecordComment>, RegistrationError> {
        self.records.list_record_comments(id)
    }

    /// Deletes a Comment from a Record.
    ///
    /// # Errors
    ///
    /// Returns a Record/Comment lookup or storage error.
    pub fn delete_record_comment(&self, id: u64, comment_id: u64) -> Result<(), RegistrationError> {
        self.records
            .delete_record_comment(id, RecordCommentId(comment_id))
    }

    /// Returns readiness information for this application instance.
    #[must_use]
    pub fn status(&self) -> ApplicationStatus {
        ApplicationStatus {
            name: "forge",
            version: env!("CARGO_PKG_VERSION"),
            ready: true,
        }
    }
}

/// Application-owned readiness information.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationStatus {
    pub name: &'static str,
    pub version: &'static str,
    pub ready: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Repository {
        records: Mutex<Vec<Record>>,
        comments: Mutex<Vec<RecordComment>>,
    }
    impl ProjectRepository for Repository {
        fn register(&self, project: &Project) -> Result<Project, RegistrationError> {
            Ok(project.clone())
        }
        fn find_containing(&self, _: &Path) -> Result<Project, RegistrationError> {
            unreachable!()
        }
    }
    impl RecordRepository for Repository {
        fn open_record(&self, record: &NewRecord) -> Result<Record, RegistrationError> {
            let mut records = self.records.lock().unwrap();
            let record = Record {
                id: (records.len() + 1) as u64,
                record_type: RecordType::Generic,
                body: record.body.clone(),
                project_slugs: record.project_slugs.clone(),
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![],
            };
            records.push(record.clone());
            Ok(record)
        }
        fn open_typed_record(&self, record: &NewTypedRecord) -> Result<Record, RegistrationError> {
            let mut records = self.records.lock().unwrap();
            let record = Record {
                id: (records.len() + 1) as u64,
                record_type: record.record_type,
                body: record.body.clone(),
                project_slugs: record.project_slugs.clone(),
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![],
            };
            records.push(record.clone());
            Ok(record)
        }
        fn open_task_under_spec(
            &self,
            body: &str,
            spec_id: u64,
        ) -> Result<Record, RegistrationError> {
            let spec = self.show_record(spec_id)?;
            if spec.record_type != RecordType::Spec {
                return Err(RegistrationError::new(
                    "invalid-parent-type",
                    "a Task may be opened under a Spec only",
                ));
            }
            let mut records = self.records.lock().unwrap();
            let task_id = (records.len() + 1) as u64;
            let relationship = RecordRelationship {
                kind: RecordRelationshipKind::ParentChild,
                parent_id: spec_id,
                child_id: task_id,
            };
            let task = Record {
                id: task_id,
                record_type: RecordType::Task,
                body: body.to_owned(),
                project_slugs: spec.project_slugs,
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![relationship.clone()],
            };
            let parent = records
                .iter_mut()
                .find(|record| record.id == spec_id)
                .expect("shown Spec must remain present while the repository lock is held");
            parent.relationships.push(relationship);
            records.push(task.clone());
            Ok(task)
        }
        fn list_records(
            &self,
            filter: &RecordListFilter,
        ) -> Result<Vec<Record>, RegistrationError> {
            Ok(self
                .records
                .lock()
                .unwrap()
                .iter()
                .filter(|record| {
                    filter
                        .project_slug
                        .as_ref()
                        .is_none_or(|slug| record.project_slugs.contains(slug))
                        && filter.status.is_none_or(|status| record.status == status)
                })
                .cloned()
                .rev()
                .collect())
        }
        fn show_record(&self, id: u64) -> Result<Record, RegistrationError> {
            self.records
                .lock()
                .unwrap()
                .iter()
                .find(|record| record.id == id)
                .cloned()
                .ok_or_else(|| RegistrationError::new("record-not-found", "Record does not exist"))
        }
        fn edit_record(&self, id: u64, body: &str) -> Result<Record, RegistrationError> {
            let mut records = self.records.lock().unwrap();
            let record = records
                .iter_mut()
                .find(|record| record.id == id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            if record.status == RecordStatus::Done {
                return Err(RegistrationError::new(
                    "invalid-status-transition",
                    "Record is Done",
                ));
            }
            record.body = body.into();
            Ok(record.clone())
        }
        fn start_record(&self, id: u64) -> Result<Record, RegistrationError> {
            let mut records = self.records.lock().unwrap();
            let record = records
                .iter_mut()
                .find(|record| record.id == id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            if record.status != RecordStatus::Open {
                return Err(RegistrationError::new(
                    "invalid-status-transition",
                    "invalid transition",
                ));
            }
            record.status = RecordStatus::InProgress;
            Ok(record.clone())
        }
        fn done_record(
            &self,
            id: u64,
            completion_resolution: Option<&str>,
        ) -> Result<Record, RegistrationError> {
            let mut records = self.records.lock().unwrap();
            let index = records
                .iter()
                .position(|record| record.id == id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            if records[index].status != RecordStatus::InProgress {
                return Err(RegistrationError::new(
                    "invalid-status-transition",
                    "invalid transition",
                ));
            }
            if records[index].relationships.iter().any(|relationship| {
                relationship.parent_id == id
                    && records
                        .iter()
                        .find(|record| record.id == relationship.child_id)
                        .is_some_and(|child| child.status != RecordStatus::Done)
            }) {
                return Err(RegistrationError::new(
                    "children-not-done",
                    "all child Records must be Done before completing a parent",
                ));
            }
            records[index].status = RecordStatus::Done;
            records[index].completion_resolution = completion_resolution.map(str::to_owned);
            Ok(records[index].clone())
        }
        fn relate_records(
            &self,
            child_id: u64,
            parent_id: u64,
        ) -> Result<Record, RegistrationError> {
            if child_id == parent_id {
                return Err(RegistrationError::new(
                    "invalid-relationship",
                    "a Record cannot be its own parent",
                ));
            }
            let mut records = self.records.lock().unwrap();
            let child = records
                .iter()
                .position(|record| record.id == child_id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            let parent = records
                .iter()
                .position(|record| record.id == parent_id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            if records[child].project_slugs != records[parent].project_slugs {
                return Err(RegistrationError::new(
                    "scope-mismatch",
                    "parent and child Records must have identical Project Scopes",
                ));
            }
            if records.iter().any(|record| {
                record
                    .relationships
                    .iter()
                    .any(|relationship| relationship.child_id == child_id)
            }) {
                return Err(RegistrationError::new(
                    "child-already-related",
                    "a child Record can have only one parent",
                ));
            }
            let mut pending = vec![child_id];
            while let Some(current) = pending.pop() {
                if current == parent_id {
                    return Err(RegistrationError::new(
                        "relationship-cycle",
                        "parent/child Relationships cannot form a cycle",
                    ));
                }
                pending.extend(
                    records
                        .iter()
                        .flat_map(|record| record.relationships.iter())
                        .filter(|relationship| relationship.parent_id == current)
                        .map(|relationship| relationship.child_id),
                );
            }
            let relationship = RecordRelationship {
                kind: RecordRelationshipKind::ParentChild,
                parent_id,
                child_id,
            };
            records[child].relationships.push(relationship.clone());
            records[parent].relationships.push(relationship);
            Ok(records[child].clone())
        }
        fn unrelate_records(
            &self,
            child_id: u64,
            parent_id: u64,
        ) -> Result<Record, RegistrationError> {
            let mut records = self.records.lock().unwrap();
            let child = records
                .iter()
                .position(|record| record.id == child_id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            let parent = records
                .iter()
                .position(|record| record.id == parent_id)
                .ok_or_else(|| {
                    RegistrationError::new("record-not-found", "Record does not exist")
                })?;
            let exists = records[child].relationships.iter().any(|relationship| {
                relationship.parent_id == parent_id && relationship.child_id == child_id
            });
            if !exists {
                return Err(RegistrationError::new(
                    "relationship-not-found",
                    "parent/child Relationship does not exist",
                ));
            }
            for index in [child, parent] {
                records[index].relationships.retain(|relationship| {
                    relationship.parent_id != parent_id || relationship.child_id != child_id
                });
            }
            Ok(records[child].clone())
        }
        fn list_record_updates(&self, id: u64) -> Result<Vec<RecordUpdate>, RegistrationError> {
            self.show_record(id)?;
            Ok(vec![])
        }
        fn add_record_comment(
            &self,
            id: u64,
            body: &str,
            author: Option<&str>,
        ) -> Result<RecordComment, RegistrationError> {
            self.show_record(id)?;
            let mut comments = self.comments.lock().unwrap();
            let comment = RecordComment {
                id: RecordCommentId((comments.len() + 1) as u64),
                created_at_millis: 0,
                author: author.map(str::to_owned),
                body: body.to_owned(),
            };
            comments.push(comment.clone());
            Ok(comment)
        }
        fn list_record_comments(&self, id: u64) -> Result<Vec<RecordComment>, RegistrationError> {
            self.show_record(id)?;
            Ok(self
                .comments
                .lock()
                .unwrap()
                .iter()
                .cloned()
                .rev()
                .collect())
        }
        fn delete_record_comment(
            &self,
            id: u64,
            comment_id: RecordCommentId,
        ) -> Result<(), RegistrationError> {
            self.show_record(id)?;
            let mut comments = self.comments.lock().unwrap();
            let before = comments.len();
            comments.retain(|comment| comment.id != comment_id);
            if comments.len() == before {
                return Err(RegistrationError::new(
                    "comment-not-found",
                    "Comment does not exist",
                ));
            }
            Ok(())
        }
    }
    struct Inspector;
    impl RepositoryInspector for Inspector {
        fn inspect(&self, _: &Path) -> Result<Project, RegistrationError> {
            unreachable!()
        }
    }
    fn application() -> Application {
        Application::new(
            Box::new(Repository {
                records: Mutex::new(vec![]),
                comments: Mutex::new(vec![]),
            }),
            Box::new(Inspector),
        )
    }

    #[test]
    fn opening_requires_a_non_blank_body_and_a_distinct_scope() {
        assert_eq!(
            application().open_record(" \n", vec![]).unwrap_err().code,
            "invalid-body"
        );
        assert_eq!(
            application()
                .open_record("body", vec!["alpha".into(), "alpha".into()])
                .unwrap_err()
                .code,
            "duplicate-project"
        );
    }

    #[test]
    fn typed_records_have_immutable_opening_rules_and_hierarchy() {
        let app = application();
        let spec = app.open_spec("spec", "alpha".into()).unwrap();
        assert_eq!(spec.record_type, RecordType::Spec);
        assert_eq!(spec.project_slugs, ["alpha"]);

        let standalone = app.open_task("standalone", None).unwrap();
        assert_eq!(standalone.record_type, RecordType::Task);
        assert!(standalone.project_slugs.is_empty());
        let task = app.open_task_under_spec("child", spec.id).unwrap();
        assert_eq!(task.record_type, RecordType::Task);
        assert_eq!(task.project_slugs, ["alpha"]);
        assert_eq!(task.relationships[0].parent_id, spec.id);
        assert_eq!(
            app.open_task_under_spec("invalid", standalone.id)
                .unwrap_err()
                .code,
            "invalid-parent-type"
        );
        assert_eq!(
            app.relate_records(task.id, spec.id).unwrap_err().code,
            "invalid-relationship"
        );
        assert_eq!(
            app.unrelate_records(task.id, spec.id).unwrap_err().code,
            "invalid-relationship"
        );

        app.start_record(spec.id).unwrap();
        assert_eq!(
            app.done_record(spec.id, None).unwrap_err().code,
            "children-not-done"
        );
    }

    #[test]
    fn lists_records_by_scope_membership_and_status_newest_first() {
        let app = application();
        let alpha = app.open_record("alpha", vec!["alpha".into()]).unwrap();
        let beta = app
            .open_record("both", vec!["alpha".into(), "beta".into()])
            .unwrap();
        let unscoped = app.open_record("unscoped", vec![]).unwrap();
        app.start_record(beta.id).unwrap();

        assert_eq!(
            app.list_records(Some("alpha".into()), None)
                .unwrap()
                .into_iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec![beta.id, alpha.id]
        );
        assert_eq!(
            app.list_records(None, Some(RecordStatus::Open))
                .unwrap()
                .into_iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec![unscoped.id, alpha.id]
        );
    }

    #[test]
    fn record_content_and_lifecycle_advance_once_and_completion_is_immutable() {
        let app = application();
        let record = app.open_record("original", vec![]).unwrap();
        assert_eq!(
            app.edit_record(record.id, "revised").unwrap().body,
            "revised"
        );
        assert_eq!(
            app.done_record(record.id, None).unwrap_err().code,
            "invalid-status-transition"
        );
        assert_eq!(
            app.start_record(record.id).unwrap().status,
            RecordStatus::InProgress
        );
        let done = app.done_record(record.id, Some("completed")).unwrap();
        assert_eq!(done.status, RecordStatus::Done);
        assert_eq!(done.completion_resolution.as_deref(), Some("completed"));
        assert_eq!(
            app.edit_record(record.id, "later").unwrap_err().code,
            "invalid-status-transition"
        );
        assert_eq!(
            app.done_record(record.id, Some("other")).unwrap_err().code,
            "invalid-status-transition"
        );
        assert_eq!(
            app.done_record(record.id, Some(" ")).unwrap_err().code,
            "invalid-resolution"
        );
    }

    #[test]
    fn relationships_are_nested_and_gate_parent_completion() {
        let app = application();
        let parent = app.open_record("parent", vec![]).unwrap();
        let child = app.open_record("child", vec![]).unwrap();
        let grandchild = app.open_record("grandchild", vec![]).unwrap();
        assert_eq!(
            app.relate_records(child.id, parent.id)
                .unwrap()
                .relationships
                .len(),
            1
        );
        app.relate_records(grandchild.id, child.id).unwrap();
        assert_eq!(
            app.relate_records(parent.id, grandchild.id)
                .unwrap_err()
                .code,
            "relationship-cycle"
        );
        app.start_record(parent.id).unwrap();
        assert_eq!(
            app.done_record(parent.id, None).unwrap_err().code,
            "children-not-done"
        );
        for record_id in [grandchild.id, child.id] {
            app.start_record(record_id).unwrap();
            app.done_record(record_id, Some("complete")).unwrap();
        }
        assert_eq!(
            app.done_record(parent.id, None).unwrap().status,
            RecordStatus::Done
        );
        assert_eq!(
            app.unrelate_records(child.id, parent.id)
                .unwrap()
                .relationships
                .len(),
            1
        );
    }

    #[test]
    fn comments_require_meaningful_content_and_are_immutable() {
        let app = application();
        let record = app.open_record("record", vec![]).unwrap();
        assert_eq!(
            app.add_record_comment(record.id, " ", None)
                .unwrap_err()
                .code,
            "invalid-comment"
        );
        assert_eq!(
            app.add_record_comment(record.id, "text", Some(" "))
                .unwrap_err()
                .code,
            "invalid-comment-author"
        );
        let comment = app
            .add_record_comment(record.id, "text", Some("agent"))
            .unwrap();
        assert_eq!(
            app.list_record_comments(record.id).unwrap(),
            vec![comment.clone()]
        );
        app.delete_record_comment(record.id, comment.id.0).unwrap();
        assert!(app.list_record_comments(record.id).unwrap().is_empty());
    }

    #[test]
    fn opening_assigns_open_status_and_sorts_scope() {
        assert_eq!(
            application()
                .open_record("body", vec!["zeta".into(), "alpha".into()])
                .unwrap(),
            Record {
                id: 1,
                record_type: RecordType::Generic,
                body: "body".into(),
                project_slugs: vec!["alpha".into(), "zeta".into()],
                status: RecordStatus::Open,
                completion_resolution: None,
                relationships: vec![],
            }
        );
    }
}
