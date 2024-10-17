use access_control::casbin::notification::WorkspaceMemberNotification;
use anyhow::Error;
use appflowy_collaborate::collab::notification::CollabMemberNotification;
use database::listener::PostgresDBListener;
use database::pg_row::AFUserNotification;
use sqlx::PgPool;

pub struct PgListeners {
  user_listener: UserListener,
}

impl PgListeners {
  pub async fn new(pg_pool: &PgPool) -> Result<Self, Error> {
    let user_listener = UserListener::new(pg_pool, "af_user_channel").await?;
    Ok(Self { user_listener })
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
