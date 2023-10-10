use database::user;
use database_entity::{
  AFCollabSnapshots, DeleteCollabParams, InsertCollabParams, QueryObjectSnapshotParams,
  QuerySnapshotParams,
};
use shared_entity::{app_error::AppError, error_code::ErrorCode};
use sqlx::{types::Uuid, PgPool};
use validator::Validate;

pub async fn create_collab(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  params: &InsertCollabParams,
) -> Result<(), AppError> {
  params.validate()?;
  // TODO: access control for user_uuid
  if database::collab::collab_exists(pg_pool, &params.object_id).await? {
    return Err(ErrorCode::RecordAlreadyExists.into());
  }
  upsert_collab(pg_pool, user_uuid, params).await
}

pub async fn upsert_collab(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  params: &InsertCollabParams,
) -> Result<(), AppError> {
  params.validate()?;

  // TODO: access control for user_uuid

  let owner_uid = user::uid_from_uuid(pg_pool, user_uuid).await?;
  let mut tx = pg_pool.begin().await?;
  database::collab::insert_af_collab(&mut tx, owner_uid, params).await?;
  tx.commit().await?;
  Ok(())
}

pub async fn get_collab_snapshot(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  params: &QuerySnapshotParams,
) -> Result<Vec<u8>, AppError> {
  // TODO: access control for user_uuid

  let blob = database::collab::get_snapshot_blob(pg_pool, params.snapshot_id).await?;
  Ok(blob)
}

pub async fn get_all_collab_snapshot(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  params: &QueryObjectSnapshotParams,
) -> Result<AFCollabSnapshots, AppError> {
  // TODO: access control for user_uuid
  let snapshots = database::collab::get_all_snapshots(pg_pool, &params.object_id).await?;
  Ok(snapshots)
}
pub async fn delete_collab(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  params: &DeleteCollabParams,
) -> Result<(), AppError> {
  params.validate()?;

  // TODO: access control for user_uuid

  database::collab::delete_collab(pg_pool, &params.object_id).await?;
  Ok(())
}
