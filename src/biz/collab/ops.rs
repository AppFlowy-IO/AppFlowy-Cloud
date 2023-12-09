use anyhow::Context;
use database::user;
use std::ops::DerefMut;

use app_error::AppError;
use database_entity::dto::{
  AFCollabMember, AFCollabSnapshots, CollabMemberIdentify, DeleteCollabParams,
  InsertCollabMemberParams, InsertCollabParams, QueryCollabMembers, QueryObjectSnapshotParams,
  QuerySnapshotParams, UpdateCollabMemberParams,
};

use sqlx::{types::Uuid, PgPool};
use tracing::{event, trace};
use validator::Validate;

pub async fn create_collab(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  params: &InsertCollabParams,
) -> Result<(), AppError> {
  params.validate()?;
  if database::collab::collab_exists(pg_pool, &params.object_id).await? {
    return Err(AppError::RecordAlreadyExists(format!(
      "Collab with object_id {} already exists",
      params.object_id
    )));
  }
  upsert_collab(pg_pool, user_uuid, params).await
}

pub async fn upsert_collab(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  params: &InsertCollabParams,
) -> Result<(), AppError> {
  params.validate()?;

  let mut tx = pg_pool
    .begin()
    .await
    .context("acquire transaction to upsert collab")?;
  let owner_uid = user::select_uid_from_uuid(tx.deref_mut(), user_uuid).await?;
  database::collab::insert_into_af_collab(&mut tx, &owner_uid, params).await?;
  tx.commit()
    .await
    .context("fail to commit the transaction to upsert collab")?;
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
/// If the collab member already exists, return [AppError::RecordAlreadyExists]
/// If the collab member does not exist, create a new one
pub async fn create_collab_member(
  pg_pool: &PgPool,
  params: &InsertCollabMemberParams,
) -> Result<(), AppError> {
  params.validate()?;

  let mut txn = pg_pool
    .begin()
    .await
    .context("acquire transaction to insert collab member")?;

  if !database::collab::is_collab_exists(&params.object_id, txn.deref_mut()).await? {
    return Err(AppError::RecordNotFound(format!(
      "Fail to insert collab member. The Collab with object_id {} does not exist",
      params.object_id
    )));
  }

  if database::collab::is_collab_member_exists(params.uid, &params.object_id, txn.deref_mut())
    .await?
  {
    return Err(AppError::RecordAlreadyExists(format!(
      "Collab member with uid {} and object_id {} already exists",
      params.uid, params.object_id
    )));
  }

  trace!("Inserting collab member: {:?}", params);
  database::collab::insert_collab_member(
    params.uid,
    &params.object_id,
    &params.access_level,
    &mut txn,
  )
  .await?;

  txn
    .commit()
    .await
    .context("fail to commit the transaction to insert collab member")?;
  Ok(())
}

pub async fn upsert_collab_member(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  params: &UpdateCollabMemberParams,
) -> Result<(), AppError> {
  params.validate()?;
  let mut txn = pg_pool
    .begin()
    .await
    .context("acquire transaction to upsert collab member")?;

  if !database::collab::is_collab_exists(&params.object_id, txn.deref_mut()).await? {
    return Err(AppError::RecordNotFound(format!(
      "Fail to upsert collab member. The Collab with object_id {} does not exist",
      params.object_id
    )));
  }

  database::collab::insert_collab_member(
    params.uid,
    &params.object_id,
    &params.access_level,
    &mut txn,
  )
  .await?;

  txn
    .commit()
    .await
    .context("fail to commit the transaction to upsert collab member")?;
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
  event!(
    tracing::Level::DEBUG,
    "Deleting member:{} from {}",
    params.uid,
    params.object_id
  );
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
