use crate::error::HistoryError;
use collab::core::collab::{DataSource, MutexCollab, TransactionMutExt, WeakMutexCollab};
use collab::core::origin::CollabOrigin;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{Collab, Update};
use collab_entity::CollabType;
use collab_stream::client::CollabRedisStream;
use collab_stream::stream_group::{ReadOption, StreamGroup};
use std::time::Duration;
use tokio::time::interval;
use tracing::error;

pub struct OpenCollabHandle {
  pub object_id: String,
  pub mutex_collab: MutexCollab,
  pub collab_type: CollabType,
  update_stream: Option<StreamGroup>,
}

impl OpenCollabHandle {
  pub fn new(
    object_id: &str,
    doc_state: Vec<u8>,
    collab_type: CollabType,
    update_stream: Option<StreamGroup>,
  ) -> Result<Self, HistoryError> {
    let mut collab = Collab::new_with_source(
      CollabOrigin::Empty,
      object_id,
      DataSource::DocStateV1(doc_state),
      vec![],
      true,
    )?;
    collab.initialize();

    let mutex_collab = MutexCollab::new(collab);
    let object_id = object_id.to_string();
    let cloned_update_stream = update_stream.clone();

    // Spawn a task to receive updates from the update stream.
    spawn_recv_update(
      &object_id,
      &collab_type,
      mutex_collab.downgrade(),
      cloned_update_stream,
    );

    Ok(Self {
      object_id,
      mutex_collab,
      collab_type,
      update_stream,
    })
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

fn spawn_recv_update(
  object_id: &str,
  collab_type: &CollabType,
  mutex_collab: WeakMutexCollab,
  update_stream: Option<StreamGroup>,
) {
  if update_stream.is_none() {
    return;
  }
  let mut update_stream = update_stream.unwrap();
  let mut interval = interval(Duration::from_secs(5));
  let object_id = object_id.to_string();
  let collab_type = collab_type.clone();
  tokio::spawn(async move {
    loop {
      interval.tick().await;
      if let Ok(messages) = update_stream
        .consumer_messages("open_collab", ReadOption::Undelivered)
        .await
      {
        for message in &messages {
          if let Ok(update) = Update::decode_v1(&message.data) {
            if let Some(mutex_collab) = mutex_collab.upgrade() {
              let mut lock_guard = mutex_collab.lock();
              let mut txn = lock_guard.try_transaction_mut().unwrap();

              // TODO(nathan): impl retry when panic happened in the apply_update will cause this loop to exit.
              if let Err(err) = txn.try_apply_update(update) {
                error!(
                  "{}:{} failed to apply update: {:?}",
                  object_id, collab_type, err
                );
              }
            }
          }
        }
        if let Err(err) = update_stream.ack_messages(&messages).await {
          error!("Failed to ack update stream messages: {:?}", err);
        }
      }
    }
  });
}
