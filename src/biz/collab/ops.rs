use database::user;
use database::workspace::select_user_can_edit_collab;
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
  let blob = database::collab::get_snapshot_blob(pg_pool, params.snapshot_id).await?;
  Ok(blob)
}

pub async fn get_all_collab_snapshot(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  params: &QueryObjectSnapshotParams,
) -> Result<AFCollabSnapshots, AppError> {
  let snapshots = database::collab::get_all_snapshots(pg_pool, &params.object_id).await?;
  Ok(snapshots)
}
pub async fn delete_collab(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  params: &DeleteCollabParams,
) -> Result<(), AppError> {
  params.validate()?;
  database::collab::delete_collab(pg_pool, &params.object_id).await?;
  Ok(())
}

pub async fn require_user_can_edit(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  user_uuid: &Uuid,
  oid: &str,
) -> Result<(), AppError> {
  match select_user_can_edit_collab(pg_pool, user_uuid, workspace_id, oid).await? {
    true => Ok(()),
    false => Err(ErrorCode::NotEnoughPermissions.into()),
  }
}
