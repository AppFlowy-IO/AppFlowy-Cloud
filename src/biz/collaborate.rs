use database::{
  collaborate::{self, collab_exists},
  user,
};
use database_entity::{AFCollabSnapshots, QueryObjectSnapshotParams, QuerySnapshotParams};
use redis::{aio::ConnectionManager, AsyncCommands};
use shared_entity::{
  dto::{DeleteCollabParams, InsertCollabParams, QueryCollabParams},
  error::AppError,
  error_code::ErrorCode,
};
use sqlx::{types::Uuid, PgPool};
use validator::Validate;

pub async fn create_collab(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  params: &InsertCollabParams,
) -> Result<(), AppError> {
  params.validate()?;

  // TODO: access control for user_uuid

  if collab_exists(pg_pool, &params.object_id).await? {
    return Err(ErrorCode::RecordAlreadyExists.into());
  };

  let workspace_uuid = params.workspace_id.parse::<Uuid>()?;
  let owner_uid = user::uid_from_uuid(pg_pool, user_uuid).await?;
  let mut tx = pg_pool.begin().await?;

  collaborate::insert_af_collab(
    &mut tx,
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
  mut redis_client: ConnectionManager,
  _user_uuid: &Uuid,
  params: QueryCollabParams,
) -> Result<Vec<u8>, AppError> {
  static REDIS_COLLAB_CACHE_DURATION_SEC: usize = 60 * 60;

  params.validate()?;

  // TODO: access control for user_uuid

  // Get from redis
  let redis_key = collab_redis_key(&params.object_id);
  match redis_client.get::<_, Vec<u8>>(redis_key.clone()).await {
    Err(err) => {
      tracing::error!("Get collab from redis failed: {:?}", err);
      Ok(collaborate::get_collab_blob(pg_pool, &params.collab_type, &params.object_id).await?)
    },
    Ok(raw_data) => {
      if !raw_data.is_empty() {
        return Ok(raw_data);
      }

      // get data from postgresql
      let blob =
        collaborate::get_collab_blob(pg_pool, &params.collab_type, &params.object_id).await?;

      // put to redis
      let _ = redis_client
        .set_options::<_, _, ()>(
          redis_key,
          &blob,
          redis::SetOptions::default()
            .with_expiration(redis::SetExpiry::EXAT(REDIS_COLLAB_CACHE_DURATION_SEC)),
        )
        .await
        .map_err(|err| {
          tracing::error!("Set collab to redis failed: {:?}", err);
        });

      Ok(blob)
    },
  }
}

pub async fn update_collab(
  pg_pool: &PgPool,
  mut redis_client: ConnectionManager,
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
    &workspace_uuid,
    owner_uid,
    &params.object_id,
    &params.raw_data,
    &params.collab_type,
  )
  .await?;

  redis_invalidate_collab(&mut redis_client, &params.object_id).await;
  Ok(())
}

pub async fn delete(
  pg_pool: &PgPool,
  mut redis_client: ConnectionManager,
  _user_uuid: &Uuid,
  params: &DeleteCollabParams,
) -> Result<(), AppError> {
  params.validate()?;

  // TODO: access control for user_uuid

  collaborate::delete_collab(pg_pool, &params.object_id).await?;
  redis_invalidate_collab(&mut redis_client, &params.object_id).await;
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

fn collab_redis_key(object_id: &str) -> String {
  format!("collab::{}", object_id)
}

async fn redis_invalidate_collab(redis_client: &mut ConnectionManager, object_id: &str) {
  let _ = redis_client
    .del::<_, ()>(collab_redis_key(object_id))
    .await
    .map_err(|err| {
      tracing::error!(
        "Delete collab from redis failed, id: {}, err: {:?}",
        object_id,
        err
      )
    });
}
