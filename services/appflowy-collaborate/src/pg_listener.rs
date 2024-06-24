use crate::collab::notification::CollabMemberNotification;
use anyhow::Error;
use database::listener::PostgresDBListener;
use database::pg_row::AFUserNotification;
use sqlx::PgPool;
use tokio::sync::broadcast;
use workspace_access::notification::WorkspaceMemberNotification;

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

pub type CollabMemberListener = PostgresDBListener<CollabMemberNotification>;
pub type UserListener = PostgresDBListener<AFUserNotification>;
pub type WorkspaceMemberListener = PostgresDBListener<WorkspaceMemberNotification>;
