use crate::core::open_handle::OpenCollabHandle;
use collab_stream::client::CollabRedisStream;
use collab_stream::model::CollabControlEvent;
use collab_stream::stream_group::ReadOption;
use dashmap::mapref::entry::Entry;
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
    spawn_control_group(redis_stream.clone(), Arc::downgrade(&handles)).await;
    Self {
      handles,
      redis_stream,
    }
  }
}

async fn spawn_control_group(
  redis_stream: CollabRedisStream,
  handles: Weak<DashMap<String, OpenCollabHandle>>,
) {
  let mut control_group = redis_stream.collab_control_stream("history").await.unwrap();
  let mut interval = interval(Duration::from_secs(1));
  tokio::spawn(async move {
    loop {
      interval.tick().await;
      if let Ok(messages) = control_group
        .consumer_messages("open_collab", ReadOption::Count(10))
        .await
      {
        if let Some(handles) = handles.upgrade() {
          for message in &messages {
            if let Ok(event) = CollabControlEvent::decode(&message.data) {
              handle_control_event(&redis_stream, event, &handles).await;
            }
          }
          if let Err(err) = control_group.ack_messages(&messages).await {
            error!("Failed to ack messages: {:?}", err);
          }
        }
      }
    }
  });
}

async fn handle_control_event(
  redis_stream: &CollabRedisStream,
  event: CollabControlEvent,
  handles: &Arc<DashMap<String, OpenCollabHandle>>,
) {
  match event {
    CollabControlEvent::Open {
      workspace_id,
      object_id,
      collab_type,
      doc_state,
    } => match handles.entry(object_id.clone()) {
      Entry::Occupied(_) => {},
      Entry::Vacant(entry) => {
        trace!("Opening collab: {}", object_id);
        let group_name = format!("history_{}:{}", workspace_id, object_id);
        let update_stream = redis_stream
          .collab_update_stream(&workspace_id, &object_id, &group_name)
          .await
          .unwrap();
        match OpenCollabHandle::new(&object_id, doc_state, collab_type, Some(update_stream)) {
          Ok(handle) => {
            entry.insert(handle);
          },
          Err(err) => {
            error!("Failed to open collab: {:?}", err);
          },
        }
      },
    },
    CollabControlEvent::Close { object_id } => {
      trace!("Close collab: {}", object_id);
    },
  }
}
