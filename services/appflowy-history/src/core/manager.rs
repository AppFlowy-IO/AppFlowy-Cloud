use crate::biz::persistence::HistoryPersistence;
use crate::core::open_handle::OpenCollabHandle;
use crate::error::HistoryError;
use collab_entity::CollabType;
use collab_stream::client::CollabRedisStream;
use collab_stream::model::CollabControlEvent;
use collab_stream::stream_group::ReadOption;
use dashmap::mapref::entry::Entry;

use crate::config::StreamSetting;
use dashmap::DashMap;
use database::history::ops::get_latest_snapshot;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tonic_proto::history::{HistoryStatePb, SingleSnapshotInfoPb, SnapshotRequestPb};
use tracing::{error, trace};
use uuid::Uuid;

const CONSUMER_NAME: &str = "open_collab";
pub struct OpenCollabManager {
  #[allow(dead_code)]
  handles: Arc<DashMap<String, Arc<OpenCollabHandle>>>,
  #[allow(dead_code)]
  redis_stream: CollabRedisStream,
}

impl OpenCollabManager {
  pub async fn new(
    redis_stream: CollabRedisStream,
    pg_pool: PgPool,
    setting: &StreamSetting,
  ) -> Self {
    let handles = Arc::new(DashMap::new());
    spawn_control_group(redis_stream.clone(), &handles, pg_pool, setting).await;
    Self {
      handles,
      redis_stream,
    }
  }

  pub async fn get_in_memory_history(
    &self,
    req: SnapshotRequestPb,
  ) -> Result<HistoryStatePb, HistoryError> {
    match self.handles.get(&req.object_id) {
      None => Err(HistoryError::RecordNotFound(req.object_id)),
      Some(handle) => handle.history_state().await,
    }
  }

  pub async fn get_latest_snapshot(
    &self,
    req: SnapshotRequestPb,
    pg_pool: &PgPool,
  ) -> Result<SingleSnapshotInfoPb, HistoryError> {
    let collab_type = CollabType::from(req.collab_type);
    match get_latest_snapshot(&req.object_id, &collab_type, pg_pool).await {
      Ok(Some(pb)) => Ok(pb),
      _ => Err(HistoryError::RecordNotFound(req.object_id)),
    }
  }
}

async fn spawn_control_group(
  redis_stream: CollabRedisStream,
  handles: &Arc<DashMap<String, Arc<OpenCollabHandle>>>,
  pg_pool: PgPool,
  setting: &StreamSetting,
) {
  let mut control_group = redis_stream
    .collab_control_stream(&setting.control_key, "history")
    .await
    .unwrap();

  // Handle stale messages
  if let Ok(stale_messages) = control_group.get_unacked_messages(CONSUMER_NAME).await {
    for message in &stale_messages {
      if let Ok(event) = CollabControlEvent::decode(&message.data) {
        handle_control_event(&redis_stream, event, handles, &pg_pool).await;
      }
    }

    if let Err(err) = control_group.ack_messages(&stale_messages).await {
      error!("Failed to ack stale messages: {:?}", err);
    }
  }

  let weak_handles = Arc::downgrade(handles);
  let mut interval = interval(Duration::from_secs(1));
  tokio::spawn(async move {
    loop {
      interval.tick().await;
      if let Ok(messages) = control_group
        .consumer_messages(CONSUMER_NAME, ReadOption::Count(10))
        .await
      {
        if let Some(handles) = weak_handles.upgrade() {
          if messages.is_empty() {
            continue;
          }

          trace!("[History] received {} control messages", messages.len());
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
  trace!("[History] received control event: {}", event);
  match event {
    CollabControlEvent::Open {
      workspace_id,
      object_id,
      collab_type,
      doc_state,
    } => match handles.entry(object_id.clone()) {
      Entry::Occupied(_) => {},
      Entry::Vacant(entry) => {
        trace!(
          "[History] create collab: {}, collab_type:{}",
          object_id,
          collab_type
        );
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
      trace!("[History] close collab: {}", object_id);
      if let Some(handle) = handles.get(&object_id) {
        if let Err(err) = handle.generate_history().await {
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
  .await
}
