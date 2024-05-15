use crate::collab_handle::CollabHandle;
use crate::error::Result;
use appflowy_ai_client::client::AppFlowyAIClient;
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
    ai_client: AppFlowyAIClient,
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
      &ai_client,
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
                Self::handle_event(event, &redis_stream, &ai_client, &handles, ingest_interval)
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
    ai_client: &AppFlowyAIClient,
    redis_stream: &CollabRedisStream,
    handles: Handles,
    messages: Vec<StreamMessage>,
    ingest_interval: Duration,
  ) {
    if messages.is_empty() {
      return;
    }

    tracing::trace!("received {} messages", messages.len());
    for message in &messages {
      if let Ok(event) = CollabControlEvent::decode(&message.data) {
        Self::handle_event(event, redis_stream, ai_client, &handles, ingest_interval).await
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
    ai_client: &AppFlowyAIClient,
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
            ai_client.clone(),
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
              tracing::trace!("created a new handle for {}/{}", workspace_id, object_id);
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
          tracing::trace!("shutting down handle for {}", object_id);
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
  use crate::test_utils::{collab_update_forwarder, redis_stream};
  use appflowy_ai_client::client::AppFlowyAIClient;
  use appflowy_ai_client::dto::SearchDocumentsRequest;
  use collab::core::transaction::DocTransactionExtension;
  use collab::preclude::Collab;
  use collab_entity::CollabType;
  use collab_stream::model::CollabControlEvent;
  use std::time::Duration;
  use yrs::Map;

  #[tokio::test]
  async fn graceful_handle_shutdown() {
    let _ = env_logger::builder().is_test(true).try_init();

    let redis_stream = redis_stream().await;
    let workspace_id = uuid::Uuid::new_v4().to_string();
    let object_id = uuid::Uuid::new_v4().to_string();

    let mut collab = Collab::new(1, object_id.clone(), "device-1".to_string(), vec![], false);
    collab.initialize();
    let ai_client = AppFlowyAIClient::new("http://localhost:5001");

    let mut control_stream = redis_stream
      .collab_control_stream("af_collab_control", "indexer")
      .await
      .unwrap();

    let update_stream = redis_stream
      .collab_update_stream(&workspace_id, &object_id, "indexer")
      .await
      .unwrap();

    let _s = collab_update_forwarder(&mut collab, update_stream.clone());
    let doc = collab.get_doc();
    let init_state = doc.get_encoded_collab_v1().doc_state;
    let data_ref = doc.get_or_insert_map("data");
    let consumer = OpenCollabConsumer::new(
      redis_stream,
      ai_client.clone(),
      "af_collab_control",
      Duration::from_secs(1), // interval longer than test timeout
    )
    .await
    .unwrap();

    control_stream
      .insert_message(CollabControlEvent::Open {
        workspace_id: workspace_id.clone(),
        object_id: object_id.clone(),
        collab_type: CollabType::Document,
        doc_state: init_state.into(),
      })
      .await
      .unwrap();

    collab.with_origin_transact_mut(|txn| {
      data_ref.insert(txn, "test-key", "test-value");
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
      consumer.handles.contains_key(&object_id),
      "in reaction to open control event, a corresponding handle should be created"
    );

    control_stream
      .insert_message(CollabControlEvent::Close {
        object_id: object_id.clone(),
      })
      .await
      .unwrap();

    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
      !consumer.handles.contains_key(&object_id),
      "in reaction to close control event, a corresponding handle should be destroyed"
    );

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
