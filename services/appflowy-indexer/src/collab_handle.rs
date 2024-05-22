use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use collab::core::collab::TransactionMutExt;
use collab::core::collab::{DataSource, MutexCollab};
use collab::core::origin::CollabOrigin;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::Update;
use collab_document::document::Document;
use collab_entity::CollabType;
use futures::{Stream, StreamExt, TryFutureExt};
use tokio::select;
use tokio::task::JoinSet;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use collab_stream::client::CollabRedisStream;
use collab_stream::model::{CollabUpdateEvent, StreamMessage};
use collab_stream::stream_group::{ReadOption, StreamGroup};

use crate::error::Result;
use crate::indexer::{Fragment, FragmentID, Indexer};
use crate::watchers::DocumentWatcher;

const CONSUMER_NAME: &str = "open_collab_handle";

pub trait Indexable: Send + Sync {
  fn get_collab(&self) -> &MutexCollab;
  fn changes(&self) -> Pin<Box<dyn Stream<Item = FragmentUpdate> + Send + Sync>>;
}

#[allow(dead_code)]
pub struct CollabHandle {
  content: Arc<dyn Indexable>,
  tasks: JoinSet<()>,
  closing: CancellationToken,
}

impl CollabHandle {
  pub(crate) async fn open(
    redis_stream: &CollabRedisStream,
    indexer: Arc<dyn Indexer>,
    object_id: String,
    workspace_id: String,
    collab_type: CollabType,
    doc_state: Vec<u8>,
    ingest_interval: Duration,
  ) -> Result<Option<Self>> {
    let closing = CancellationToken::new();
    let content: Arc<dyn Indexable> = match collab_type {
      CollabType::Document => {
        let content = Document::from_doc_state(
          CollabOrigin::Empty,
          DataSource::DocStateV1(doc_state),
          &object_id,
          vec![],
        )?;
        let watcher = DocumentWatcher::new(object_id.clone(), workspace_id.clone(), content, true)?;
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
      content.changes(),
      indexer,
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
    mut updates: Pin<Box<dyn Stream<Item = FragmentUpdate> + Send + Sync>>,
    indexer: Arc<dyn Indexer>,
    object_id: String,
    workspace_id: String,
    collab_type: CollabType,
    ingest_interval: Duration,
    token: CancellationToken,
  ) {
    let mut last_update = Instant::now();
    let mut inserts = HashMap::new();
    let mut removals = HashSet::new();
    let mut interval = interval(ingest_interval);
    loop {
      select! {
          _ = interval.tick() => {
              match Self::publish_updates(&indexer, &mut inserts, &mut removals).await {
                Ok(_) => last_update = Instant::now(),
                Err(err) => tracing::error!("failed to publish fragment updates: {}", err),
              }
          }
          _ = token.cancelled() => {
            tracing::trace!("closing signal received, flushing remaining updates");
            if let Err(err) = Self::publish_updates(&indexer, &mut inserts, &mut removals).await {
              tracing::error!("failed to publish fragment updates: {}", err);
            }
            return;
          },
          Some(update) = updates.next() => {
            match update {
              FragmentUpdate::Update(doc) => {
                if doc.content.is_empty() {
                  // we count empty blocks as removals
                  removals.insert(doc.fragment_id.clone());
                } else {
                  inserts.insert(doc.fragment_id.clone(), doc);
                }
              }
              FragmentUpdate::Removed(id) => {
                removals.insert(id);
              }
            }

            let now = Instant::now();
            if now.duration_since(last_update) > ingest_interval {
              match Self::publish_updates(&indexer, &mut inserts, &mut removals).await {
                Ok(_) => last_update = now,
                Err(err) => tracing::error!("failed to publish fragment updates: {}", err),
              }
            }
        }
      }
    }
  }

  async fn publish_updates(
    indexer: &Arc<dyn Indexer>,
    inserts: &mut HashMap<FragmentID, Fragment>,
    removals: &mut HashSet<FragmentID>,
  ) -> Result<()> {
    if inserts.is_empty() && removals.is_empty() {
      return Ok(());
    }
    let inserts: Vec<_> = inserts.drain().map(|(_, doc)| doc).collect();
    tracing::debug!("updating indexes for {} fragments", inserts.len());
    indexer.update_index(inserts).await?;

    tracing::debug!("removing indexes for {} fragments", removals.len());
    indexer
      .remove(&removals.drain().collect::<Vec<_>>())
      .await?;
    Ok(())
  }

  pub async fn shutdown(mut self) {
    let _ = self.closing.cancel();
    while let Some(_) = self.tasks.join_next().await { /* wait for all tasks to finish */ }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FragmentUpdate {
  Update(Fragment),
  Removed(FragmentID),
}

impl FragmentUpdate {
  pub fn fragment_id(&self) -> &FragmentID {
    match self {
      FragmentUpdate::Update(doc) => &doc.fragment_id,
      FragmentUpdate::Removed(id) => id,
    }
  }
}

#[cfg(test)]
mod test {
  use std::sync::Arc;
  use std::time::Duration;

  use collab::core::collab::MutexCollab;
  use collab::preclude::Collab;
  use collab_document::document::Document;
  use collab_entity::CollabType;
  use sqlx::Row;

  use workspace_template::document::get_started::get_started_document_data;

  use crate::collab_handle::CollabHandle;
  use crate::indexer::{Indexer, PostgresIndexer};
  use crate::test_utils::{collab_update_forwarder, db_pool, openai_client, redis_stream};

  #[tokio::test]
  async fn test_indexing_for_new_initialized_document() {
    let _ = env_logger::builder().is_test(true).try_init();

    let redis_stream = redis_stream().await;
    let workspace_id = uuid::Uuid::new_v4().to_string();
    let object_id = uuid::Uuid::new_v4().to_string();

    let mut collab = Collab::new(1, object_id.clone(), "device-1".to_string(), vec![], false);
    collab.initialize();
    let db = db_pool().await;
    let openai = openai_client();
    let indexer: Arc<dyn Indexer> = Arc::new(PostgresIndexer::new(openai, db));

    let stream_group = redis_stream
      .collab_update_stream(&workspace_id, &object_id, "indexer")
      .await
      .unwrap();

    let _s = collab_update_forwarder(&mut collab, stream_group.clone());

    let doc_data = get_started_document_data().unwrap();
    let fragments_count = doc_data
      .meta
      .text_map
      .as_ref()
      .unwrap()
      .iter()
      .filter(|(_, v)| !v.is_empty())
      .count();
    let document =
      Document::create_with_data(Arc::new(MutexCollab::new(collab)), doc_data).unwrap();
    let init_state = document.encode_collab().unwrap().doc_state;

    let _handle = CollabHandle::open(
      &redis_stream,
      indexer.clone(),
      object_id.clone(),
      workspace_id.clone(),
      CollabType::Document,
      init_state.into(),
      Duration::from_millis(50),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(2000)).await;

    let db = db_pool().await;

    let tx = db.begin().await.unwrap();
    let records = sqlx::query!(
      "SELECT oid FROM af_collab_embeddings WHERE oid = $1",
      object_id
    )
    .fetch_all(&db)
    .await
    .unwrap();

    let contents =
      sqlx::query("SELECT content, embedding from af_collab_embeddings WHERE oid = $1")
        .bind(&object_id)
        .fetch_all(&db)
        .await
        .unwrap();

    assert_eq!(contents.len(), fragments_count);
  }
}
