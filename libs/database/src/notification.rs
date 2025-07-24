use std::time::Duration;

use app_error::AppError;
use database_entity::dto::{PageMentionNotification, ProcessedPageMentionNotification};
use sqlx::{postgres::types::PgInterval, Executor, Postgres, QueryBuilder};

pub async fn select_recent_page_mentions<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  recency_seconds: u64,
  grace_period_seconds: u64, // To cover for cases where previous batch of processing didn't succeed due to server restart or other issues.
) -> Result<Vec<PageMentionNotification>, AppError> {
  let interval = PgInterval {
    months: 0,
    days: 0,
    microseconds: Duration::from_secs(recency_seconds + grace_period_seconds).as_micros() as i64,
  };
  // Use FOR UPDATE SKIP LOCKED in case there is multiple instances of appflowy cloud running.
  // For instance, when both old and new server version are running during new deployment.
  let recent_page_mentions = sqlx::query_as!(
    PageMentionNotification,
    r#"
      SELECT
        w.workspace_name AS "workspace_name!",
        pm.workspace_id,
        pm.view_id,
        pm.view_name,
        mentioner.name AS "mentioner_name!",
        mentioner.metadata ->> 'icon_url' AS "mentioner_avatar_url",
        pm.person_id AS "mentioned_person_id",
        mentioned_person.name AS "mentioned_person_name!",
        mentioned_person.email AS "mentioned_person_email!",
        pm.mentioned_at AS "mentioned_at!",
        pm.block_id
      FROM af_page_mention AS pm
      JOIN af_workspace AS w ON pm.workspace_id = w.workspace_id
      JOIN af_user AS mentioned_person
        ON pm.person_id = mentioned_person.uuid
      JOIN af_user AS mentioner
        ON pm.mentioned_by = mentioner.uid
      WHERE pm.mentioned_at > NOW() - $1::INTERVAL
      AND require_notification
      AND NOT notified
      FOR UPDATE SKIP LOCKED
    "#,
    interval
  )
  .fetch_all(executor)
  .await?;
  Ok(recent_page_mentions)
}

pub async fn update_page_mention_notification_status<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  successful_notifications: &[ProcessedPageMentionNotification],
) -> Result<(), AppError> {
  if successful_notifications.is_empty() {
    return Ok(());
  }

  let mut builder: QueryBuilder<Postgres> =
    QueryBuilder::new("UPDATE af_page_mention SET notified = TRUE WHERE (view_id, person_id) IN (");
  builder.push_tuples(successful_notifications, |mut b, notified| {
    b.push_bind(notified.view_id).push_bind(notified.person_id);
  });
  builder.push(")");
  builder.build().execute(executor).await?;
  Ok(())
}
