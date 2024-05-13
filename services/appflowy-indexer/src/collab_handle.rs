use collab::core::collab::{DataSource, MutexCollab, TransactionMutExt};
use collab::core::origin::CollabOrigin;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{Collab, Update};
use collab_entity::CollabType;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::interval;
use uuid::Uuid;

use collab_stream::client::CollabRedisStream;
use collab_stream::model::{CollabUpdateEvent, StreamMessage};
use collab_stream::stream_group::{ReadOption, StreamGroup};

use crate::error::{Error, Result};
const CONSUMER_NAME: &str = "open_collab_handle";

pub struct CollabHandle {
  handle: JoinHandle<()>,
}

impl CollabHandle {
  pub(crate) async fn open(
    redis_stream: &CollabRedisStream,
    object_id: &str,
    workspace_id: &str,
    collab_type: CollabType,
    doc_state: Vec<u8>,
  ) -> Result<Self> {
    let group_name = format!("indexer_{}:{}", workspace_id, object_id);
    let mut update_stream = redis_stream
      .collab_update_stream(workspace_id, object_id, &group_name)
      .await
      .unwrap();

    let workspace_id = match Uuid::parse_str(workspace_id) {
      Ok(workspace_id) => workspace_id,
      Err(_) => return Err(Error::InvalidWorkspaceId(workspace_id.to_string())),
    };

    let messages = update_stream.get_unacked_messages(CONSUMER_NAME).await?;
    let collab = {
      let mut collab = Collab::new_with_source(
        CollabOrigin::Empty,
        object_id,
        DataSource::DocStateV1(doc_state),
        vec![],
        false,
      )?;
      collab.initialize();
      MutexCollab::new(collab)
    };
    Self::handle_messages(&mut update_stream, &collab, messages).await?;
    let handle = tokio::spawn(Self::receive_messages(update_stream, collab));

    Ok(Self { handle })
  }

  async fn receive_messages(mut update_stream: StreamGroup, collab: MutexCollab) {
    let mut interval = interval(Duration::from_secs(15));
    loop {
      interval.tick().await;
      let result = update_stream
        .consumer_messages(CONSUMER_NAME, ReadOption::Count(10))
        .await;
      match result {
        Ok(messages) => {
          if let Err(err) = Self::handle_messages(&mut update_stream, &collab, messages).await {
            tracing::error!("failed to handle messages: {}", err);
          }
        },
        Err(err) => {
          tracing::error!("failed to receive messages: {}", err);
        },
      }
    }
  }

  async fn handle_messages(
    update_stream: &mut StreamGroup,
    collab: &MutexCollab,
    messages: Vec<StreamMessage>,
  ) -> Result<()> {
    if messages.is_empty() {
      return Ok(());
    }
    tracing::trace!("received {} messages", messages.len());
    if let Some(collab) = collab.try_lock() {
      let mut txn = collab.try_transaction_mut()?;

      for message in &messages {
        if let Ok(event) = CollabUpdateEvent::decode(&message.data) {
          match event {
            CollabUpdateEvent::UpdateV1 { encode_update } => {
              let update = Update::decode_v1(&*encode_update)?;
              txn.try_apply_update(update)?;
            },
          }
        }
      }
    } else {
      tracing::error!("failed to lock collab");
    }
    update_stream.ack_messages(&messages).await?;

    Ok(())
  }
}

impl Drop for CollabHandle {
  fn drop(&mut self) {
    self.handle.abort();
  }
}
