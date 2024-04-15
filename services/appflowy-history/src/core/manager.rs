use crate::biz::persistence::HistoryPersistence;
use crate::core::open_handle::OpenCollabHandle;
use crate::error::HistoryError;
use collab_entity::CollabType;
use collab_stream::client::{CollabRedisStream, CONTROL_STREAM_KEY};
use collab_stream::model::CollabControlEvent;
use collab_stream::stream_group::ReadOption;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use sqlx::PgPool;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, trace};
use uuid::Uuid;

pub struct OpenCollabManager {
  #[allow(dead_code)]
  handles: Arc<DashMap<String, Arc<OpenCollabHandle>>>,
  #[allow(dead_code)]
  redis_stream: CollabRedisStream,
}

impl OpenCollabManager {
  pub async fn new(redis_stream: CollabRedisStream, pg_pool: PgPool) -> Self {
    let handles = Arc::new(DashMap::new());
    spawn_control_group(redis_stream.clone(), Arc::downgrade(&handles), pg_pool).await;
    Self {
      handles,
      redis_stream,
    }
  }
}

async fn spawn_control_group(
  redis_stream: CollabRedisStream,
  handles: Weak<DashMap<String, Arc<OpenCollabHandle>>>,
  pg_pool: PgPool,
) {
  let mut control_group = redis_stream
    .collab_control_stream(CONTROL_STREAM_KEY, "history")
    .await
    .unwrap();
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
              handle_control_event(&redis_stream, event, &handles, &pg_pool).await;
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
  handles: &Arc<DashMap<String, Arc<OpenCollabHandle>>>,
  pg_pool: &PgPool,
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
        match init_collab_handle(
          redis_stream,
          pg_pool,
          &workspace_id,
          &object_id,
          collab_type,
          doc_state,
        )
        .await
        {
          Ok(handle) => {
            let arc_handle = Arc::new(handle);
            entry.insert(arc_handle);
          },
          Err(err) => {
            error!("Failed to open collab: {:?}", err);
          },
        }
      },
    },
    CollabControlEvent::Close { object_id } => {
      trace!("Close collab: {}", object_id);
      if let Some(handle) = handles.get(&object_id) {
        if let Err(err) = handle.gen_history().await {
          error!(
            "Failed to generate history when receiving close event: {:?}",
            err
          );
        }
      }
    },
  }
}

#[inline]
async fn init_collab_handle(
  redis_stream: &CollabRedisStream,
  pg_pool: &PgPool,
  workspace_id: &String,
  object_id: &String,
  collab_type: CollabType,
  doc_state: Vec<u8>,
) -> Result<OpenCollabHandle, HistoryError> {
  trace!("Opening collab: {}", object_id);
  let group_name = format!("history_{}:{}", workspace_id, object_id);
  let update_stream = redis_stream
    .collab_update_stream(workspace_id, object_id, &group_name)
    .await
    .unwrap();

  let workspace_id =
    Uuid::parse_str(workspace_id).map_err(|err| HistoryError::Internal(err.into()))?;
  let persistence = Arc::new(HistoryPersistence::new(workspace_id, pg_pool.clone()));
  OpenCollabHandle::new(
    object_id,
    doc_state,
    collab_type,
    Some(update_stream),
    Some(persistence),
  )
}
