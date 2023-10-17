use crate::biz::collab::member_listener::{CollabMemberChange, CollabMemberListener};
use crate::biz::workspace::member_listener::{WorkspaceMemberChange, WorkspaceMemberListener};
use anyhow::Error;
use serde::de::DeserializeOwned;
use sqlx::postgres::PgListener;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::error;

pub struct PgListeners {
  workspace_member_listener: WorkspaceMemberListener,
  collab_member_listener: CollabMemberListener,
}

impl PgListeners {
  pub async fn new(pg_pool: &PgPool) -> Result<Self, Error> {
    let workspace_member_listener =
      WorkspaceMemberListener::new(pg_pool, "af_workspace_member_channel").await?;

    let collab_member_listener =
      CollabMemberListener::new(pg_pool, "af_collab_member_channel").await?;

    Ok(Self {
      workspace_member_listener,
      collab_member_listener,
    })
  }

  pub fn subscribe_workspace_member_change(&self) -> broadcast::Receiver<WorkspaceMemberChange> {
    self.workspace_member_listener.notify.subscribe()
  }

  pub fn subscribe_collab_member_change(&self) -> broadcast::Receiver<CollabMemberChange> {
    self.collab_member_listener.notify.subscribe()
  }
}

pub struct PostgresDBListener<T: Clone> {
  notify: broadcast::Sender<T>,
}

impl<T> PostgresDBListener<T>
where
  T: Clone + DeserializeOwned + Send + 'static,
{
  pub async fn new(pg_pool: &PgPool, channel: &str) -> Result<Self, Error> {
    let mut listener = PgListener::connect_with(pg_pool).await?;
    listener.listen(channel).await?;

    let (tx, _) = broadcast::channel(1000);
    let notify = tx.clone();
    tokio::spawn(async move {
      while let Ok(notification) = listener.recv().await {
        match serde_json::from_str::<T>(notification.payload()) {
          Ok(change) => {
            let _ = tx.send(change);
          },
          Err(err) => {
            error!(
              "Failed to deserialize change: {:?}, payload: {}",
              err,
              notification.payload()
            );
          },
        }
      }
    });
    Ok(Self { notify })
  }
}
