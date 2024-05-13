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

type Handles = Arc<DashMap<String, Arc<CollabHandle>>>;

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
              tracing::error!("Failed to ack messages: {:?}", err);
            }
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
            collab_type,
            doc_state,
            ingest_interval,
          )
          .await;
          match result {
            Ok(handle) => {
              entry.insert(Arc::new(handle));
            },
            Err(err) => {
              tracing::error!(
                "failed to open collab handle for {}/{}: {}",
                object_id,
                workspace_id,
                err
              );
            },
          }
        },
      },
      CollabControlEvent::Close { object_id } => {
        handles.remove(&object_id);
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
