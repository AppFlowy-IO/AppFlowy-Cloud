use std::time::Duration;

use database::notification::{
  select_recent_page_mentions, update_page_mention_notification_status,
};
use database_entity::dto::ProcessedPageMentionNotification;
use sqlx::PgPool;
use tokio::time::interval;

use crate::mailer::{AFCloudMailer, PageMentionNotificationMailerParam};

pub struct EmailNotificationWorker {
  pub pg_pool: PgPool,
  pub mailer: AFCloudMailer,
  pub notification_interval_seconds: u64,
  pub notification_grace_period_seconds: u64,
  pub appflowy_web_url: String,
}

impl EmailNotificationWorker {
  pub fn new(
    pg_pool: PgPool,
    mailer: AFCloudMailer,
    notification_interval_seconds: u64,
    notification_grace_period_seconds: u64,
    appflowy_web_url: &str,
  ) -> Self {
    Self {
      pg_pool,
      mailer,
      notification_interval_seconds,
      notification_grace_period_seconds,
      appflowy_web_url: appflowy_web_url.to_string(),
    }
  }

  pub async fn start_task(&self) {
    let mut interval = interval(Duration::from_secs(self.notification_interval_seconds));

    loop {
      interval.tick().await;
      self.send_page_notification_emails().await;
    }
  }

  async fn send_page_notification_emails(&self) {
    let default_mentioner_avatar_url =
      "https://cdn.pixabay.com/photo/2015/10/05/22/37/blank-profile-picture-973460_1280.png"
        .to_string();

    match select_recent_page_mentions(
      &self.pg_pool,
      self.notification_interval_seconds,
      self.notification_grace_period_seconds,
    )
    .await
    {
      Ok(page_mentions) => {
        let processed_mentions: Vec<ProcessedPageMentionNotification> = page_mentions
          .iter()
          .map(|pm| ProcessedPageMentionNotification {
            view_id: pm.view_id,
            person_id: pm.mentioned_person_id,
          })
          .collect();

        for mention in page_mentions {
          let mut page_url = format!(
            "{}/app/{}/{}",
            self.appflowy_web_url, mention.workspace_id, mention.view_id
          );
          if let Some(block_id) = mention.block_id {
            page_url.push_str(&format!("?blockId={}", block_id));
          }

          let param = PageMentionNotificationMailerParam {
            workspace_name: mention.workspace_name,
            mentioned_page_name: mention.view_name,
            mentioner_icon_url: default_mentioner_avatar_url.clone(),
            mentioner_name: mention.mentioner_name,
            mentioned_page_url: format!(
              "{}/app/{}/{}",
              self.appflowy_web_url, mention.workspace_id, mention.view_id
            ),
            mentioned_at: mention
              .mentioned_at
              .with_timezone(&chrono::Utc)
              .format("%b %d, %Y, %-I:%M %p (UTC)")
              .to_string(),
          };

          if let Err(err) = self
            .mailer
            .send_page_mention_notification(
              &mention.mentioned_person_name,
              &mention.mentioned_person_email,
              &param,
            )
            .await
          {
            tracing::error!(
              "Failed to send page mention notification email to {}: {}",
              &mention.mentioned_person_email,
              err
            );
          } else {
            tracing::debug!(
              "Sent page mention notification email to {}",
              &mention.mentioned_person_email
            );
          }
        }

        let update_result =
          update_page_mention_notification_status(&self.pg_pool, &processed_mentions).await;
        if update_result.is_err() {
          tracing::warn!(
            "Failed to update page mention notification status: {:?}",
            update_result.err()
          );
        } else {
          tracing::debug!("Successfully updated page mention notification status");
        }
      },
      Err(err) => {
        tracing::warn!("Failed to get recent page mention updates: {:?}", err);
      },
    }
  }
}
