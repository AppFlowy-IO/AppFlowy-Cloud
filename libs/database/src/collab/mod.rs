mod collab_db_ops;
mod collab_storage;
// mod recent;

pub use collab_db_ops::*;
use collab_entity::CollabType;
pub use collab_storage::*;

pub(crate) fn partition_key_from_collab_type(collab_type: &CollabType) -> i32 {
  match collab_type {
    CollabType::Document => 0,
    CollabType::Database => 1,
    CollabType::WorkspaceDatabase => 2,
    CollabType::Folder => 3,
    CollabType::DatabaseRow => 4,
    CollabType::UserAwareness => 5,
    // TODO(nathan): create a partition table for CollabType::Unknown
    CollabType::Unknown => 0,
  }
}
