use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotMeta {
  pub oid: String,
  /// Using yrs::Snapshot to deserialize the snapshot
  pub snapshot: Vec<u8>,
  /// Specifies the version of the snapshot
  pub snapshot_version: i32,
  pub created_at: i64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct RepeatedSnapshotMeta {
  pub items: Vec<SnapshotMeta>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryState {
  pub object_id: String,
  pub doc_state: Vec<u8>,
  pub doc_state_version: i32,
}

///
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotInfo {
  pub history: HistoryState,
  pub snapshot_meta: SnapshotMeta,
}
