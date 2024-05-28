use crate::collab_handle::CollabHandle;
use crate::error::Result;
use crate::indexer::Indexer;
use collab_stream::client::CollabRedisStream;
use collab_stream::model::{CollabControlEvent, StreamMessage};
use collab_stream::stream_group::{ReadOption, StreamGroup};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::interval;

const CONSUMER_NAME: &str = "open_collab";

type Handles = Arc<DashMap<String, CollabHandle>>;

pub struct OpenCollabConsumer {
  #[allow(dead_code)]
  handles: Handles,
  consumer_group: tokio::task::JoinHandle<()>,
}

impl OpenCollabConsumer {
  pub(crate) async fn new(
    redis_stream: CollabRedisStream,
    indexer: Arc<dyn Indexer>,
    control_stream_key: &str,
    ingest_interval: Duration,
  ) -> Result<Self> {
    let handles = Arc::new(DashMap::new());
    let mut control_group = redis_stream
      .collab_control_stream(control_stream_key, "indexer")
      .await?;

    // Handle stale messages
    let stale_messages = control_group.get_unacked_messages(CONSUMER_NAME).await?;
    Self::handle_messages(
      &mut control_group,
      indexer.clone(),
      &redis_stream,
      handles.clone(),
      stale_messages,
      ingest_interval,
    )
    .await;

    let weak_handles = Arc::downgrade(&handles);
    let mut interval = interval(Duration::from_secs(1));
    let consumer_group = tokio::spawn(async move {
      loop {
        interval.tick().await;
        if let Ok(messages) = control_group
          .consumer_messages(CONSUMER_NAME, ReadOption::Count(10))
          .await
        {
          if let Some(handles) = weak_handles.upgrade() {
            for message in &messages {
              if let Ok(event) = CollabControlEvent::decode(&message.data) {
                Self::handle_event(
                  event,
                  &redis_stream,
                  indexer.clone(),
                  &handles,
                  ingest_interval,
                )
                .await;
              }
            }
            if let Err(err) = control_group.ack_messages(&messages).await {
              tracing::error!("failed to ack messages: {:?}", err);
            }
          } else {
            tracing::trace!("consumer handles dropped, exiting");
            return;
          }
        }
      }
    });

    Ok(Self {
      handles,
      consumer_group,
    })
  }

  #[inline]
  async fn handle_messages(
    control_group: &mut StreamGroup,
    indexer: Arc<dyn Indexer>,
    redis_stream: &CollabRedisStream,
    handles: Handles,
    messages: Vec<StreamMessage>,
    ingest_interval: Duration,
  ) {
    if messages.is_empty() {
      return;
    }

    tracing::debug!("received {} messages from Redis", messages.len());
    for message in &messages {
      if let Ok(event) = CollabControlEvent::decode(&message.data) {
        Self::handle_event(
          event,
          redis_stream,
          indexer.clone(),
          &handles,
          ingest_interval,
        )
        .await
      }
    }

    if let Err(err) = control_group.ack_messages(&messages).await {
      tracing::error!("failed to ack stale messages: {}", err);
    }
  }

  #[inline]
  async fn handle_event(
    event: CollabControlEvent,
    redis_stream: &CollabRedisStream,
    indexer: Arc<dyn Indexer>,
    handles: &Handles,
    ingest_interval: Duration,
  ) {
    match event {
      CollabControlEvent::Open {
        workspace_id,
        object_id,
        collab_type,
        doc_state,
      } => match handles.entry(object_id.clone()) {
        Entry::Occupied(_) => { /* do nothing */ },
        Entry::Vacant(entry) => {
          // create a new collab document handle, which will subscribe and apply incoming updates
          let result = CollabHandle::open(
            redis_stream,
            indexer,
            object_id.clone(),
            workspace_id.clone(),
            collab_type.clone(),
            doc_state,
            ingest_interval,
          )
          .await;
          match result {
            Ok(Some(handle)) => {
              entry.insert(handle);
              tracing::info!("created a new handle for {}/{}", workspace_id, object_id);
            },
            Ok(None) => {
              tracing::debug!(
                "document {}/{} of type {} is not indexable",
                workspace_id,
                object_id,
                collab_type
              );
            },
            Err(e) => {
              tracing::error!(
                "failed to open handle for {}/{}: {}",
                object_id,
                workspace_id,
                e
              );
            },
          }
        },
      },
      CollabControlEvent::Close { object_id } => {
        if let Some((_, handle)) = handles.remove(&object_id) {
          // trigger shutdown signal and gracefully wait for handle to complete
          tracing::info!("shutting down handle for {}", object_id);
          handle.shutdown().await;
        }
      },
    }
  }
}

