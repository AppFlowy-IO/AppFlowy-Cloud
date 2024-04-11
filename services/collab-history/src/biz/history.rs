use crate::error::HistoryError;
use collab::core::collab::{DocStateSource, MutexCollab};
use collab::core::origin::CollabOrigin;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::updates::encoder::{Encoder, EncoderV2};
use collab::preclude::{Collab, ReadTxn, Snapshot, Update};
use serde_json::Value;
use std::ops::Deref;

pub struct CollabHistory {
  mutex_collab: MutexCollab,
}

impl CollabHistory {
  pub fn new(object_id: &str, doc_state: Vec<u8>) -> Result<Self, HistoryError> {
    let collab = Collab::new_with_doc_state(
      CollabOrigin::Empty,
      object_id,
      DocStateSource::FromDocState(doc_state),
      vec![],
      true,
    )?;

    let mutex_collab = MutexCollab::new(collab);
    Ok(Self { mutex_collab })
  }

  pub fn empty(object_id: &str) -> Result<Self, HistoryError> {
    let collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], true);
    let mutex_collab = MutexCollab::new(collab);
    Ok(Self { mutex_collab })
  }

  /// Generate a snapshot of the current state of the collab
  pub fn gen_snapshot(&self, uid: i64) -> Result<CollabSnapshot, HistoryError> {
    let lock_guard = self.mutex_collab.lock();
    let txn = lock_guard.try_transaction()?;
    let snapshot = txn.snapshot();
    let timestamp = chrono::Utc::now().timestamp();
    Ok(CollabSnapshot::new(uid, snapshot, timestamp))
  }

  /// Encode the state of the collab as Update.
  pub fn encode_update_v2(&self, snapshot: &Snapshot) -> Result<Vec<u8>, HistoryError> {
    let lock_guard = self.mutex_collab.lock();
    let txn = lock_guard.try_transaction()?;
    let mut encoder = EncoderV2::new();
    txn
      .encode_state_from_snapshot(snapshot, &mut encoder)
      .map_err(|err| HistoryError::Internal(err.into()))?;
    Ok(encoder.to_vec())
  }

  /// Apply an update to the collab
  ///
  pub fn apply_update_v2(&self, update: &[u8]) -> Result<(), HistoryError> {
    let lock_guard = self.mutex_collab.lock();
    let mut txn = lock_guard.try_transaction_mut()?;
    let update = Update::decode_v1(update).map_err(|err| HistoryError::Internal(err.into()))?;
    txn.apply_update(update);
    Ok(())
  }

  #[cfg(debug_assertions)]
  pub fn json(&self) -> Value {
    let lock_guard = self.mutex_collab.lock();
    lock_guard.to_json_value()
  }
}

pub struct CollabSnapshot {
  uid: i64,
  snapshot: Snapshot,
  created_at: i64,
}

impl Deref for CollabSnapshot {
  type Target = Snapshot;

  fn deref(&self) -> &Self::Target {
    &self.snapshot
  }
}

impl CollabSnapshot {
  pub fn new(uid: i64, snapshot: Snapshot, created_at: i64) -> Self {
    Self {
      uid,
      snapshot,
      created_at,
    }
  }
}
