//! Offline storage-upgrade contracts shared with the revision manager.
use serde::{Deserialize, Serialize};

/// Separate from both the storage schema and daemon Management Protocol.
pub const UPGRADE_INTERFACE_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeCapabilities {
    pub interface_version: u32,
    pub target_schema: u32,
    pub supported_sources: Vec<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableInventory {
    pub name: String,
    pub definition: String,
    pub rows: u64,
    pub digest: String,
}

/// Read-only evidence. The fingerprint covers schema and every stored value,
/// independently of SQLite page layout, WAL checkpointing, and row order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreInspection {
    pub schema: Option<u32>,
    pub fingerprint: String,
    pub pristine: bool,
    pub blockers: Vec<String>,
    pub tables: Vec<TableInventory>,
}
