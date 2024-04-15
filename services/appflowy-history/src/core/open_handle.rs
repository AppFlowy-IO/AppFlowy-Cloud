use crate::biz::history::CollabHistory;
use crate::biz::persistence::HistoryPersistence;
use crate::error::HistoryError;
use collab::core::collab::{DataSource, MutexCollab, TransactionMutExt, WeakMutexCollab};
use collab::core::origin::CollabOrigin;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{Collab, Update};
use collab_entity::CollabType;
use collab_stream::stream_group::{ReadOption, StreamGroup};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, trace};

pub struct OpenCollabHandle {
  pub object_id: String,
  pub mutex_collab: MutexCollab,
  pub collab_type: CollabType,
  pub history: Arc<CollabHistory>,

  #[allow(dead_code)]
  /// The history persistence to save the history periodically.
  /// bind the lifetime to the handle.
  history_persistence: Option<Arc<HistoryPersistence>>,
}

impl OpenCollabHandle {
  pub fn new(
    object_id: &str,
    doc_state: Vec<u8>,
    collab_type: CollabType,
    update_stream: Option<StreamGroup>,
    history_persistence: Option<Arc<HistoryPersistence>>,
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

    let history = Arc::new(CollabHistory::new(
      &object_id,
      mutex_collab.clone(),
      collab_type.clone(),
    )?);

    // Spawn a task to receive updates from the update stream.
    spawn_recv_update(
      &object_id,
      &collab_type,
      mutex_collab.downgrade(),
      update_stream,
    );

    // spawn a task periodically to save the history to the persistence.
    if let Some(persistence) = &history_persistence {
      spawn_save_history(Arc::downgrade(&history), Arc::downgrade(persistence));
    }

    Ok(Self {
      object_id,
      mutex_collab,
      collab_type,
      history,
      history_persistence,
    })
  }

  pub async fn gen_history(&self) -> Result<(), HistoryError> {
    if let Some(history_persistence) = &self.history_persistence {
      save_history(self.history.clone(), history_persistence.clone()).await;
    }
    Ok(())
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
        let weak_mutex_collab = mutex_collab.clone();
        match tokio::task::spawn_blocking(move || {
          for message in &messages {
            if let Ok(update) = Update::decode_v1(&message.data) {
              if let Some(mutex_collab) = weak_mutex_collab.upgrade() {
                let lock_guard = mutex_collab.lock();
                let mut txn = lock_guard.try_transaction_mut()?;
                txn.try_apply_update(update)?;
              }
            }
          }
          Ok::<_, HistoryError>(messages)
        })
        .await
        {
          Ok(Ok(messages)) => {
            if let Err(err) = update_stream.ack_messages(&messages).await {
              error!("Failed to ack update stream messages: {:?}", err);
            }
          },
          Ok(Err(err)) => error!(
            "{}:{} failed to apply update: {:?}",
            object_id, collab_type, err
          ),
          Err(err) => {
            error!(
              "Failed to spawn_blocking when trying to apply udpate: {:?}",
              err
            );
          },
        }
      }
    }
  });
}

fn spawn_save_history(history: Weak<CollabHistory>, history_persistence: Weak<HistoryPersistence>) {
  tokio::spawn(async move {
    let mut interval = if cfg!(debug_assertions) {
      interval(Duration::from_secs(60))
    } else {
      interval(Duration::from_secs(60 * 60))
    };

    loop {
      interval.tick().await;
      if let (Some(history), Some(history_persistence)) =
        (history.upgrade(), history_persistence.upgrade())
      {
        save_history(history, history_persistence).await;
      } else {
        break;
      }
    }
  });
}

#[inline]
async fn save_history(history: Arc<CollabHistory>, history_persistence: Arc<HistoryPersistence>) {
  match history.gen_snapshot_context().await {
    Ok(Some(ctx)) => {
      if let Err(err) = history_persistence
        .save_snapshot(ctx.state, ctx.snapshots, ctx.collab_type)
        .await
      {
        error!("Failed to save snapshot: {:?}", err);
      }
    },
    Ok(None) => {
      trace!("No snapshot to save");
    },
    Err(err) => error!("Failed to generate snapshot context: {:?}", err),
  }
}