impl Future for OpenCollabConsumer {
  type Output = ();

  fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let consumer_group = Pin::new(&mut self.consumer_group);
    match consumer_group.poll(cx) {
      Poll::Ready(Ok(())) => Poll::Ready(()),
      Poll::Ready(Err(err)) => {
        tracing::error!("consumer group failed: {}", err);
        Poll::Ready(())
      },
      Poll::Pending => Poll::Pending,
    }
  }
}

impl Drop for OpenCollabConsumer {
  fn drop(&mut self) {
    self.consumer_group.abort();
  }
}

#[cfg(test)]
mod test {
  use crate::consumer::OpenCollabConsumer;
  use crate::indexer::PostgresIndexer;
  use crate::test_utils::{
    collab_update_forwarder, db_pool, openai_client, redis_stream, setup_collab,
  };
  use collab::core::collab::MutexCollab;
  use collab::preclude::Collab;
  use collab_document::document::Document;
  use collab_document::document_data::default_document_data;
  use collab_entity::CollabType;
  use collab_stream::model::CollabControlEvent;
  use serde_json::json;
  use sqlx::Row;
  use std::sync::Arc;
  use std::time::Duration;

  #[tokio::test]
  async fn graceful_handle_shutdown() {
    let _ = env_logger::builder().is_test(true).try_init();

    let redis_stream = redis_stream().await;
    let uid = rand::random();
    let object_id = uuid::Uuid::new_v4();

    let db = db_pool().await;
    let openai = openai_client();

    let mut collab = Collab::new(
      uid,
      object_id.to_string(),
      "device-1".to_string(),
      vec![],
      false,
    );
    collab.initialize();
    let collab = Arc::new(MutexCollab::new(collab));
    let doc_data = default_document_data();

    let text_id = doc_data
      .meta
      .text_map
      .as_ref()
      .unwrap()
      .iter()
      .next()
      .unwrap()
      .0
      .clone();
    let document = Document::create_with_data(collab.clone(), doc_data).unwrap();
    let doc_state: Vec<u8> = document.encode_collab().unwrap().doc_state.into();

    let workspace_id = setup_collab(&db, uid, object_id, doc_state.clone()).await;

    let indexer = Arc::new(PostgresIndexer::new(openai, db.clone()));

    let mut control_stream = redis_stream
      .collab_control_stream("af_collab_control", "indexer")
      .await
      .unwrap();

    let update_stream = redis_stream
      .collab_update_stream(&workspace_id.to_string(), &object_id.to_string(), "indexer")
      .await
      .unwrap();

    let _s = collab_update_forwarder(collab.clone(), update_stream.clone());
    let consumer = OpenCollabConsumer::new(
      redis_stream,
      indexer.clone(),
      "af_collab_control",
      Duration::from_secs(1), // interval longer than test timeout
    )
    .await
    .unwrap();

    control_stream
      .insert_message(CollabControlEvent::Open {
        workspace_id: workspace_id.to_string(),
        object_id: object_id.to_string(),
        collab_type: CollabType::Document,
        doc_state,
      })
      .await
      .unwrap();

    document.apply_text_delta(&text_id, json!([{"insert": "test-value"}]).to_string());

    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
      consumer.handles.contains_key(&object_id.to_string()),
      "in reaction to open control event, a corresponding handle should be created"
    );

    control_stream
      .insert_message(CollabControlEvent::Close {
        object_id: object_id.to_string(),
      })
      .await
      .unwrap();

    tokio::time::sleep(Duration::from_millis(2000)).await;
    assert!(
      !consumer.handles.contains_key(&object_id.to_string()),
      "in reaction to close control event, a corresponding handle should be destroyed"
    );

    let contents = sqlx::query("SELECT content from af_collab_embeddings WHERE oid = $1")
      .bind(object_id.to_string())
      .fetch_all(&db)
      .await
      .unwrap();

    assert_ne!(contents.len(), 0);
    let content: Option<String> = contents[0].get(0);
    assert_eq!(content.as_deref(), Some("test-value\n"));
  }
}
