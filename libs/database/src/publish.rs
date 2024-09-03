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
  publish_items: Vec<PublishCollabItem<serde_json::Value, Vec<u8>>>,
) -> Result<(), AppError> {
  let item_count = publish_items.len();
  let mut view_ids: Vec<Uuid> = Vec::with_capacity(item_count);
  let mut publish_names: Vec<String> = Vec::with_capacity(item_count);
  let mut metadatas: Vec<serde_json::Value> = Vec::with_capacity(item_count);
  let mut blobs: Vec<Vec<u8>> = Vec::with_capacity(item_count);
  publish_items.into_iter().for_each(|item| {
    view_ids.push(item.meta.view_id);
    publish_names.push(item.meta.publish_name);
    metadatas.push(item.meta.metadata);
    blobs.push(item.data);
  });

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
    item_count as i32,
  )
  .execute(executor)
  .await?;

  if res.rows_affected() != item_count as u64 {
    tracing::warn!(
      "Failed to insert or replace publish collab meta batch, workspace_id: {}, publisher_uuid: {}, rows_affected: {}, item_count: {}",
      workspace_id, publisher_uuid, res.rows_affected(), item_count
    );
  }

  Ok(())
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
pub async fn select_published_data_for_view_id(
  pg_pool: &PgPool,
  view_id: &Uuid,
) -> Result<Option<(serde_json::Value, Vec<u8>)>, AppError> {
  let res = sqlx::query!(
    r#"
      SELECT metadata, blob
      FROM af_published_collab
      WHERE view_id = $1
    "#,
    view_id,
  )
  .fetch_optional(pg_pool)
  .await?;
  Ok(res.map(|res| (res.metadata, res.blob)))
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
        aw.publish_namespace AS namespace,
        apc.publish_name,
        apc.view_id
      FROM af_published_collab apc
      LEFT JOIN af_workspace aw
        ON apc.workspace_id = aw.workspace_id
      WHERE apc.view_id = $1;
    "#,
    view_id,
  )
  .fetch_one(executor)
  .await?;

  Ok(res)
}

pub async fn select_workspace_id_for_publish_namespace<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  publish_namespace: &str,
) -> Result<Uuid, AppError> {
  let res = sqlx::query!(
    r#"
      SELECT workspace_id
      FROM af_workspace
      WHERE publish_namespace = $1
    "#,
    publish_namespace,
  )
  .fetch_one(executor)
  .await?;

  Ok(res.workspace_id)
}

pub async fn select_published_view_ids_for_workspace<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: Uuid,
) -> Result<Vec<Uuid>, AppError> {
  let res = sqlx::query!(
    r#"
      SELECT view_id
      FROM af_published_collab
      WHERE workspace_id = $1
    "#,
    workspace_id,
  )
  .fetch_all(executor)
  .await?;

  Ok(res.into_iter().map(|r| r.view_id).collect())
}
