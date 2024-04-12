use crate::biz::open_handle::OpenCollabHandle;
use collab_stream::client::CollabRedisStream;
use collab_stream::model::{CollabControlEvent, StreamMessage};
use collab_stream::stream_group::{ConsumeOptions, StreamGroup};
use dashmap::DashMap;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, trace};

pub struct OpenCollabManager {
  handles: Arc<DashMap<String, OpenCollabHandle>>,
  redis_stream: CollabRedisStream,
}

impl OpenCollabManager {
  pub async fn new(redis_stream: CollabRedisStream) -> Self {
    let handles = Arc::new(DashMap::new());
    spawn_control_group(&redis_stream, Arc::downgrade(&handles)).await;
    Self {
      handles,
      redis_stream,
    }
  }
}

async fn spawn_control_group(
  redis_stream: &CollabRedisStream,
  handles: Weak<DashMap<String, OpenCollabHandle>>,
) {
  let mut control_group = redis_stream.collab_control_stream("history").await.unwrap();
  let mut interval = interval(Duration::from_secs(1));
  tokio::spawn(async move {
    loop {
      interval.tick();
      if let Ok(messages) = control_group
        .consumer_messages("open_collab", ConsumeOptions::Count(10))
        .await
      {
        if let Some(handles) = handles.upgrade() {
          let mut message_ids = vec![];
          for message in messages {
            if let Ok(event) = CollabControlEvent::decode(&message.raw_data) {
              handle_control_event(event, &handles).await;
            }
            message_ids.push(message.id.to_string());
          }
          if let Err(err) = control_group.ack_messages(&message_ids).await {
            error!("Failed to ack messages: {:?}", err);
          }
        }
      }
    }
  });
}

async fn handle_control_event(
  event: CollabControlEvent,
  handles: &Arc<DashMap<String, OpenCollabHandle>>,
) {
  match event {
    CollabControlEvent::Open {
      uid,
      object_id,
      collab_type,
    } => {
      trace!("Open collab: {}", object_id);
    },
    CollabControlEvent::Close { uid, object_id } => {
      trace!("Close collab: {}", object_id);
    },
  }
}
