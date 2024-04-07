use crate::biz::casbin::pg_listen::{
  CollabMemberListener, CollabMemberNotification, WorkspaceMemberListener,
  WorkspaceMemberNotification,
};
use crate::biz::user::user_verify::UserListener;
use anyhow::Error;
use database::pg_row::AFUserNotification;
use serde::de::DeserializeOwned;
use sqlx::postgres::PgListener;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{error, trace};

pub struct PgListeners {
  user_listener: UserListener,
  workspace_member_listener: WorkspaceMemberListener,
  collab_member_listener: CollabMemberListener,
}

impl PgListeners {
  pub async fn new(pg_pool: &PgPool) -> Result<Self, Error> {
    let user_listener = UserListener::new(pg_pool, "af_user_channel").await?;

    let workspace_member_listener =
      WorkspaceMemberListener::new(pg_pool, "af_workspace_member_channel").await?;

    let collab_member_listener =
      CollabMemberListener::new(pg_pool, "af_collab_member_channel").await?;

    Ok(Self {
      user_listener,
      workspace_member_listener,
      collab_member_listener,
    })
  }

  pub fn subscribe_workspace_member_change(
    &self,
  ) -> broadcast::Receiver<WorkspaceMemberNotification> {
    self.workspace_member_listener.notify.subscribe()
  }

  pub fn subscribe_collab_member_change(&self) -> broadcast::Receiver<CollabMemberNotification> {
    self.collab_member_listener.notify.subscribe()
  }

  pub fn subscribe_user_change(&self, uid: i64) -> tokio::sync::mpsc::Receiver<AFUserNotification> {
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    let mut user_notify = self.user_listener.notify.subscribe();
    tokio::spawn(async move {
      while let Ok(notification) = user_notify.recv().await {
        if let Some(row) = notification.payload.as_ref() {
          if row.uid == uid {
            let _ = tx.send(notification).await;
          }
        }
      }
    });
    rx
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
    // TODO(nathan): using listen_all
    listener.listen(channel).await?;

    let (tx, _) = broadcast::channel(1000);
    let notify = tx.clone();
    tokio::spawn(async move {
      while let Ok(notification) = listener.recv().await {
        trace!("Received notification: {}", notification.payload());
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
