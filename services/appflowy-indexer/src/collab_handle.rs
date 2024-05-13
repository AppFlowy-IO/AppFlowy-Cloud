use crate::error::Result;
use appflowy_ai_client::client::AppFlowyAIClient;
use appflowy_ai_client::dto::Document;
use collab::core::collab::{
  DataSource, IndexContent, IndexContentReceiver, MutexCollab, TransactionMutExt, WeakMutexCollab,
};
use collab::core::origin::CollabOrigin;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{Collab, Update};
use collab_entity::CollabType;
use collab_stream::client::CollabRedisStream;
use collab_stream::model::{CollabUpdateEvent, StreamMessage};
use collab_stream::stream_group::{ReadOption, StreamGroup};
use std::time::{Duration, Instant};
use tokio::select;
use tokio::task::JoinSet;
use tokio::time::interval;
const CONSUMER_NAME: &str = "open_collab_handle";

#[allow(dead_code)]
pub struct CollabHandle {
  collab: MutexCollab,
  tasks: JoinSet<()>,
}

impl CollabHandle {
  pub(crate) async fn open(
    redis_stream: &CollabRedisStream,
    ai_client: AppFlowyAIClient,
    object_id: String,
    workspace_id: String,
    collab_type: CollabType,
    doc_state: Vec<u8>,
    ingest_interval: Duration,
  ) -> Result<Self> {
    let group_name = format!("indexer_{}:{}", workspace_id, object_id);
    let mut update_stream = redis_stream
      .collab_update_stream(&workspace_id, &object_id, &group_name)
      .await
      .unwrap();

    let messages = update_stream.get_unacked_messages(CONSUMER_NAME).await?;
    let (collab, index_content) = {
      let mut collab = Collab::new_with_source(
        CollabOrigin::Empty,
        &object_id,
        DataSource::DocStateV1(doc_state),
        vec![],
        false,
      )?;
      collab.initialize();
      let index_content = collab.subscribe_index_content();
      (MutexCollab::new(collab), index_content)
    };
    Self::handle_messages(&mut update_stream, &collab, messages).await?;
    let mut tasks = JoinSet::new();
    tasks.spawn(Self::receive_messages(
      update_stream,
      collab.downgrade(),
      ingest_interval,
    ));
    tasks.spawn(Self::receive_snapshots(
      index_content,
      ai_client,
      object_id,
      workspace_id,
      collab_type,
      ingest_interval,
    ));

    Ok(Self { collab, tasks })
  }

  async fn receive_messages(
    mut update_stream: StreamGroup,
    collab: WeakMutexCollab,
    ingest_interval: Duration,
  ) {
    let mut interval = interval(ingest_interval);
    loop {
      interval.tick().await;
      let result = update_stream
        .consumer_messages(CONSUMER_NAME, ReadOption::Count(100))
        .await;
      match result {
        Ok(messages) => {
          if let Some(collab) = collab.upgrade() {
            match Self::handle_messages(&mut update_stream, &collab, messages).await {
              Ok(_) => { /* no changes */ },
              Err(err) => tracing::error!("failed to handle messages: {}", err),
            }
          } else {
            tracing::trace!("collab dropped, stopping consumer");
            return;
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
              let update = Update::decode_v1(&encode_update)?;
              txn.try_apply_update(update)?;
            },
          }
        }
      }
      txn.commit();
    } else {
      tracing::error!("failed to lock collab");
    }
    update_stream.ack_messages(&messages).await?;

    Ok(())
  }
  async fn receive_snapshots(
    mut stream: IndexContentReceiver,
    client: AppFlowyAIClient,
    object_id: String,
    workspace_id: String,
    collab_type: CollabType,
    interval: Duration,
  ) {
    let mut last_update_time = Instant::now();
    let mut last_update = None;
    loop {
      select! {
        _ = tokio::time::sleep(interval) => {
          if let Some(update) = last_update.take() {
            // debounce: if we're flooded with a lot of updates in short timespan,
            // we only emit the snapshot indexing request on the most recent one
            match Self::handle_index_event(&client, update, object_id.clone(), workspace_id.clone(), &collab_type).await {
              Ok(_) => {
                last_update_time = Instant::now();
                last_update = None;
              },
              Err(err) => tracing::error!("failed to handle index event: {}", err),
            }
          }
        },
        result = stream.recv() => {
          match result {
            Err(_) => {
              tracing::trace!("index content stream for {}/{} has been closed", workspace_id, object_id);
              return;
            }
            Ok(update) => {
              let now = Instant::now();
              // check if enough time has passed since the last index update request
              if now - last_update_time >= interval {
                match Self::handle_index_event(&client, update, object_id.clone(), workspace_id.clone(), &collab_type).await {
                  Ok(_) => {
                    last_update_time = now;
                    last_update = None;
                  },
                  Err(err) => tracing::error!("failed to handle index event: {}", err),
                }
              } else {
                // not enough time has passed - just store the update for the debouncer to hit
                last_update = Some(update);
              }
            }
          }
        },
      }
    }
  }

  fn map_collab_type(collab_entity: &CollabType) -> Option<appflowy_ai_client::dto::CollabType> {
    match collab_entity {
      CollabType::Document => Some(appflowy_ai_client::dto::CollabType::Document),
      CollabType::Database => Some(appflowy_ai_client::dto::CollabType::Database),
      CollabType::WorkspaceDatabase => Some(appflowy_ai_client::dto::CollabType::WorkspaceDatabase),
      CollabType::Folder => Some(appflowy_ai_client::dto::CollabType::Folder),
      CollabType::DatabaseRow => Some(appflowy_ai_client::dto::CollabType::DatabaseRow),
      CollabType::UserAwareness => None,
      CollabType::Unknown => None,
    }
  }

  async fn handle_index_event(
    client: &AppFlowyAIClient,
    e: IndexContent,
    object_id: String,
    workspace_id: String,
    collab_type: &CollabType,
  ) -> Result<()> {
    match e {
      IndexContent::Create(json) | IndexContent::Update(json) => {
        if let Some(collab_type) = Self::map_collab_type(collab_type) {
          let document = Document {
            id: object_id,
            doc_type: collab_type,
            workspace_id,
            content: json.to_string(),
          };
          client.index_documents(&[document]).await?;
        } else {
          tracing::warn!(
            "this collab type indexing is not supported: {:?}",
            collab_type
          );
        }
      },
      IndexContent::Delete(_json) => {
        //FIXME: implement unindexing of the documents
        tracing::warn!("unindexing of the documents is not implemented yet");
      },
    }
    Ok(())
  }
}

impl Drop for CollabHandle {
  fn drop(&mut self) {
    self.tasks.abort_all()
  }
}
