use anyhow::Context;
use database::user;
use std::ops::DerefMut;

use app_error::AppError;
use database_entity::dto::{
  AFAccessLevel, AFCollabMember, CollabMemberIdentify, CollabParams, DeleteCollabParams,
  InsertCollabMemberParams, QueryCollabMembers, UpdateCollabMemberParams,
};

use realtime::collaborate::{CollabAccessControl, CollabUserId};
use sqlx::{types::Uuid, PgPool};
use tracing::{event, trace};
use validator::Validate;

pub async fn create_collabs<C>(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &str,
  params_list: Vec<CollabParams>,
  collab_access_control: &C,
) -> Result<(), AppError>
where
  C: CollabAccessControl,
{
  for params in params_list {
    if !params.override_if_exist
      && database::collab::collab_exists(pg_pool, &params.object_id).await?
    {
      // When calling this function, the caller should have already checked if the collab exists.
      return Err(AppError::RecordAlreadyExists(format!(
        "Collab with object_id {} already exists",
        params.object_id
      )));
    }
    collab_access_control
      .cache_collab_access_level(
        CollabUserId::UserUuid(user_uuid),
        &params.object_id,
        AFAccessLevel::FullAccess,
      )
      .await?;
    upsert_collab(pg_pool, user_uuid, workspace_id, vec![params]).await?;
  }
  Ok(())
}

/// Upsert a collab
/// If one of the [CollabParams] validation fails, it will rollback the transaction and return the error
pub async fn upsert_collab(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &str,
  params: Vec<CollabParams>,
) -> Result<(), AppError> {
  let mut tx = pg_pool
    .begin()
    .await
    .context("acquire transaction to upsert collab")?;
  let owner_uid = user::select_uid_from_uuid(tx.deref_mut(), user_uuid).await?;
  for params in params {
    params.validate()?;
    database::collab::insert_into_af_collab(&mut tx, &owner_uid, workspace_id, &params).await?;
  }

  tx.commit()
    .await
    .context("fail to commit the transaction to upsert collab")?;
  Ok(())
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
