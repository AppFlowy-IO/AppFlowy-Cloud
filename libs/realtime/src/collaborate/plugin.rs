use crate::collaborate::group::CollabGroup;
use crate::entities::RealtimeUser;
use crate::error::RealtimeError;
use async_trait::async_trait;
use bytes::Bytes;
use collab::core::collab::TransactionMutExt;
use collab::core::origin::CollabOrigin;
use collab::preclude::{CollabPlugin, Doc, TransactionMut};
use collab::sync_protocol::awareness::Awareness;
use collab_entity::CollabType;
use database::collab::CollabStorage;
use database_entity::dto::{InsertCollabParams, QueryCollabParams, RawData};
use database_entity::error::DatabaseError;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use tracing::{error, trace};
use yrs::updates::decoder::Decode;
use yrs::{ReadTxn, StateVector, Transact, Update};

pub struct CollabStoragePlugin<S, U> {
  uid: i64,
  workspace_id: String,
  storage: Arc<S>,
  did_load: AtomicBool,
  update_count: AtomicU32,
  group: Weak<CollabGroup<U>>,
  collab_type: CollabType,
}

impl<S, U> CollabStoragePlugin<S, U> {
  pub fn new(
    uid: i64,
    workspace_id: &str,
    collab_type: CollabType,
    storage: S,
    group: Weak<CollabGroup<U>>,
  ) -> Self {
    let storage = Arc::new(storage);
    let workspace_id = workspace_id.to_string();
    let did_load = AtomicBool::new(false);
    let update_count = AtomicU32::new(0);
    Self {
      uid,
      workspace_id,
      storage,
      did_load,
      update_count,
      group,
      collab_type,
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
impl<S, U> CollabPlugin for CollabStoragePlugin<S, U>
where
  S: CollabStorage,
  U: RealtimeUser,
{
  async fn init(&self, object_id: &str, _origin: &CollabOrigin, doc: &Doc) {
    let params = QueryCollabParams {
      object_id: object_id.to_string(),
      workspace_id: self.workspace_id.clone(),
      collab_type: self.collab_type.clone(),
    };

    match self.storage.get_collab(&self.uid, params).await {
      Ok(raw_data) => match init_collab_with_raw_data(raw_data, doc) {
        Ok(_) => {},
        Err(e) => error!("🔴Init collab failed: {:?}", e),
      },
      Err(err) => match &err {
        DatabaseError::RecordNotFound(_) => {
          let raw_data = {
            let txn = doc.transact();
            txn.encode_state_as_update_v1(&StateVector::default())
          };
          let params = InsertCollabParams::from_raw_data(
            object_id,
            self.collab_type.clone(),
            raw_data,
            &self.workspace_id,
          );

          trace!("Collab not found, create new one");
          if let Err(err) = self.storage.insert_collab(&self.uid, params).await {
            error!("fail to create new collab in plugin: {:?}", err);
          }
        },
        _ => error!("{:?}", err),
      },
    }
  }
  fn did_init(&self, _awareness: &Awareness, _object_id: &str) {
    self.did_load.store(true, Ordering::SeqCst);
  }

  fn receive_update(&self, _object_id: &str, _txn: &TransactionMut, _update: &[u8]) {
    let count = self.update_count.fetch_add(1, Ordering::SeqCst);
    tracing::trace!("receive_update, count: {}", count);
    if !self.did_load.load(Ordering::SeqCst) {
      return;
    }

    if count >= self.storage.config().flush_per_update {
      self.update_count.store(0, Ordering::SeqCst);
      tracing::trace!("number of updates reach flush_per_update, start flushing");
      match self.group.upgrade() {
        None => tracing::error!("🔴Group is dropped, skip flush collab"),
        Some(group) => group.save_collab(),
      }
    }
  }

  fn flush(&self, object_id: &str, update: &Bytes) {
    let storage = self.storage.clone();
    let params = InsertCollabParams::from_raw_data(
      object_id,
      self.collab_type.clone(),
      update.to_vec(),
      &self.workspace_id,
    );

    tracing::debug!(
      "[💭Server] start flushing {}:{} with len: {}",
      object_id,
      params.collab_type,
      params.raw_data.len()
    );

    let uid = self.uid;
    tokio::spawn(async move {
      let object_id = params.object_id.clone();
      match storage.insert_collab(&uid, params).await {
        Ok(_) => tracing::debug!("[💭Server] end flushing collab: {}", object_id),
        Err(err) => tracing::error!("save collab failed: {:?}", err),
      }
    });
  }
}
