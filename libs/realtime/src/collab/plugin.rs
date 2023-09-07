use crate::error::CollabSyncError;
use async_trait::async_trait;
use collab::core::origin::CollabOrigin;
use collab::preclude::{CollabPlugin, Doc, TransactionMut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use storage::collab::CollabStorage;
use y_sync::awareness::Awareness;

pub struct CollabStoragePlugin {
  storage: Arc<CollabStorage>,
  did_load: Arc<AtomicBool>,
}

impl CollabStoragePlugin {
  pub fn new(storage: Arc<CollabStorage>) -> Result<Self, CollabSyncError> {
    let did_load = Arc::new(AtomicBool::new(false));
    Ok(Self { storage, did_load })
  }
}

#[async_trait]
impl CollabPlugin for CollabStoragePlugin {
  async fn init(&self, object_id: &str, _origin: &CollabOrigin, _doc: &Doc) {
    // let mut txn = doc.transact_mut_with(origin.clone());

    // Check the document is exist or not
    if self.storage.is_exist(object_id).await {
      let _raw_data = self.storage.get_collab(object_id).await.unwrap();

      // let _ = r_db_txn
      //     .load_doc_with_txn(self.collab_id, object_id, &mut txn)
      //     .unwrap();
      // drop(r_db_txn);
    } else {
      // Drop the read txn before write txn
      // let result = self.db.with_write_txn(|w_db_txn| {
      //     w_db_txn.create_new_doc(self.collab_id, object_id, &txn)?;
      //     Ok(())
      // });
      let raw_data = vec![];
      self
        .storage
        .update_collab(object_id, raw_data)
        .await
        .unwrap();
    }
  }
  fn did_init(&self, _awareness: &Awareness, _object_id: &str) {
    self.did_load.store(true, Ordering::SeqCst);
  }

  fn receive_update(&self, _object_id: &str, _txn: &TransactionMut, _update: &[u8]) {
    // Only push update if the doc is loaded
    if !self.did_load.load(Ordering::SeqCst) {
      return;
    }
    todo!()
    // /Acquire a write transaction to ensure consistency
    // let result = self.db.with_write_txn(|w_db_txn| {
    //     let _ = w_db_txn.push_update(self.collab_id, object_id, update)?;
    //     Ok(())
    // });
    //
    // if let Err(e) = result {
    //     tracing::error!("🔴Save update failed: {:?}", e);
    // }
  }
}
