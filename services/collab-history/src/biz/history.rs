use crate::biz::open_handle::OpenCollabHandle;
use crate::biz::snapshot::{gen_snapshot, CollabSnapshot, CollabStateSnapshot, SnapshotGenerator};
use crate::error::HistoryError;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::updates::encoder::{Encoder, EncoderV2};
use collab::preclude::{ReadTxn, Snapshot, StateVector, Update};
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

    let state = CollabStateSnapshot::new(
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

  /// Apply an update to the collab.
  /// The update is encoded in the v1 format.
  pub fn apply_update_v1(&self, update: &[u8]) -> Result<(), HistoryError> {
    let lock_guard = self.open_collab_handle.mutex_collab.lock();
    let mut txn = lock_guard.try_transaction_mut()?;
    let decode_update =
      Update::decode_v1(update).map_err(|err| HistoryError::Internal(err.into()))?;
    txn.apply_update(decode_update);
    drop(txn);
    drop(lock_guard);

    self.snapshot_generator.did_apply_update(update);
    Ok(())
  }

  #[cfg(debug_assertions)]
  pub fn json(&self) -> Value {
    let lock_guard = self.open_collab_handle.mutex_collab.lock();
    lock_guard.to_json_value()
  }
}

pub struct SnapshotContext {
  pub state: CollabStateSnapshot,
  pub snapshots: Vec<CollabSnapshot>,
}
