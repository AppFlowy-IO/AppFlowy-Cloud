use crate::collaborate::group::CollabGroup;
use crate::entities::RealtimeUser;
use crate::error::RealtimeError;
use app_error::AppError;
use async_trait::async_trait;

use crate::collaborate::{CollabAccessControl, CollabUserId};
use collab::core::collab::TransactionMutExt;
use collab::core::collab_plugin::EncodedCollabV1;
use collab::core::origin::CollabOrigin;
use collab::preclude::{CollabPlugin, Doc, TransactionMut};
use collab::sync_protocol::awareness::Awareness;
use collab_entity::CollabType;
use database::collab::CollabStorage;
use database_entity::dto::{AFAccessLevel, InsertCollabParams, QueryCollabParams};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use tracing::{error, trace};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{ReadTxn, StateVector, Transact, Update};

pub struct CollabStoragePlugin<S, U, AC> {
  uid: i64,
  workspace_id: String,
  storage: Arc<S>,
  did_load: AtomicBool,
  update_count: AtomicU32,
  group: Weak<CollabGroup<U>>,
  collab_type: CollabType,
  access_control: Arc<AC>,
}

impl<S, U, AC> CollabStoragePlugin<S, U, AC> {
  pub fn new(
    uid: i64,
    workspace_id: &str,
    collab_type: CollabType,
    storage: S,
    group: Weak<CollabGroup<U>>,
    access_control: Arc<AC>,
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
      access_control,
    }
  }
}

fn init_collab_with_raw_data(
  encoded_collab: EncodedCollabV1,
  doc: &Doc,
) -> Result<(), RealtimeError> {
  if encoded_collab.doc_state.is_empty() {
    return Err(RealtimeError::UnexpectedData("raw data is empty"));
  }
  let mut txn = doc.transact_mut();
  if encoded_collab.doc_state.is_empty() {
    return Err(RealtimeError::UnexpectedData("Document state is empty"));
  }
  let update = Update::decode_v1(&encoded_collab.doc_state)?;
  txn.try_apply_update(update)?;
  Ok(())
}

#[async_trait]
impl<S, U, AC> CollabPlugin for CollabStoragePlugin<S, U, AC>
where
  S: CollabStorage,
  U: RealtimeUser,
  AC: CollabAccessControl,
{
  async fn init(&self, object_id: &str, _origin: &CollabOrigin, doc: &Doc) {
    let params = QueryCollabParams {
      object_id: object_id.to_string(),
      workspace_id: self.workspace_id.clone(),
      collab_type: self.collab_type.clone(),
    };

    match self.storage.get_collab_encoded_v1(&self.uid, params).await {
      Ok(encoded_collab) => match init_collab_with_raw_data(encoded_collab, doc) {
        Ok(_) => {},
        Err(e) => error!("🔴Init collab failed: {:?}", e),
      },
      Err(err) => match &err {
        AppError::RecordNotFound(_) => {
          let _ = self
            .access_control
            .cache_collab_access_level(
              CollabUserId::from(&self.uid),
              object_id,
              AFAccessLevel::FullAccess,
            )
            .await;

          let result = {
            let txn = doc.transact();
            let doc_state = txn.encode_diff_v1(&StateVector::default());
            let state_vector = txn.state_vector().encode_v1();
            EncodedCollabV1::new(doc_state, state_vector).encode_to_bytes()
          };

          match result {
            Ok(encoded_collab_v1) => {
              let params = InsertCollabParams::from_raw_data(
                object_id,
                self.collab_type.clone(),
                encoded_collab_v1,
                &self.workspace_id,
              );

              trace!("Collab not found, create new one");
              if let Err(err) = self.storage.insert_collab(&self.uid, params).await {
                error!("fail to create new collab in plugin: {:?}", err);
              }
            },
            Err(err) => {
              error!("fail to encode EncodedCollabV1 to bytes: {:?}", err);
            },
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

  fn flush(&self, object_id: &str, data: &EncodedCollabV1) {
    let storage = self.storage.clone();
    match data.encode_to_bytes() {
      Ok(encoded_collab_v1) => {
        let params = InsertCollabParams::from_raw_data(
          object_id,
          self.collab_type.clone(),
          encoded_collab_v1,
          &self.workspace_id,
        );

        tracing::debug!(
          "[realtime] start flushing {}:{} with len: {}",
          object_id,
          params.collab_type,
          params.encoded_collab_v1.len()
        );

        let uid = self.uid;
        tokio::spawn(async move {
          let object_id = params.object_id.clone();
          match storage.insert_collab(&uid, params).await {
            Ok(_) => tracing::debug!("[realtime] end flushing collab: {}", object_id),
            Err(err) => tracing::error!("save collab failed: {:?}", err),
          }
        });
      },
      Err(err) => {
        error!("fail to encode EncodedDocV1 to bytes: {:?}", err);
      },
    }
  }
}
