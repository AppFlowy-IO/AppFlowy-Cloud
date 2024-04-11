use crate::error::HistoryError;
use collab::core::collab::{DataSource, MutexCollab};
use collab::core::origin::CollabOrigin;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{Collab, Update};
use collab_entity::CollabType;

pub struct OpenCollabHandle {
  pub object_id: String,
  pub mutex_collab: MutexCollab,
  pub collab_type: CollabType,
  pub callbacks: Vec<Box<dyn OpenCollabCallback>>,
}

impl OpenCollabHandle {
  pub fn new(
    object_id: &str,
    doc_state: Vec<u8>,
    collab_type: CollabType,
  ) -> Result<Self, HistoryError> {
    let collab = Collab::new_with_source(
      CollabOrigin::Empty,
      object_id,
      DataSource::DocStateV1(doc_state),
      vec![],
      true,
    )?;

    let mutex_collab = MutexCollab::new(collab);
    let object_id = object_id.to_string();
    Ok(Self {
      object_id,
      mutex_collab,
      collab_type,
      callbacks: vec![],
    })
  }

  pub fn register_callback(&mut self, callback: Box<dyn OpenCollabCallback>) {
    self.callbacks.push(callback);
  }

  /// Apply an update to the collab.
  /// The update is encoded in the v1 format.
  pub fn apply_update_v1(&self, update: &[u8]) -> Result<(), HistoryError> {
    let lock_guard = self.mutex_collab.lock();
    let mut txn = lock_guard.try_transaction_mut()?;
    let decode_update =
      Update::decode_v1(update).map_err(|err| HistoryError::Internal(err.into()))?;
    txn.apply_update(decode_update);
    drop(txn);
    drop(lock_guard);
    Ok(())
  }
}

pub struct UpdateStream {}

pub trait OpenCollabCallback: Send + Sync {
  fn did_apply_update(&self, update: &[u8]);
}
