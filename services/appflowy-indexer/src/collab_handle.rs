use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use collab::core::collab::MutexCollab;
use collab::core::collab::TransactionMutExt;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::Update;
use collab_entity::CollabType;
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinSet;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tracing::instrument;
use yrs::{Doc, Transact};

use appflowy_ai_client::client::AppFlowyAIClient;

use crate::document_index::DocumentWatcher;
use appflowy_ai_client::dto::Document as DocumentContent;
use collab_stream::client::CollabRedisStream;
use collab_stream::model::{CollabUpdateEvent, StreamMessage};
use collab_stream::stream_group::{ReadOption, StreamGroup};

use crate::error::Result;

const CONSUMER_NAME: &str = "open_collab_handle";

#[allow(dead_code)]
pub struct CollabHandle {
  content: Arc<dyn Indexable>,
  tasks: JoinSet<()>,
  closing: CancellationToken,
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
  ) -> Result<Option<Self>> {
    let closing = CancellationToken::new();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let content: Arc<dyn Indexable> = match collab_type {
      CollabType::Document => {
        let mut watcher = DocumentWatcher::new(object_id.clone(), workspace_id.clone(), doc_state)?;
        watcher.attach_listener(tx);
        Arc::new(watcher)
      },
      _ => return Ok(None),
    };

    let group_name = format!("indexer_{}:{}", workspace_id, object_id);
    let mut update_stream = redis_stream
      .collab_update_stream(&workspace_id, &object_id, &group_name)
      .await
      .unwrap();

    let messages = update_stream.get_unacked_messages(CONSUMER_NAME).await?;
    if !messages.is_empty() {
      Self::handle_collab_updates(&mut update_stream, content.get_collab(), messages).await?;
    }

    let mut tasks = JoinSet::new();
    tasks.spawn(Self::receive_collab_updates(
      update_stream,
      Arc::downgrade(&content),
      object_id.clone(),
      workspace_id.clone(),
      collab_type.clone(),
      ingest_interval,
      closing.clone(),
    ));
    tasks.spawn(Self::process_content_changes(
      rx,
      ai_client,
      object_id,
      workspace_id,
      collab_type,
      ingest_interval,
      closing.clone(),
    ));

    Ok(Some(Self {
      content,
      tasks,
      closing,
    }))
  }

  /// In regular time intervals, receive yrs updates and apply them to the locall in-memory collab
  /// representation. This should emit index content events, which we listen to on
  /// [Self::receive_index_events].
  async fn receive_collab_updates(
    mut update_stream: StreamGroup,
    content: Weak<dyn Indexable>,
    object_id: String,
    workspace_id: String,
    collab_type: CollabType,
    ingest_interval: Duration,
    closing: CancellationToken,
  ) {
    let mut interval = interval(ingest_interval);
    loop {
      select! {
        _ = closing.cancelled() => {
          tracing::trace!("closing signal received, stopping consumer");
          return;
        },
        _ = interval.tick() => {
          let result = update_stream
            .consumer_messages(CONSUMER_NAME, ReadOption::Count(100))
            .await;
          match result {
            Ok(messages) => {
              if let Some(content) = content.upgrade() {
                // check if we received empty message batch, if not: update the collab
                if !messages.is_empty() {
                  if let Err(err) = Self::handle_collab_updates(&mut update_stream, content.get_collab(), messages).await {
                    tracing::error!("failed to handle messages: {}", err);
                  }
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
    }
  }

  #[instrument(skip(update_stream, collab, messages), fields(messages = messages.len()))]
  async fn handle_collab_updates(
    update_stream: &mut StreamGroup,
    collab: &MutexCollab,
    messages: Vec<StreamMessage>,
  ) -> Result<()> {
    if let Some(collab) = collab.try_lock() {
      let mut txn = collab.try_transaction_mut()?;

      for message in &messages {
        match CollabUpdateEvent::decode(&message.data) {
          Ok(CollabUpdateEvent::UpdateV1 { encode_update }) => {
            let update = Update::decode_v1(&encode_update)?;
            txn.try_apply_update(update)?;
          },
          Err(err) => tracing::error!("failed to decode update event: {}", err),
        }
      }
      txn.commit();
    }
    update_stream.ack_messages(&messages).await?;

    Ok(())
  }

  async fn process_content_changes(
    mut rx: UnboundedReceiver<DocumentUpdate>,
    ai_client: AppFlowyAIClient,
    object_id: String,
    workspace_id: String,
    collab_type: CollabType,
    ingest_interval: Duration,
    token: CancellationToken,
  ) {
    let mut last_update = Instant::now();
    let mut inserts = HashMap::new();
    let mut removals = HashSet::new();
    loop {
      select! {
          _ = token.cancelled() => {
            tracing::trace!("closing signal received, stopping consumer");
            return;
          },
          Some(update) = rx.recv() => {
            match update {
              DocumentUpdate::Update(doc) => {
                inserts.insert(doc.id.clone(), doc);
              }
              DocumentUpdate::Removed(id) => {
                removals.insert(id);
              }
            }

            let now = Instant::now();
            if now.duration_since(last_update) > ingest_interval {
              let inserts: Vec<_> = inserts.drain().map(|(_,doc)| doc).collect();
              match ai_client.index_documents(&inserts).await {
                Ok(_) => last_update = now,
                Err(err) => tracing::error!("failed to index document {}/{}: {}", workspace_id, object_id, err),
              }
          }
        }
      }
    }
  }

  pub async fn shutdown(mut self) {
    let _ = self.closing.cancel();
    while let Some(_) = self.tasks.join_next().await { /* wait for all tasks to finish */ }
  }
}

pub trait Indexable: Send + Sync {
  fn get_collab(&self) -> &MutexCollab;
  fn attach_listener(&mut self, channel: UnboundedSender<DocumentUpdate>);
}

pub enum DocumentUpdate {
  Update(DocumentContent),
  Removed(String),
}

#[cfg(test)]
mod test {
  use std::time::Duration;

  use collab::core::transaction::DocTransactionExtension;
  use collab::preclude::Collab;
  use collab_entity::CollabType;
  use yrs::Map;

  use appflowy_ai_client::client::AppFlowyAIClient;
  use appflowy_ai_client::dto::SearchDocumentsRequest;

  use crate::collab_handle::CollabHandle;
  use crate::test_utils::{collab_update_forwarder, redis_stream};

  #[tokio::test]
  async fn test_indexing_pipeline() {
    let _ = env_logger::builder().is_test(true).try_init();

    let redis_stream = redis_stream().await;
    let workspace_id = uuid::Uuid::new_v4().to_string();
    let object_id = uuid::Uuid::new_v4().to_string();

    let mut collab = Collab::new(1, object_id.clone(), "device-1".to_string(), vec![], false);
    collab.initialize();
    let ai_client = AppFlowyAIClient::new("http://localhost:5001");

    let stream_group = redis_stream
      .collab_update_stream(&workspace_id, &object_id, "indexer")
      .await
      .unwrap();

    let _s = collab_update_forwarder(&mut collab, stream_group.clone());
    let doc = collab.get_doc();
    let init_state = doc.get_encoded_collab_v1().doc_state;
    let data_ref = doc.get_or_insert_map("data");
    collab.with_origin_transact_mut(|txn| {
      data_ref.insert(txn, "test-key", "test-value");
    });

    let _handle = CollabHandle::open(
      &redis_stream,
      ai_client.clone(),
      object_id.clone(),
      workspace_id.clone(),
      CollabType::Document,
      init_state.into(),
      Duration::from_millis(50),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(2000)).await;

    let docs = ai_client
      .search_documents(&SearchDocumentsRequest {
        workspaces: vec![workspace_id],
        query: "key".to_string(),
        result_count: None,
      })
      .await
      .unwrap();

    assert_eq!(docs.len(), 1);
    assert!(docs[0].content.contains("test-value"));
  }
}
