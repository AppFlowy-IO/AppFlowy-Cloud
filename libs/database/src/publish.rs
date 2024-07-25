use app_error::AppError;
use database_entity::dto::{PublishCollabItem, PublishInfo};
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

pub async fn select_user_is_collab_publisher_for_all_views(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_uuid: &Uuid,
  view_ids: &[Uuid],
) -> Result<bool, AppError> {
  let count = sqlx::query_scalar!(
    r#"
      SELECT COUNT(*)
      FROM af_published_collab
      WHERE workspace_id = $1
        AND view_id = ANY($2)
        AND published_by = (SELECT uid FROM af_user WHERE uuid = $3)
    "#,
    workspace_uuid,
    view_ids,
    user_uuid,
  )
  .fetch_one(pg_pool)
  .await?;

  match count {
    Some(c) => Ok(c == view_ids.len() as i64),
    None => Ok(false),
  }
}

#[inline]
pub async fn select_workspace_publish_namespace_exists<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
  namespace: &str,
) -> Result<bool, AppError> {
  let res = sqlx::query_scalar!(
    r#"
      SELECT EXISTS(
        SELECT 1
        FROM af_workspace
        WHERE workspace_id = $1
          AND publish_namespace = $2
      )
    "#,
    workspace_id,
    namespace,
  )
  .fetch_one(executor)
  .await?;

  Ok(res.unwrap_or(false))
}

#[inline]
pub async fn update_workspace_publish_namespace<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
  new_namespace: &str,
) -> Result<(), AppError> {
  let res = sqlx::query!(
    r#"
      UPDATE af_workspace
      SET publish_namespace = $1
      WHERE workspace_id = $2
    "#,
    new_namespace,
    workspace_id,
  )
  .execute(executor)
  .await?;

  if res.rows_affected() != 1 {
    tracing::error!(
      "Failed to update workspace publish namespace, workspace_id: {}, new_namespace: {}, rows_affected: {}",
      workspace_id, new_namespace, res.rows_affected()
    );
  }

  Ok(())
}

#[inline]
pub async fn select_workspace_publish_namespace<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
) -> Result<String, AppError> {
  let res = sqlx::query_scalar!(
    r#"
      SELECT publish_namespace
      FROM af_workspace
      WHERE workspace_id = $1
    "#,
    workspace_id,
  )
  .fetch_one(executor)
  .await?;

  Ok(res)
}

#[inline]
pub async fn insert_or_replace_publish_collabs<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
  publisher_uuid: &Uuid,
  publish_item: &[PublishCollabItem<serde_json::Value, Vec<u8>>],
) -> Result<(), AppError> {
  let view_ids: Vec<Uuid> = publish_item.iter().map(|item| item.meta.view_id).collect();
  let publish_names: Vec<String> = publish_item
    .iter()
    .map(|item| item.meta.publish_name.clone())
    .collect();
  let metadatas: Vec<serde_json::Value> = publish_item
    .iter()
    .map(|item| item.meta.metadata.clone())
    .collect();

  let blobs: Vec<Vec<u8>> = publish_item.iter().map(|item| item.data.clone()).collect();
  let res = sqlx::query!(
    r#"
      INSERT INTO af_published_collab (workspace_id, view_id, publish_name, published_by, metadata, blob)
      SELECT * FROM UNNEST(
        (SELECT array_agg((SELECT $1::uuid)) FROM generate_series(1, $7))::uuid[],
        $2::uuid[],
        $3::text[],
        (SELECT array_agg((SELECT uid FROM af_user WHERE uuid = $4)) FROM generate_series(1, $7))::bigint[],
        $5::jsonb[],
        $6::bytea[]
      )
      ON CONFLICT (workspace_id, view_id) DO UPDATE
      SET metadata = EXCLUDED.metadata,
          blob = EXCLUDED.blob,
          published_by = EXCLUDED.published_by,
          publish_name = EXCLUDED.publish_name
    "#,
    workspace_id,
    &view_ids,
    &publish_names,
    publisher_uuid,
    &metadatas,
    &blobs,
    publish_item.len() as i32,
  )
  .execute(executor)
  .await?;

  if res.rows_affected() != publish_item.len() as u64 {
    tracing::warn!(
      "Failed to insert or replace publish collab meta batch, workspace_id: {}, publisher_uuid: {}, rows_affected: {}",
      workspace_id, publisher_uuid, res.rows_affected()
    );
  }

  Ok(())
}

#[inline]
pub async fn select_publish_collab_meta_for_view_id(
  txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
  view_id: &Uuid,
) -> Result<serde_json::Value, AppError> {
  let res = sqlx::query!(
    r#"
      SELECT metadata
      FROM af_published_collab
      WHERE view_id = $1
    "#,
    view_id,
  )
  .fetch_one(txn.as_mut())
  .await?;
  let metadata: serde_json::Value = res.metadata;
  Ok(metadata)
}

#[inline]
pub async fn select_publish_collab_meta<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  publish_namespace: &str,
  publish_name: &str,
) -> Result<serde_json::Value, AppError> {
  let res = sqlx::query!(
    r#"
    SELECT metadata
    FROM af_published_collab
    WHERE workspace_id = (SELECT workspace_id FROM af_workspace WHERE publish_namespace = $1)
      AND publish_name = $2
    "#,
    publish_namespace,
    publish_name,
  )
  .fetch_one(executor)
  .await?;
  let metadata: serde_json::Value = res.metadata;
  Ok(metadata)
}

#[inline]
pub async fn delete_published_collabs<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
  view_ids: &[Uuid],
) -> Result<(), AppError> {
  let res = sqlx::query!(
    r#"
      DELETE FROM af_published_collab
      WHERE workspace_id = $1
        AND view_id = ANY($2)
    "#,
    workspace_id,
    view_ids,
  )
  .execute(executor)
  .await?;

  if res.rows_affected() != view_ids.len() as u64 {
    tracing::error!(
      "Failed to delete published collabs, workspace_id: {}, view_ids: {:?}, rows_affected: {}",
      workspace_id,
      view_ids,
      res.rows_affected()
    );
  }

  Ok(())
}

#[inline]
pub async fn select_published_collab_doc_state_for_view_id(
  txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
  view_id: &Uuid,
) -> Result<Option<Vec<u8>>, AppError> {
  let res = sqlx::query_scalar!(
    r#"
      SELECT blob
      FROM af_published_collab
      WHERE view_id = $1
    "#,
    view_id,
  )
  .fetch_optional(txn.as_mut())
  .await?;
  Ok(res)
}

#[inline]
pub async fn select_published_collab_blob<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  publish_namespace: &str,
  publish_name: &str,
) -> Result<Vec<u8>, AppError> {
  let res = sqlx::query_scalar!(
    r#"
      SELECT blob
      FROM af_published_collab
      WHERE workspace_id = (SELECT workspace_id FROM af_workspace WHERE publish_namespace = $1)
      AND publish_name = $2
    "#,
    publish_namespace,
    publish_name,
  )
  .fetch_one(executor)
  .await?;

  Ok(res)
}

pub async fn select_published_collab_info<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  view_id: &Uuid,
) -> Result<PublishInfo, AppError> {
  let res = sqlx::query_as!(
    PublishInfo,
    r#"
      SELECT
        (SELECT publish_namespace FROM af_workspace aw WHERE aw.workspace_id = apc.workspace_id) AS namespace,
        publish_name,
        view_id
      FROM af_published_collab apc
      WHERE view_id = $1
    "#,
    view_id,
  )
  .fetch_one(executor)
  .await?;

  Ok(res)
}
