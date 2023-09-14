use crate::error::RealtimeError;
use async_trait::async_trait;
use bytes::Bytes;
use collab::core::collab::TransactionMutExt;
use collab::core::origin::CollabOrigin;
use collab::preclude::{CollabPlugin, Doc, TransactionMut};

use collab_define::CollabType;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Weak};

use storage::collab::CollabStorage;
use storage::error::StorageError;
use storage_entity::{InsertCollabParams, QueryCollabParams, RawData};

use crate::collaborate::group::CollabGroup;
use y_sync::awareness::Awareness;
use yrs::updates::decoder::Decode;
use yrs::{ReadTxn, StateVector, Transact, Update};

pub struct CollabStoragePlugin<S> {
  uid: i64,
  workspace_id: String,
  storage: Arc<S>,
  did_load: AtomicBool,
  update_count: AtomicU32,
  group: Weak<CollabGroup>,
  collab_type: CollabType,
}

impl<S> CollabStoragePlugin<S> {
  pub fn new(
    uid: i64,
    workspace_id: &str,
    collab_type: CollabType,
    storage: S,
    group: Weak<CollabGroup>,
  ) -> Self {
    let workspace_id = workspace_id.to_string();
    let did_load = AtomicBool::new(false);
    let update_count = AtomicU32::new(0);
    Self {
      uid,
      workspace_id,
      storage: Arc::new(storage),
      did_load,
      update_count,
      group,
      collab_type,
    }
  }
}

impl<S> CollabStoragePlugin<S>
where
  S: CollabStorage,
{
  pub fn flush_collab(&self, _object_id: &str) {
    match self.group.upgrade() {
      None => tracing::error!("🔴Group is dropped, skip flush collab"),
      Some(group) => group.flush_collab(),
    }
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
impl<S> CollabPlugin for CollabStoragePlugin<S>
where
  S: CollabStorage,
{
  async fn init(&self, object_id: &str, _origin: &CollabOrigin, doc: &Doc) {
    let params = QueryCollabParams {
      object_id: object_id.to_string(),
      collab_type: self.collab_type.clone(),
    };

    match self.storage.get_collab(params).await {
      Ok(raw_data) => match init_collab_with_raw_data(raw_data, doc) {
        Ok(_) => {},
        Err(e) => {
          // TODO: retry?
          tracing::error!("🔴Init collab failed: {:?}", e);
        },
      },
      Err(err) => {
        match &err {
          StorageError::RecordNotFound => {
            let raw_data = {
              let txn = doc.transact();
              txn.encode_state_as_update_v1(&StateVector::default())
            };
            let params = InsertCollabParams::from_raw_data(
              self.uid,
              object_id,
              self.collab_type.clone(),
              raw_data,
              &self.workspace_id,
            );
            match self.storage.insert_collab(params).await {
              Ok(_) => {},
              Err(err) => tracing::error!("🔴Create collab failed: {:?}", err),
            }
          },
          _ => {
            tracing::error!("🔴Get collab failed: {:?}", err);
            // TODO: retry?
          },
        }
      },
    }
  }
  fn did_init(&self, _awareness: &Awareness, _object_id: &str) {
    self.did_load.store(true, Ordering::SeqCst);
  }

  fn receive_update(&self, object_id: &str, _txn: &TransactionMut, _update: &[u8]) {
    if !self.did_load.load(Ordering::SeqCst) {
      return;
    }
    let count = self.update_count.fetch_add(1, Ordering::SeqCst);
    if count >= self.storage.config().flush_per_update {
      self.update_count.store(0, Ordering::SeqCst);
      self.flush_collab(object_id);
    }
  }

  fn flush(&self, object_id: &str, update: &Bytes) {
    tracing::debug!("[💭Server] start flushing collab: {}", object_id);
    let weak_storage = Arc::downgrade(&self.storage);
    let params = InsertCollabParams::from_raw_data(
      self.uid,
      object_id,
      self.collab_type.clone(),
      update.to_vec(),
      &self.workspace_id,
    );

    tokio::spawn(async move {
      if let Some(storage) = weak_storage.upgrade() {
        let object_id = params.object_id.clone();
        match storage.insert_collab(params).await {
          Ok(_) => tracing::debug!("[💭Server] end flushing collab: {}", object_id),
          Err(err) => tracing::error!("🔴Update collab failed: {:?}", err),
        }
      }
    });
  }
}
