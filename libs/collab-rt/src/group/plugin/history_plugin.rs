use crate::group::group_init::WeakMutexCollab;
use crate::rt_server::COLLAB_RUNTIME;

use collab::preclude::CollabPlugin;
use collab_entity::CollabType;
use database::collab::CollabStorage;
use database_entity::dto::InsertSnapshotParams;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{error, trace};
use yrs::TransactionMut;

/// [HistoryPlugin] will be moved to history collab server. For now, it's temporarily placed here.
pub struct HistoryPlugin<S> {
  workspace_id: String,
  object_id: String,
  collab_type: CollabType,
  storage: Arc<S>,
  did_create_snapshot: AtomicBool,
  weak_collab: WeakMutexCollab,
}

impl<S> HistoryPlugin<S>
where
  S: CollabStorage,
{
  pub fn new(
    workspace_id: String,
    object_id: String,
    collab_type: CollabType,
    weak_collab: WeakMutexCollab,
    storage: Arc<S>,
  ) -> Self {
    Self {
      workspace_id,
      object_id,
      collab_type,
      storage,
      did_create_snapshot: Default::default(),
      weak_collab,
    }
  }
}

impl<S> CollabPlugin for HistoryPlugin<S>
where
  S: CollabStorage,
{
  fn receive_update(&self, _object_id: &str, _txn: &TransactionMut, _update: &[u8]) {
    if self.did_create_snapshot.load(Ordering::Relaxed) {
      // Snapshot already created, no further action needed
      return;
    }
    self.did_create_snapshot.store(true, Ordering::SeqCst);

    let storage = self.storage.clone();
    let weak_collab = self.weak_collab.clone();
    let collab_type = self.collab_type.clone();
    let object_id = self.object_id.clone();
    let workspace_id = self.workspace_id.clone();

    COLLAB_RUNTIME.spawn(async move {
      if !storage.should_create_snapshot(&object_id).await {
        // Condition for creating snapshot not met, exit early
        return;
      }

      // Attempt to encode collaboration data for snapshot
      let encode_collab = weak_collab.upgrade().and_then(|collab| {
        collab.try_lock().and_then(|lock| {
          lock
            .encode_collab_v1(|collab| collab_type.validate(collab))
            .ok()
        })
      });

      if let Some(encode_collab) = encode_collab {
        match encode_collab.encode_to_bytes() {
          Ok(bytes) => {
            let params = InsertSnapshotParams {
              object_id,
              encoded_collab_v1: bytes,
              workspace_id,
              collab_type,
            };
            match storage.queue_snapshot(params).await {
              Ok(_) => trace!("Successfully queued snapshot creation"),
              Err(err) => error!("Failed to create snapshot: {:?}", err),
            }
          },
          Err(e) => error!("Failed to encode collaboration data: {:?}", e),
        }
      }
    });
  }
}
