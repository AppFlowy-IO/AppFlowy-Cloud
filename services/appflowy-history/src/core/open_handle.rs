use std::sync::{Arc, Weak};
use std::time::Duration;

use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::error::CollabError;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{Collab, Update};
use collab_entity::CollabType;
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{error, trace};

use collab_stream::model::{CollabUpdateEvent, StreamMessage};
use collab_stream::stream_group::{ReadOption, StreamGroup};
use tonic_proto::history::HistoryStatePb;

use crate::biz::history::CollabHistory;
use crate::biz::persistence::HistoryPersistence;
use crate::error::HistoryError;

const CONSUMER_NAME: &str = "open_collab_handle";
pub struct OpenCollabHandle {
  pub object_id: String,
  pub collab: Arc<RwLock<Collab>>,
  pub collab_type: CollabType,
  pub history: Arc<CollabHistory>,

  #[allow(dead_code)]
  /// The history persistence to save the history periodically.
  /// bind the lifetime to the handle.
  history_persistence: Option<Arc<HistoryPersistence>>,
}

impl OpenCollabHandle {
  pub async fn new(
    object_id: &str,
    doc_state: Vec<u8>,
    collab_type: CollabType,
    update_stream: Option<StreamGroup>,
    history_persistence: Option<Arc<HistoryPersistence>>,
  ) -> Result<Self, HistoryError> {
    // Must set skip_gc = true to avoid the garbage collection of the collab.
    let mut collab = Collab::new_with_source(
      CollabOrigin::Empty,
      object_id,
      DataSource::DocStateV1(doc_state),
      vec![],
      true,
    )?;
    collab.initialize();
    let collab = Arc::new(RwLock::new(collab));

    let object_id = object_id.to_string();
    let history =
      Arc::new(CollabHistory::new(&object_id, collab.clone(), collab_type.clone()).await);

    // Spawn a task to receive updates from the update stream.
    spawn_recv_update(&object_id, &collab_type, collab.clone(), update_stream).await?;

    // spawn a task periodically to save the history to the persistence.
    if let Some(persistence) = &history_persistence {
      spawn_save_history(Arc::downgrade(&history), Arc::downgrade(persistence));
    }

    Ok(Self {
      object_id,
      collab,
      collab_type,
      history,
      history_persistence,
    })
  }

  pub async fn history_state(&self) -> Result<HistoryStatePb, HistoryError> {
    let lock_guard = self.collab.read().await;
    let encode_collab =
      lock_guard.encode_collab_v1(|collab| self.collab_type.validate_require_data(collab))?;
    Ok(HistoryStatePb {
      object_id: self.object_id.clone(),
      doc_state: encode_collab.doc_state.to_vec(),
      doc_state_version: 1,
    })
  }

  pub async fn generate_history(&self) -> Result<(), HistoryError> {
    if let Some(history_persistence) = &self.history_persistence {
      save_history(self.history.clone(), history_persistence.clone()).await;
    }
    Ok(())
  }
}

/// Spawns an asynchronous task to continuously receive and process updates from a given update stream.
async fn spawn_recv_update(
  object_id: &str,
  collab_type: &CollabType,
  collab: Arc<RwLock<Collab>>,
  update_stream: Option<StreamGroup>,
) -> Result<(), HistoryError> {
  let mut update_stream = match update_stream {
    Some(stream) => stream,
    None => return Ok(()),
  };

  let interval_duration = Duration::from_secs(5);
  let object_id = object_id.to_string();
  let collab_type = collab_type.clone();

  if let Ok(stale_messages) = update_stream.get_unacked_messages(CONSUMER_NAME).await {
    let message_ids = stale_messages
      .iter()
      .map(|m| m.id.to_string())
      .collect::<Vec<_>>();

    // 1.Process the stale messages.
    if let Err(err) = process_messages(
      &mut update_stream,
      stale_messages,
      collab.clone(),
      &object_id,
      &collab_type,
    )
    .await
    {
      // 2.Clear the stale messages if failed to process them.
      if let Err(err) = update_stream.clear().await {
        error!("[History]: fail to clear stale update messages: {:?}", err);
      }
      return Err(HistoryError::ApplyStaleMessage(err.to_string()));
    }

    // 3.Acknowledge the stale messages.
    if let Err(err) = update_stream.ack_message_ids(message_ids).await {
      error!("[History ] fail to ack stale messages: {:?}", err);
    }
  }

  // spawn a task to receive updates from the update stream.
  let weak_collab = Arc::downgrade(&collab);
  tokio::spawn(async move {
    let mut interval = interval(interval_duration);
    loop {
      interval.tick().await;

      // Check if the mutex_collab is still alive. If not, break the loop.
      if let Some(collab) = weak_collab.upgrade() {
        if let Ok(messages) = update_stream
          .consumer_messages(CONSUMER_NAME, ReadOption::Undelivered)
          .await
        {
          if messages.is_empty() {
            continue;
          }

          trace!("[History] received {} update messages", messages.len());
          if let Err(e) = process_messages(
            &mut update_stream,
            messages,
            collab,
            &object_id,
            &collab_type,
          )
          .await
          {
            error!("Error processing update: {:?}", e);
          }
        }
      } else {
        // break the loop if the mutex_collab is dropped.
        break;
      }
    }
  });
  Ok(())
}

/// Processes messages from the update stream and applies them.
async fn process_messages(
  update_stream: &mut StreamGroup,
  messages: Vec<StreamMessage>,
  collab: Arc<RwLock<Collab>>,
  _object_id: &str,
  _collab_type: &CollabType,
) -> Result<(), HistoryError> {
  let mut lock = collab.write().await;
  apply_updates(&messages, &mut lock)?;
  drop(lock);
  update_stream.ack_messages(&messages).await?;
  Ok(())
}

/// Applies decoded updates from messages to the given locked collaboration object.
fn apply_updates(messages: &[StreamMessage], collab: &mut Collab) -> Result<(), HistoryError> {
  let mut txn = collab.transact_mut();
  for message in messages {
    let CollabUpdateEvent::UpdateV1 { encode_update } = CollabUpdateEvent::decode(&message.data)?;
    let update = Update::decode_v1(&encode_update)
      .map_err(|e| CollabError::YrsEncodeStateError(e.to_string()))?;
    txn
      .apply_update(update)
      .map_err(|err| HistoryError::Internal(err.into()))?;
  }
  Ok(())
}

fn spawn_save_history(history: Weak<CollabHistory>, history_persistence: Weak<HistoryPersistence>) {
  tokio::spawn(async move {
    let mut interval = if cfg!(debug_assertions) {
      // In debug mode, save the history every 10 seconds.
      interval(Duration::from_secs(10))
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
    Ok(None) => {},
    Err(err) => error!("Failed to generate snapshot context: {:?}", err),
  }
}
