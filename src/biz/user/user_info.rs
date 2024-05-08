use anyhow::Context;
use app_error::AppError;
use database::workspace::{select_all_user_workspaces, select_user_profile, select_workspace};
use database_entity::dto::{AFUserProfile, AFUserWorkspaceInfo, AFWorkspace};
use serde_json::json;
use shared_entity::dto::auth_dto::UpdateUserParams;
use shared_entity::response::AppResponseError;
use sqlx::PgPool;
use std::ops::DerefMut;
use tracing::instrument;
use uuid::Uuid;

pub async fn get_profile(pg_pool: &PgPool, uuid: &Uuid) -> anyhow::Result<AFUserProfile, AppError> {
  let row = select_user_profile(pg_pool, uuid)
    .await?
    .ok_or(AppError::RecordNotFound(format!(
      "Can't find the user profile for user: {}",
      uuid
    )))?;

  let profile = AFUserProfile::try_from(row)?;
  Ok(profile)
}

#[instrument(level = "debug", skip(pg_pool), err)]
pub async fn get_user_workspace_info(
  pg_pool: &PgPool,
  uuid: &Uuid,
) -> anyhow::Result<AFUserWorkspaceInfo, AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("failed to acquire the transaction to query the user workspace info")?;
  let row = select_user_profile(txn.deref_mut(), uuid)
    .await?
    .ok_or(AppError::RecordNotFound(format!(
      "Can't find the user profile for {}",
      uuid
    )))?;

  // Get the latest workspace that the user has visited recently
  // TODO(nathan): the visiting_workspace might be None if the user get deleted from the workspace
  let visiting_workspace = AFWorkspace::try_from(
    select_workspace(txn.deref_mut(), &row.latest_workspace_id.unwrap()).await?,
  )?;

  // Get the user profile
  let user_profile = AFUserProfile::try_from(row)?;

  // Get all workspaces that the user can access to
  let workspaces = select_all_user_workspaces(txn.deref_mut(), uuid)
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
) -> anyhow::Result<(), AppResponseError> {
  let metadata = params.metadata.map(|m| json!(m.into_inner()));
  Ok(database::user::update_user(pg_pool, &user_uuid, params.name, params.email, metadata).await?)
}
