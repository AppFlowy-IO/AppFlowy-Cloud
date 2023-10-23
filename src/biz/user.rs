use anyhow::{Context, Result};
use database::{user::create_user_if_not_exists, workspace::select_user_profile};
use gotrue::api::Client;
use serde_json::json;
use shared_entity::app_error::AppError;
use std::ops::DerefMut;
use uuid::Uuid;

use database::workspace::{select_user_workspace, select_workspace};
use database_entity::dto::{AFUserProfile, AFUserWorkspaceInfo, AFWorkspace};

use shared_entity::dto::auth_dto::UpdateUserParams;
use sqlx::{types::uuid, PgPool};
use tracing::instrument;

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

pub async fn get_profile(pg_pool: &PgPool, uuid: &Uuid) -> Result<AFUserProfile, AppError> {
  let row = select_user_profile(pg_pool, uuid)
    .await?
    .ok_or(sqlx::Error::RowNotFound)?;

  let profile = AFUserProfile::try_from(row)?;
  Ok(profile)
}

#[instrument(skip(pg_pool), err)]
pub async fn get_user_workspace_info(
  pg_pool: &PgPool,
  uuid: &Uuid,
) -> Result<AFUserWorkspaceInfo, AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("failed to acquire the transaction to query the user workspace info")?;
  let row = select_user_profile(txn.deref_mut(), uuid)
    .await?
    .ok_or(sqlx::Error::RowNotFound)?;

  // Get the latest workspace that the user has visited recently
  // TODO(nathan): the visiting_workspace might be None if the user get deleted from the workspace
  let visiting_workspace = AFWorkspace::try_from(
    select_workspace(txn.deref_mut(), &row.latest_workspace_id.unwrap()).await?,
  )?;

  // Get the user profile
  let user_profile = AFUserProfile::try_from(row)?;

  // Get all workspaces that the user can access to
  let workspaces = select_user_workspace(txn.deref_mut(), uuid)
    .await?
    .into_iter()
    .flat_map(|row| AFWorkspace::try_from(row).ok())
    .collect::<Vec<AFWorkspace>>();

  txn
    .commit()
    .await
    .context("failed to commit the transaction to get user workspace info")?;

  Ok(AFUserWorkspaceInfo {
    user_profile,
    visiting_workspace,
    workspaces,
  })
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
