use serde::{Deserialize, Serialize};
use uuid::Uuid;
macro_rules! impl_serialization {
  ($type:ty) => {
    impl $type {
      pub fn encode(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
      }

      pub fn decode(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
      }
    }
  };
}
#[derive(Serialize, Deserialize, Debug)]
pub struct SnapshotState {
  pub oid: String,
  pub doc_state: Vec<u8>,
  pub doc_state_version: i32,
  pub deps_snapshot_id: Option<Uuid>,
}
impl_serialization!(SnapshotState);

#[derive(Serialize, Deserialize, Debug)]
pub struct SnapshotMeta {
  pub oid: String,
  pub snapshot: Vec<u8>,
  /// Indicates the version of the snapshot format.
  /// if the version is 1, then using Snapshot::decode_v1 to decode the snapshot.
  /// if the version is 2, then using Snapshot::decode_v2 to decode the snapshot.
  pub snapshot_version: i32,
  pub created_at: i64,
}
impl_serialization!(SnapshotMeta);

#[derive(Serialize, Deserialize, Debug)]
pub struct RepeatedSnapshotMeta {
  pub items: Vec<SnapshotMeta>,
}
impl_serialization!(RepeatedSnapshotMeta);

#[derive(Serialize, Deserialize, Debug)]
pub struct SnapshotInfo {
  pub object_id: String,
  pub snapshot: Vec<u8>,
  pub doc_state: Vec<u8>,
  /// Indicates the version of the doc state format.
  /// if the version is 1, then using Update::decode_v1 to decode the doc state.
  /// if the version is 2, then using Update::decode_v2 to decode the doc state.
  pub doc_state_version: i32,
}
impl_serialization!(SnapshotInfo);
