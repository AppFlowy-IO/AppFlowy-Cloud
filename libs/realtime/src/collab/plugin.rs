use crate::error::RealtimeError;
use async_trait::async_trait;
use collab::core::collab::TransactionMutExt;
use collab::core::origin::CollabOrigin;
use collab::preclude::{CollabPlugin, Doc, TransactionMut};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use storage::collab::{CollabStorage, RawData};
use storage::entities::CreateCollabParams;
use storage::error::StorageError;

use y_sync::awareness::Awareness;
use yrs::updates::decoder::Decode;
use yrs::{ReadTxn, StateVector, Transact, Update};

pub struct CollabStoragePlugin {
  workspace_id: String,
  storage: Arc<CollabStorage>,
  did_load: AtomicBool,
  update_count: AtomicU32,
}

impl CollabStoragePlugin {
  pub fn new(workspace_id: &str, storage: Arc<CollabStorage>) -> Result<Self, RealtimeError> {
    let workspace_id = workspace_id.to_string();
    let did_load = AtomicBool::new(false);
    let update_count = AtomicU32::new(0);
    Ok(Self {
      workspace_id,
      storage,
      did_load,
      update_count,
    })
  }
}

fn init_collab_with_raw_data(raw_data: RawData, doc: &Doc) -> Result<(), RealtimeError> {
  if raw_data.is_empty() {
    return Err(RealtimeError::UnexpectedData("raw data is empty"));
  }
  let mut txn = doc.transact_mut();
  let update = Update::decode_v1(&raw_data)?;
  txn.try_apply_update(update)?;
  Ok(())
}

#[async_trait]
impl CollabPlugin for CollabStoragePlugin {
  async fn init(&self, object_id: &str, _origin: &CollabOrigin, doc: &Doc) {
    match self.storage.get_collab(object_id).await {
      Ok(raw_data) => match init_collab_with_raw_data(raw_data, doc) {
        Ok(_) => {},
        Err(e) => {
          // TODO: retry?
          tracing::error!("🔴Init collab failed: {:?}", e);
        },
      },
      Err(err) => {
        match err {
          StorageError::RecordNotFound => {
            let raw_data = {
              let txn = doc.transact();
              txn.encode_state_as_update_v1(&StateVector::default())
            };
            let params = CreateCollabParams::from_raw_data(object_id, raw_data, &self.workspace_id);
            match self.storage.create_collab(params).await {
              Ok(_) => {},
              Err(err) => tracing::error!("🔴Create collab failed: {:?}", err),
            }
          },
          StorageError::Internal(e) => {
            tracing::error!("🔴Get collab failed: {:?}", e);
            // TODO: retry?
          },
        }
      },
    }
  }
  fn did_init(&self, _awareness: &Awareness, _object_id: &str) {
    self.did_load.store(true, Ordering::SeqCst);
  }

  fn receive_update(&self, object_id: &str, _txn: &TransactionMut, update: &[u8]) {
    if !self.did_load.load(Ordering::SeqCst) {
      return;
    }
    tracing::trace!("🔵Receive {} update with len: {}", object_id, update.len());
    let update_count = self.update_count.fetch_add(1, Ordering::SeqCst);
    if update_count > 100 {
      // Save the collab to disk
    }
  }

  fn flush(&self, object_id: &str, _update: &[u8]) {
    tracing::trace!("🔵Flush collab: {}", object_id);
  }
}
