use collab_define::CollabType;
use database::{
  collaborate::{self, collab_exists},
  user,
};
use redis::{aio::ConnectionManager, AsyncCommands, RedisError};
use shared_entity::{
  dto::{InsertCollabParams, QueryCollabParams},
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
  user_uuid: &Uuid,
  params: QueryCollabParams,
) -> Result<Vec<u8>, AppError> {
  static REDIS_COLLAB_CACHE_DURATION_SEC: usize = 60 * 60;

  params.validate()?;

  // TODO: access control for user_uuid
  _ = user_uuid;

  // Get from redis
  let redis_key = collab_redis_key(&params.collab_type, &params.object_id);
  let redis_result: Result<Vec<u8>, RedisError> = redis_client.get(redis_key.clone()).await;
  match redis_result {
    Ok(raw_data) => {
      if !raw_data.is_empty() {
        return Ok(raw_data);
      }
      // get data from postgresql, put to redis
      let blob =
        collaborate::get_collab_blob(pg_pool, &params.collab_type, &params.object_id).await?;

      let r: Result<String, redis::RedisError> = redis_client
        .set_options(
          redis_key,
          &blob,
          redis::SetOptions::default()
            .with_expiration(redis::SetExpiry::EXAT(REDIS_COLLAB_CACHE_DURATION_SEC)),
        )
        .await;
      if let Err(err) = r {
        tracing::error!("Set collab to redis failed: {:?}", err);
      }

      Ok(blob)
    },
    Err(err) => {
      tracing::error!("Get collab from redis failed: {:?}", err);
      Ok(collaborate::get_collab_blob(pg_pool, &params.collab_type, &params.object_id).await?)
    },
  }
}

fn collab_redis_key(collab_type: &CollabType, object_id: &str) -> String {
  format!("collab:{}:{}", collab_type, object_id)
}
