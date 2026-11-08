use serde::{Deserialize, Serialize};

/// Request-local Workspace routing. `cwd` is supplied by the client, never the daemon.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecordSelection {
    pub workspace: Option<String>,
    pub cwd: Option<String>,
}

/// All Record operations carry the same explicit routing envelope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SelectedRecordRequest<T> {
    #[serde(flatten)]
    pub selection: RecordSelection,
    #[serde(flatten)]
    pub record: T,
}
impl<T> SelectedRecordRequest<T> {
    pub fn selected(workspace: impl Into<String>, record: T) -> Self {
        Self {
            selection: RecordSelection {
                workspace: Some(workspace.into()),
                cwd: None,
            },
            record,
        }
    }
}
