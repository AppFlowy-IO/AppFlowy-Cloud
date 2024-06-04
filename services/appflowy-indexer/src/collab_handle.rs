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
use futures::{Stream, StreamExt};
use tokio::select;
use tokio::task::JoinSet;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tracing::instrument;
use uuid::Uuid;

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
    let was_indexed = indexer.was_indexed(&object_id).await?;
    let content: Arc<dyn Indexable> = match collab_type {
      CollabType::Document => {
        let content = Document::from_doc_state(
          CollabOrigin::Empty,
          DataSource::DocStateV1(doc_state),
          &object_id,
          vec![],
        )?;
        let watcher = DocumentWatcher::new(object_id.clone(), content, !was_indexed)?;
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
    let workspace_id =
      Uuid::parse_str(&workspace_id).map_err(crate::error::Error::InvalidWorkspace)?;

    let mut tasks = JoinSet::new();
    tasks.spawn(Self::receive_collab_updates(
      update_stream,
      Arc::downgrade(&content),
      object_id.clone(),
      workspace_id,
      ingest_interval,
      closing.clone(),
    ));
    tasks.spawn(Self::process_content_changes(
      content.changes(),
      indexer,
      object_id,
      workspace_id,
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
    workspace_id: Uuid,
    ingest_interval: Duration,
    closing: CancellationToken,
  ) {
    let mut interval = interval(ingest_interval);
    loop {
      select! {
        _ = closing.cancelled() => {
          tracing::trace!("document {}/{} watcher cancelled, stopping.", workspace_id, object_id);
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
                    tracing::error!("document {}/{} watcher failed to handle updates: {}", workspace_id, object_id, err);
                  }
                }
              } else {
                tracing::trace!("collab dropped, stopping consumer");
                return;
              }
            },
            Err(err) => {
              tracing::error!("document {}/{} watcher failed to receive messages: {}", workspace_id, object_id, err);
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
    workspace_id: Uuid,
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
              match Self::publish_updates(&indexer, &workspace_id, &mut inserts, &mut removals).await {
                Ok(_) => last_update = Instant::now(),
                Err(err) => tracing::error!("document {}/{} watcher failed to publish fragment updates: {}", workspace_id, object_id, err),
              }
          }
          _ = token.cancelled() => {
            tracing::trace!("document {}/{} watcher closing signal received, flushing remaining updates", workspace_id, object_id);
            if let Err(err) = Self::publish_updates(&indexer, &workspace_id, &mut inserts, &mut removals).await {
              tracing::error!("document {}/{} watcher failed to publish fragment updates: {}", workspace_id, object_id, err);
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
              match Self::publish_updates(&indexer, &workspace_id, &mut inserts, &mut removals).await {
                Ok(_) => last_update = now,
                Err(err) => tracing::error!("document {}/{} watcher failed to publish fragment updates: {}", workspace_id, object_id, err),
              }
            }
        }
      }
    }
  }

  async fn publish_updates(
    indexer: &Arc<dyn Indexer>,
    workspace_id: &Uuid,
    inserts: &mut HashMap<FragmentID, Fragment>,
    removals: &mut HashSet<FragmentID>,
  ) -> Result<()> {
    if inserts.is_empty() && removals.is_empty() {
      return Ok(());
    }
    let inserts: Vec<_> = inserts.drain().map(|(_, doc)| doc).collect();
    if !inserts.is_empty() {
      tracing::info!("updating indexes for {} fragments", inserts.len());
      indexer.update_index(workspace_id, inserts).await?;
    }

    if !removals.is_empty() {
      tracing::info!("removing indexes for {} fragments", removals.len());
      indexer
        .remove(&removals.drain().collect::<Vec<_>>())
        .await?;
    }
    Ok(())
  }

  pub async fn shutdown(mut self) {
    self.closing.cancel();
    while self.tasks.join_next().await.is_some() { /* wait for all tasks to finish */ }
  }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum FragmentUpdate {
  Update(Fragment),
  Removed(FragmentID),
}

#[cfg(test)]
mod test {
  use std::collections::HashSet;
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
  use crate::test_utils::{
    collab_update_forwarder, db_pool, openai_client, redis_stream, setup_collab,
  };

  #[tokio::test]
  async fn test_indexing_for_new_initialized_document() {
    let _ = env_logger::builder().is_test(true).try_init();

    let redis_stream = redis_stream().await;
    let uid = rand::random();
    let object_id = uuid::Uuid::new_v4();

    let mut collab = Collab::new(
      uid,
      object_id.to_string(),
      "device-1".to_string(),
      vec![],
      false,
    );
    collab.initialize();
    let collab = Arc::new(MutexCollab::new(collab));
    let encoded_collab = {
      let doc_data = get_started_document_data().unwrap();
      let document = Document::create_with_data(collab.clone(), doc_data).unwrap();
      document.encode_collab().unwrap()
    };
    let db = db_pool().await;

    let workspace_id = setup_collab(&db, uid, object_id, &encoded_collab).await;

    let object_id = object_id.to_string();

    let openai = openai_client();
    let indexer: Arc<dyn Indexer> = Arc::new(PostgresIndexer::new(openai, db));

    let stream_group = redis_stream
      .collab_update_stream(&workspace_id.to_string(), &object_id, "indexer")
      .await
      .unwrap();

    let _s = collab_update_forwarder(collab, stream_group.clone());

    let _handle = CollabHandle::open(
      &redis_stream,
      indexer.clone(),
      object_id.clone(),
      workspace_id.to_string(),
      CollabType::Document,
      encoded_collab.doc_state.to_vec(),
      Duration::from_millis(50),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(2000)).await;

    let db = db_pool().await;

    let contents = sqlx::query("SELECT content from af_collab_embeddings WHERE oid = $1")
      .bind(&object_id)
      .fetch_all(&db)
      .await
      .unwrap();
    let contents = contents
      .into_iter()
      .map(|r| r.get::<String, _>("content"))
      .collect::<HashSet<_>>();

    assert_eq!(contents.len(), 1);

    let tokens: i64 =
      sqlx::query("SELECT index_token_usage from af_workspace WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(&db)
        .await
        .unwrap()
        .get(0);
    assert_ne!(tokens, 0);
  }
}
