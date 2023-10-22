use anyhow::Result;
use database::{user::create_user_if_not_exists, workspace::select_user_profile_view_by_uuid};
use database_entity::AFUserProfileView;
use gotrue::api::Client;
use serde_json::json;
use shared_entity::app_error::AppError;
use uuid::Uuid;

use shared_entity::dto::auth_dto::UpdateUserParams;
use sqlx::{types::uuid, PgPool};

pub async fn token_verify(
  pg_pool: &PgPool,
  gotrue_client: &Client,
  access_token: &str,
) -> Result<bool, AppError> {
  let user = gotrue_client.user_info(access_token).await?;
  let user_uuid = uuid::Uuid::parse_str(&user.id)?;
  let name = name_from_user_metadata(&user.user_metadata);
  let is_new = create_user_if_not_exists(pg_pool, &user_uuid, &user.email, &name).await?;
  Ok(is_new)
}

pub async fn get_profile(
  pg_pool: &PgPool,
  uuid: &uuid::Uuid,
) -> Result<AFUserProfileView, AppError> {
  let profile = select_user_profile_view_by_uuid(pg_pool, uuid)
    .await?
    .ok_or(sqlx::Error::RowNotFound)?;
  Ok(profile)
}

pub async fn update_user(
  pg_pool: &PgPool,
  user_uuid: Uuid,
  params: UpdateUserParams,
) -> Result<(), AppError> {
  let metadata = params.metadata.map(|m| json!(m.into_inner()));
  Ok(database::user::update_user(pg_pool, &user_uuid, params.name, params.email, metadata).await?)
}

// Best effort to get user's name after oauth
fn name_from_user_metadata(value: &serde_json::Value) -> String {
  value
    .get("name")
    .or(value.get("full_name"))
    .or(value.get("nickname"))
    .and_then(serde_json::Value::as_str)
    .map(str::to_string)
    .unwrap_or_default()
}
