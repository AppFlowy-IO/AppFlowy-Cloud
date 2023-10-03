use database::{
  collaborate::{self, collab_exists},
  user,
};
use database_entity::{AFCollabSnapshots, QueryObjectSnapshotParams, QuerySnapshotParams};
use redis::aio::ConnectionManager;
use shared_entity::{
  dto::{DeleteCollabParams, InsertCollabParams, QueryCollabParams},
  error::AppError,
  error_code::ErrorCode,
};
use sqlx::{types::Uuid, PgPool};
use validator::Validate;

pub async fn create_collab(
  pg_pool: &PgPool,
  redis_client: ConnectionManager,
  user_uuid: &Uuid,
  params: &InsertCollabParams,
) -> Result<(), AppError> {
  params.validate()?;

  // TODO: access control for user_uuid

  if collab_exists(pg_pool, &params.object_id).await? {
    return Err(ErrorCode::RecordAlreadyExists.into());
  }

  let workspace_uuid = params.workspace_id.parse::<Uuid>()?;
  let owner_uid = user::uid_from_uuid(pg_pool, user_uuid).await?;
  let mut tx = pg_pool.begin().await?;

  collaborate::insert_af_collab(
    &mut tx,
    redis_client,
    &workspace_uuid,
    owner_uid,
    &params.object_id,
    &params.raw_data,
    &params.collab_type,
  )
  .await?;
  Ok(())
}

pub async fn get_collab_raw(
  pg_pool: &PgPool,
  redis_client: ConnectionManager,
  _user_uuid: &Uuid,
  params: QueryCollabParams,
) -> Result<Vec<u8>, AppError> {
  params.validate()?;

  // TODO: access control for user_uuid

  let data = collaborate::get_collab_blob_cached(
    pg_pool,
    redis_client,
    &params.collab_type,
    &params.object_id,
  )
  .await?;
  Ok(data)
}

pub async fn update_collab(
  pg_pool: &PgPool,
  redis_client: ConnectionManager,
  user_uuid: &Uuid,
  params: &InsertCollabParams,
) -> Result<(), AppError> {
  params.validate()?;

  // TODO: access control for user_uuid

  let workspace_uuid = params.workspace_id.parse::<Uuid>()?;
  let owner_uid = user::uid_from_uuid(pg_pool, user_uuid).await?;
  let mut tx = pg_pool.begin().await?;

  collaborate::insert_af_collab(
    &mut tx,
    redis_client,
    &workspace_uuid,
    owner_uid,
    &params.object_id,
    &params.raw_data,
    &params.collab_type,
  )
  .await?;
  Ok(())
}

pub async fn delete(
  pg_pool: &PgPool,
  redis_client: ConnectionManager,
  _user_uuid: &Uuid,
  params: &DeleteCollabParams,
) -> Result<(), AppError> {
  params.validate()?;

  // TODO: access control for user_uuid

  collaborate::delete_collab_with_cached_eviction(pg_pool, redis_client, &params.object_id).await?;
  Ok(())
}

pub async fn get_collab_snapshot(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  params: &QuerySnapshotParams,
) -> Result<Vec<u8>, AppError> {
  // TODO: access control for user_uuid

  let blob = collaborate::get_snapshot_blob(pg_pool, params.snapshot_id).await?;
  Ok(blob)
}

pub async fn get_all_collab_snapshot(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  params: &QueryObjectSnapshotParams,
) -> Result<AFCollabSnapshots, AppError> {
  // TODO: access control for user_uuid

  let snapshots = collaborate::get_all_snapshots(pg_pool, &params.object_id).await?;
  Ok(snapshots)
}
