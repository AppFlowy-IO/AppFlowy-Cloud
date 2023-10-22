use database::user;

use database_entity::dto::{
  AFCollabMember, AFCollabSnapshots, CollabMemberIdentify, DeleteCollabParams,
  InsertCollabMemberParams, InsertCollabParams, QueryCollabMembers, QueryObjectSnapshotParams,
  QuerySnapshotParams, UpdateCollabMemberParams,
};
use shared_entity::{app_error::AppError, error_code::ErrorCode};
use sqlx::{types::Uuid, PgPool};
use tracing::trace;
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

  let owner_uid = user::select_uid_from_uuid(pg_pool, user_uuid).await?;
  let mut tx = pg_pool.begin().await?;
  database::collab::insert_into_af_collab(&mut tx, &owner_uid, params).await?;
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

/// Create a new collab member
/// If the collab member already exists, return [ErrorCode::RecordAlreadyExists]
/// If the collab member does not exist, create a new one
pub async fn create_collab_member(
  pg_pool: &PgPool,
  params: &InsertCollabMemberParams,
) -> Result<(), AppError> {
  params.validate()?;
  if database::collab::is_collab_member_exists(params.uid, &params.object_id, pg_pool).await? {
    return Err(ErrorCode::RecordAlreadyExists.into());
  }

  trace!("Inserting collab member: {:?}", params);
  database::collab::insert_collab_member(
    params.uid,
    &params.object_id,
    &params.access_level,
    pg_pool,
  )
  .await?;
  Ok(())
}

pub async fn upsert_collab_member(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  params: &UpdateCollabMemberParams,
) -> Result<(), AppError> {
  params.validate()?;

  database::collab::insert_collab_member(
    params.uid,
    &params.object_id,
    &params.access_level,
    pg_pool,
  )
  .await?;
  Ok(())
}

pub async fn get_collab_member(
  pg_pool: &PgPool,
  params: &CollabMemberIdentify,
) -> Result<AFCollabMember, AppError> {
  params.validate()?;
  let collab_member =
    database::collab::select_collab_member(&params.uid, &params.object_id, pg_pool).await?;
  Ok(collab_member)
}

pub async fn delete_collab_member(
  pg_pool: &PgPool,
  params: &CollabMemberIdentify,
) -> Result<(), AppError> {
  params.validate()?;
  database::collab::delete_collab_member(params.uid, &params.object_id, pg_pool).await?;
  Ok(())
}
pub async fn get_collab_member_list(
  pg_pool: &PgPool,
  params: &QueryCollabMembers,
) -> Result<Vec<AFCollabMember>, AppError> {
  params.validate()?;
  let collab_member = database::collab::select_collab_members(&params.object_id, pg_pool).await?;
  Ok(collab_member)
}
