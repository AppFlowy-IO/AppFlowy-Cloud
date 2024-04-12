use crate::biz::snapshot::{gen_snapshot, CollabSnapshot, CollabSnapshotState, SnapshotGenerator};
use crate::core::open_handle::OpenCollabHandle;
use crate::error::HistoryError;

use collab::preclude::updates::encoder::{Encoder, EncoderV2};
use collab::preclude::{CollabPlugin, ReadTxn, Snapshot, StateVector, TransactionMut};
use serde_json::Value;
use std::sync::Arc;

pub struct CollabHistory {
  open_collab_handle: Arc<OpenCollabHandle>,
  snapshot_generator: SnapshotGenerator,
}

impl CollabHistory {
  pub fn new(open_collab_handle: Arc<OpenCollabHandle>) -> Result<Self, HistoryError> {
    let weak_open_collab = Arc::downgrade(&open_collab_handle);
    let snapshot_generator =
      SnapshotGenerator::new(weak_open_collab, open_collab_handle.collab_type.clone());

    open_collab_handle
      .mutex_collab
      .lock()
      .add_plugin(Box::new(CountUpdatePlugin {
        snapshot_generator: snapshot_generator.clone(),
      }));

    Ok(Self {
      snapshot_generator,
      open_collab_handle,
    })
  }

  #[cfg(debug_assertions)]
  /// Generate a snapshot of the current state of the collab
  /// Only for testing purposes. We use [SnapshotGenerator] to generate snapshot
  pub fn gen_snapshot(&self, uid: i64) -> Result<CollabSnapshot, HistoryError> {
    gen_snapshot(&self.open_collab_handle.mutex_collab, uid)
  }

  pub async fn gen_state_snapshot(&self) -> Result<SnapshotContext, HistoryError> {
    let (doc_state_v2, state_vector) = {
      let lock_guard = self.open_collab_handle.mutex_collab.lock();
      let txn = lock_guard.try_transaction()?;
      // TODO(nathan): reduce the size of doc_state_v2 by encoding the previous [CollabStateSnapshot] doc_state_v2
      let doc_state_v2 = txn.encode_state_as_update_v2(&StateVector::default());
      let state_vector = txn.state_vector();
      drop(txn);
      (doc_state_v2, state_vector)
    };

    let timestamp = chrono::Utc::now().timestamp();
    let snapshots = self.snapshot_generator.take_pending_snapshots().await
      .into_iter()
        // Remove the snapshots which created_at is bigger than the current timestamp
      .filter(|snapshot| snapshot.created_at <= timestamp)
      .collect();

    let state = CollabSnapshotState::new(
      self.open_collab_handle.object_id.clone(),
      doc_state_v2,
      state_vector,
      chrono::Utc::now().timestamp(),
    );
    Ok(SnapshotContext { state, snapshots })
  }

  /// Encode the state of the collab as Update.
  /// We encode the collaboration state as an update using the v2 format, chosen over the v1 format
  /// due to its reduced data size. This optimization helps in minimizing the storage and
  /// transmission overhead, making the process more efficient.
  pub fn encode_update_v2(&self, snapshot: &Snapshot) -> Result<Vec<u8>, HistoryError> {
    let lock_guard = self.open_collab_handle.mutex_collab.lock();
    let txn = lock_guard.try_transaction()?;
    let mut encoder = EncoderV2::new();
    txn
      .encode_state_from_snapshot(snapshot, &mut encoder)
      .map_err(|err| HistoryError::Internal(err.into()))?;
    Ok(encoder.to_vec())
  }

  #[cfg(debug_assertions)]
  pub fn json(&self) -> Value {
    let lock_guard = self.open_collab_handle.mutex_collab.lock();
    lock_guard.to_json_value()
  }
}

pub struct SnapshotContext {
  pub state: CollabSnapshotState,
  pub snapshots: Vec<CollabSnapshot>,
}

struct CountUpdatePlugin {
  snapshot_generator: SnapshotGenerator,
}
impl CollabPlugin for CountUpdatePlugin {
  fn receive_update(&self, _object_id: &str, _txn: &TransactionMut, update: &[u8]) {
    self.snapshot_generator.did_apply_update(update);
  }
}
